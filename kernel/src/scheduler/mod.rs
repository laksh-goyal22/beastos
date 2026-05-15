//! Scheduler — Task Management & Context Switching

pub mod task;
pub mod mlfq;
pub mod switch;

use core::sync::atomic::{AtomicBool, Ordering};
use crate::sync::Spinlock;
use crate::arch::idt::AllRegisters;
use crate::kprintln;
use task::{Task, TaskId, TaskState, BlockReason, NO_TASK};
use mlfq::Scheduler;

pub static SCHEDULER: Spinlock<Scheduler> = Spinlock::new(Scheduler::new());
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Start the scheduler
pub fn run() -> ! {
    ACTIVE.store(true, Ordering::SeqCst);
    kprintln!("  [SCHED] Scheduler running");

    let mut sched = SCHEDULER.lock();
    let slot = sched.pick_next();
    if slot == NO_TASK {
        panic!("No tasks to run!");
    }
    
    let task = sched.tasks[slot].as_ref().unwrap();
    if task.id <= 1 {
        kprintln!("[SCHED] Task {} '{}': r12={:x} rip={:x} rsp={:x} cs={:x} ss={:x}", 
                  task.id, task.name_str(), task.context.r12, task.context.rip, 
                  task.context.rsp, task.context.cs, task.context.ss);
    }
    
    sched.current = slot;
    
    sched.current = slot;
    let task = sched.tasks[slot].as_mut().expect("Picked slot is empty");
    task.state = TaskState::Running;
    
    let page_table = task.page_table;
    let kernel_stack_top = task.stack_virt + crate::scheduler::task::TASK_STACK_SIZE as u64;
    crate::kprintln!("[SCHED_RUN] task={} '{}' kstack_top={:#x} ctx.rsp={:#x} ctx.rip={:#x} ctx.cs={:#x} r12={:#x} r13={:#x}",
        task.id, task.name_str(), kernel_stack_top, task.context.rsp, task.context.rip, task.context.cs,
        task.context.r12, task.context.r13);
    crate::arch::gdt::set_kernel_stack(kernel_stack_top);
    
    let ctx_ptr = &task.context as *const _ as u64;
    drop(sched);

    unsafe {
        crate::arch::paging::load_cr3(page_table, 0);
        
        // Restore initial task context by building an IRETQ frame
        core::arch::asm!(
            "mov rsp, {kernel_stack_top}",
            "mov rdx, {ctx}",

            // Push IRETQ frame
            "push qword ptr [rdx + 152]", // ss
            "push qword ptr [rdx + 144]", // rsp
            "push qword ptr [rdx + 136]", // rflags
            "push qword ptr [rdx + 128]", // cs
            "push qword ptr [rdx + 120]", // rip

            // Restore GPRs
            "mov rax, [rdx + 0]",
            "mov rbx, [rdx + 8]",
            "mov rcx, [rdx + 16]",
            "mov rbp, [rdx + 32]",
            "mov rsi, [rdx + 40]",
            "mov rdi, [rdx + 48]",
            "mov r8,  [rdx + 56]",
            "mov r9,  [rdx + 64]",
            "mov r10, [rdx + 72]",
            "mov r11, [rdx + 80]",
            "mov r12, [rdx + 88]",
            "mov r13, [rdx + 96]",
                "mov r14, [rdx + 104]",
                "mov r15, [rdx + 112]",

                // DEBUG: dump iretq frame: [rsp+16]=rflags [rsp+8]=cs [rsp+0]=rip
                "mov r15, rdx",
                // dump [rsp+8] = cs
                "mov r14, [rsp + 8]",
                "mov dx, 0x3FD",
                "9904: in al, dx",
                "test al, 0x20",
                "jz 9904b",
                "mov dx, 0x3F8",
                "mov al, 'C'; out dx, al", "mov al, 'S'; out dx, al", "mov al, '='; out dx, al",
                "mov rbx, r14",
                "mov cx, 16",
                "9905: mov al, 0x30",
                "mov r14, rbx",
                "shr r14, 60",
                "cmp r14b, 10",
                "jb 9906f",
                "add al, 7",
                "9906: add al, r14b",
                "out dx, al",
                "shl rbx, 4",
                "dec cx",
                "jnz 9905b",
                "mov al, 0x0D; out dx, al", "mov al, 0x0A; out dx, al",
                "mov rdx, r15",

                "mov rdx, [rdx + 24]",

                "iretq",
            kernel_stack_top = in(reg) kernel_stack_top,
            ctx = in(reg) ctx_ptr,
            options(noreturn)
        );
    }
}

pub fn tick(regs: &AllRegisters) {
    let mut sched = SCHEDULER.lock();
    if sched.tick() {
        drop(sched);
        reschedule(regs);
    }
}

pub fn check_reschedule() {
    kprintln!("[RSCHED] Before int 49");
    unsafe { core::arch::asm!("int 49"); }
    kprintln!("[RSCHED] After int 49 (returned)");
}

pub fn current_slot() -> usize {
    SCHEDULER.lock().current
}

pub fn current_pid() -> u64 {
    let sched = SCHEDULER.lock();
    sched.tasks[sched.current].as_ref().map(|t| t.id as u64).unwrap_or(0)
}

pub fn yield_now() {
    if !ACTIVE.load(Ordering::SeqCst) { return; }
    check_reschedule();
}

pub fn block_current(reason: BlockReason) {
    kprintln!("[BLOCK] Blocking current task");
    let mut sched = SCHEDULER.lock();
    let slot = sched.current;
    if let Some(ref mut task) = sched.tasks[slot] {
        task.state = TaskState::Blocked;
        sched.boost_for_reason(slot, reason);
    }
    drop(sched);
    kprintln!("[BLOCK] Calling check_reschedule");
    check_reschedule();
    kprintln!("[BLOCK] check_reschedule returned (will loop)");
}

pub fn wake_task(task_id: TaskId) {
    let mut sched = SCHEDULER.lock();
    let slot = sched.tasks.iter().position(|t| t.as_ref().map(|x| x.id == task_id).unwrap_or(false));
    if let Some(slot) = slot {
        let (priority, awake_pending) = {
            let task = sched.tasks[slot].as_mut().unwrap();
            if let TaskState::Blocked = task.state {
                task.state = TaskState::Ready;
                (task.priority, false)
            } else {
                task.awake_pending = true;
                (0, true)
            }
        };
        if !awake_pending {
            sched.queues[priority].push_back(slot);
        }
    }
}

pub fn wait_task(task_id: TaskId) {
    loop {
        {
            let sched = SCHEDULER.lock();
            let exists = sched.tasks.iter().any(|t| t.as_ref().map(|x| x.id == task_id).unwrap_or(false));
            if !exists { return; }
        }
        yield_now();
    }
}

pub fn exit_current() -> ! {
    let mut sched = SCHEDULER.lock();
    let slot = sched.current;
    sched.kill(slot);
    drop(sched);
    check_reschedule();
    loop { unsafe { core::arch::asm!("hlt") }; }
}

pub fn spawn(entry: extern "C" fn(), name: &str, priority: usize) -> Option<TaskId> {
    let mut sched = SCHEDULER.lock();
    sched.spawn(entry, name, priority)
}

pub fn spawn_user(entry: u64, user_stack_top: u64, name: &str, priority: u8) -> Option<TaskId> {
    let page_table = crate::memory::vmm::create_user_page_table()?;
    spawn_user_with_page_table(entry, user_stack_top, name, priority, page_table)
}

pub fn spawn_user_with_page_table(
    entry: u64,
    user_stack_top: u64,
    name: &str,
    priority: u8,
    page_table: u64,
) -> Option<TaskId> {
    spawn_user_with_page_table_and_trampoline(entry, user_stack_top, name, priority, page_table, 0)
}

pub fn spawn_user_with_page_table_and_trampoline(
    entry: u64,
    user_stack_top: u64,
    name: &str,
    priority: u8,
    page_table: u64,
    trampoline: u64,  // User-space address of trampoline code (0 = use kernel trampoline)
) -> Option<TaskId> {
    let mut sched = SCHEDULER.lock();
    let slot = sched.find_free_slot()?;
    let id = sched.next_id;
    sched.next_id += 1;

    let mut task = Task::new_user(id, entry, user_stack_top, name, priority as usize, page_table)?;
    
    // If a user-space trampoline address is provided, use it instead of kernel trampoline
    if trampoline != 0 {
        task.context.rip = trampoline;
    }
    
    sched.tasks[slot] = Some(task);
    sched.queues[priority as usize].push_back(slot);
    
    Some(id)
}

pub fn replace_current_task(
    entry: u64,
    user_stack_top: u64,
    name: &str,
    page_table: u64,
    argc: u64,
    argv: u64,
) -> ! {
    let (task_id, old_cwd) = {
        let sched = SCHEDULER.lock();
        let old_task = sched.tasks[sched.current].as_ref().unwrap();
        (old_task.id, old_task.cwd.clone())
    };

    let mut new_task = Task::new_user(task_id, entry, user_stack_top, name, 1, page_table)
        .expect("Failed to create new user task during exec");
    
    new_task.cwd = old_cwd;
    new_task.state = TaskState::Running;
    new_task.context.r14 = argc;
    new_task.context.r15 = argv;
    
    let kernel_stack_top = new_task.kernel_stack_top;
    let user_entry = new_task.user_entry;
    let user_stack_top = new_task.user_stack_top;
    let argc_val = new_task.context.r14;
    let argv_val = new_task.context.r15;
    
    {
        let mut sched = SCHEDULER.lock();
        let slot = sched.current;
        sched.tasks[slot] = Some(new_task);
        
        crate::arch::smp::set_current_kernel_stack(kernel_stack_top);
        crate::arch::gdt::set_kernel_stack(kernel_stack_top);
        crate::arch::syscall_entry::set_kernel_stack_ptr(kernel_stack_top);
    }
    
    unsafe {
        crate::arch::paging::load_cr3(page_table, 0);
        core::arch::asm!(
            "mov rsp, {0}",
            "jmp {1}",
            in(reg) kernel_stack_top - 8,
            in(reg) switch::user_entry_trampoline,
            in("r12") user_stack_top,
            in("r13") user_entry,
            in("r14") argc_val,
            in("r15") argv_val,
            options(noreturn)
        );
    }
}

pub fn reschedule(regs: &crate::arch::idt::AllRegisters) {
    let mut sched = SCHEDULER.lock();
    let old_slot = sched.current;

    // Determine if we were in kernel (CPL=0) or user (CPL=3) mode
    let was_cpl0 = (regs.cs & 3) == 0;

    // Debug: print what we're saving
    {
        let name = sched.tasks[old_slot].as_ref().map(|t| t.name_str()).unwrap_or("???");
        kprintln!("[RSCHED] Saving task {} '{}': rip={:#x}, cs={:#x}, user_rsp={:#x} [cpl0={}]", 
            old_slot, name, regs.rip, regs.cs, regs.rsp, was_cpl0);
    }

    let mut should_requeue = false;
    let mut prio = 0;
    if let Some(ref mut task) = sched.tasks[old_slot] {
        if task.state == TaskState::Running {
            task.state = TaskState::Ready;
            should_requeue = true;
            prio = task.priority;
        }
        task.context.rax = regs.rax;
        task.context.rbx = regs.rbx;
        task.context.rcx = regs.rcx;
        task.context.rdx = regs.rdx;
        task.context.rbp = regs.rbp;
        task.context.rsi = regs.rsi;
        task.context.rdi = regs.rdi;
        task.context.r8 = regs.r8;
        task.context.r9 = regs.r9;
        task.context.r10 = regs.r10;
        task.context.r11 = regs.r11;
        task.context.r12 = regs.r12;
        task.context.r13 = regs.r13;
        task.context.r14 = regs.r14;
        task.context.r15 = regs.r15;
        task.context.rip = regs.rip;
        task.context.cs = regs.cs;
        task.context.rflags = regs.rflags;
        if was_cpl0 {
            // CPL=0: int 49 or timer int fired while in kernel (inside a syscall).
            // CPU pushed only 3 values (RIP, CS, RFLAGS) — no SS:RSP.
            // frame_ptr points at AllRegisters (15 GPRs + error + RIP+CS+RFLAGS + 2 stale slots).
            // Pre-interrupt kernel RSP S = frame_ptr + 128 + 24 = frame_ptr + 152.
            // When restoring, we use a 3-push IRETQ (no SS:RSP), so context.rsp = S.
            let frame_ptr = regs as *const AllRegisters as u64;
            task.context.rsp = frame_ptr + 152;   // S = pre-interrupt kernel RSP
            task.context.ss = 0x10;
            crate::kprintln!("[RSCHED_SAVE] cpl0 task={} frame_ptr={:#x} saved_rsp={:#x} rip={:#x}",
                old_slot, frame_ptr, task.context.rsp, task.context.rip);
        } else {
            // CPL=3: interrupt fired from user mode. CPU pushed 5 values (SS, RSP, RFLAGS, CS, RIP).
            // The user RSP is at regs.rsp (AllRegisters offset 152).
            // When restoring, we use a 5-push IRETQ, and context.rsp = user RSP.
            task.context.rsp = regs.rsp;
            task.context.ss = regs.ss;
        }
    }

    if should_requeue {
        sched.queues[prio].push_back(old_slot);
    }

    let new_slot = sched.pick_next();
    if new_slot == NO_TASK || new_slot == old_slot {
        if let Some(ref mut t) = sched.tasks[old_slot] {
            t.state = TaskState::Running;
        }
        return;
    }

    sched.current = new_slot;
    let task = sched.tasks[new_slot].as_mut().expect("Picked empty slot");
    task.state = TaskState::Running;

    let ctx_ptr = &task.context as *const _ as u64;
    let new_cr3 = task.page_table;
    let kernel_stack_top = task.kernel_stack_top;
    let cs = task.context.cs;
    
    crate::kprintln!("[RSCHED_SW] -> task={} '{}' kstack_top={:#x} ctx.rsp={:#x} ctx.rip={:#x} ctx.cs={:#x} r12={:#x} r13={:#x}",
        task.id, task.name_str(), kernel_stack_top, task.context.rsp, task.context.rip, task.context.cs,
        task.context.r12, task.context.r13);
    
    crate::arch::smp::set_current_kernel_stack(kernel_stack_top);
    crate::arch::gdt::set_kernel_stack(kernel_stack_top);
    
    drop(sched);

    unsafe {
        crate::arch::paging::load_cr3(new_cr3, 0);

        let is_target_kernel = (cs & 3) == 0;

        if is_target_kernel {
            // Kernel target: use same 5-push IRETQ as user (SS=0x10, RSP from context)
            // This avoids a pre-existing GPF bug with 3-push IRETQ on kernel→kernel switches
            let rsp_from_kstack = kernel_stack_top;
            core::arch::asm!(
                "mov rsp, r8",
                // Push SS (kernel data segment)
                "push qword ptr [rdx + 152]",
                // Push RSP from saved context.rsp
                "push qword ptr [rdx + 144]",
                // Push RFLAGS, CS, RIP for IRETQ
                "push qword ptr [rdx + 136]",
                "push qword ptr [rdx + 128]",
                "push qword ptr [rdx + 120]",
                // Restore all GP registers from context
                "mov rax, [rdx + 0]",
                "mov rbx, [rdx + 8]",
                "mov rcx, [rdx + 16]",
                "mov rbp, [rdx + 32]",
                "mov rsi, [rdx + 40]",
                "mov rdi, [rdx + 48]",
                "mov r8,  [rdx + 56]",
                "mov r9,  [rdx + 64]",
                "mov r10, [rdx + 72]",
                "mov r11, [rdx + 80]",
                "mov r12, [rdx + 88]",
                "mov r13, [rdx + 96]",
                "mov r14, [rdx + 104]",
                "mov r15, [rdx + 112]",
                "mov rdx, [rdx + 24]",
                // Reload segment registers (user mode may have dirtied them)
                "push rax",
                "mov ax, 0x10",
                "mov ds, ax",
                "mov es, ax",
                "mov ss, ax",
                "xor ax, ax",
                "mov fs, ax",
                "pop rax",
                "iretq",
                in("rdx") ctx_ptr,
                in("r8") rsp_from_kstack,
                options(noreturn)
            );
        } else {
            // User target: 5-push IRETQ (SS, RSP, RFLAGS, CS, RIP)
            core::arch::asm!(
                // rdx holds ctx_ptr, r8 holds kernel_stack_top
                "mov rsp, r8",
                // Push SS
                "push qword ptr [rdx + 152]",
                // Push user RSP from saved context.rsp (NOT r12)
                "push qword ptr [rdx + 144]",
                // Push RFLAGS
                "push qword ptr [rdx + 136]",
                // Push CS
                "push qword ptr [rdx + 128]",
                // Push RIP
                "push qword ptr [rdx + 120]",
                // Restore all GP registers
                "mov rax, [rdx + 0]",
                "mov rbx, [rdx + 8]",
                "mov rcx, [rdx + 16]",
                "mov rbp, [rdx + 32]",
                "mov rsi, [rdx + 40]",
                "mov rdi, [rdx + 48]",
                "mov r8,  [rdx + 56]",
                "mov r9,  [rdx + 64]",
                "mov r10, [rdx + 72]",
                "mov r11, [rdx + 80]",
                "mov r12, [rdx + 88]",
                "mov r13, [rdx + 96]",
                "mov r14, [rdx + 104]",
                "mov r15, [rdx + 112]",
                "mov rdx, [rdx + 24]",
                // Reload segment registers (user mode may have dirtied them)
                "push rax",
                "mov ax, 0x10",
                "mov ds, ax",
                "mov es, ax",
                "mov ss, ax",
                "xor ax, ax",
                "mov fs, ax",
                "pop rax",
                "iretq",
                in("rdx") ctx_ptr,
                in("r8") kernel_stack_top,
                options(noreturn)
            );
        }
    }
}

pub fn init() {
    kprintln!("  [SCHED] Initialized");
}

#[no_mangle]
pub extern "C" fn task_exit_trampoline() -> ! {
    exit_current();
}