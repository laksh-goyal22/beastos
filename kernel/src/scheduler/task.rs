//! Task Control Block and CPU Context

use crate::memory::{pmm, vmm};
use crate::scheduler::switch;

/// Page size (4 KiB).
const PAGE_SIZE: usize = 4096;

/// Kernel stack: 32 pages = 128 KiB per task.
pub const TASK_STACK_PAGES: usize = 32;
pub const TASK_STACK_SIZE: usize = TASK_STACK_PAGES * PAGE_SIZE;

/// User stack: 8 pages = 32 KiB per user task.
pub const USER_STACK_PAGES: usize = 8;
pub const USER_STACK_SIZE: usize = USER_STACK_PAGES * PAGE_SIZE;

/// Maximum concurrent tasks.
pub const MAX_TASKS: usize = 64;

/// MLFQ priority levels (0 = highest, 5 = background).
pub const PRIORITY_LEVELS: usize = 6;

/// Task identifier.
pub type TaskId = usize;

/// Sentinel value: no task selected.
pub const NO_TASK: TaskId = usize::MAX;

/// Task lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Blocked,
    Dead,
}

/// Why a task blocked — enables SPSC-aware priority boosting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    SpscRingEmpty { producer_slot: TaskId },
    SpscRingFull { consumer_slot: TaskId },
    MutexContention,
    Sleep,
    FileRead,
    Yield,
    Waiting,
}

/// Saved CPU context — all general purpose registers + segments + stack pointer.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Context {
    pub rax: u64, pub rbx: u64, pub rcx: u64, pub rdx: u64,
    pub rbp: u64, pub rsi: u64, pub rdi: u64, pub r8: u64,
    pub r9: u64, pub r10: u64, pub r11: u64, pub r12: u64,
    pub r13: u64, pub r14: u64, pub r15: u64,
    pub rip: u64, pub cs: u64, pub rflags: u64, pub rsp: u64, pub ss: u64,
}

impl Context {
    pub const fn empty() -> Self {
        Self {
            rax: 0, rbx: 0, rcx: 0, rdx: 0,
            rbp: 0, rsi: 0, rdi: 0, r8: 0,
            r9: 0, r10: 0, r11: 0, r12: 0,
            r13: 0, r14: 0, r15: 0,
            rip: 0, cs: 0, rflags: 0, rsp: 0, ss: 0,
        }
    }
}

use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;
use crate::fs::vfs::File;
use crate::sync::Spinlock;

/// Task Control Block.
pub struct Task {
    pub id: TaskId,
    pub state: TaskState,
    pub priority: usize,
    pub base_priority: usize,
    pub ticks: u64,
    pub ticks_used: u64,
    pub context: Context,
    pub page_table: u64,
    pub kernel_stack_top: u64,
    pub stack_phys: u64,
    pub stack_virt: u64,
    pub user_entry: u64,
    pub user_stack_top: u64,
    pub name: [u8; 32],
    pub name_len: usize,
    pub awake_pending: bool,
    pub waiting_on: Option<TaskId>,
    pub cwd: String,
    pub fds: Spinlock<Vec<Option<Box<dyn File>>>>,
    pub brk: u64,          // Program break (end of heap, for sys_brk)
}

impl Task {
    pub fn new_with_args(
        id: TaskId,
        entry: extern "C" fn(u64, u64),
        arg1: u64,
        arg2: u64,
        name_str: &str,
        priority: usize,
        page_table: u64,
    ) -> Option<Self> {
        let phys = pmm::alloc_contiguous(TASK_STACK_PAGES)?;
        let virt = vmm::phys_to_virt(phys);
        unsafe { core::ptr::write_bytes(virt as *mut u8, 0, TASK_STACK_SIZE); }

        let stack_top = virt + TASK_STACK_SIZE as u64;
        
        // Set up initial stack frame
        // Stack grows downward, so we start at the top and subtract
        let mut rsp = stack_top;
        
        // Push a dummy return address (in case the function returns, it will crash)
        rsp -= 8;
        unsafe { *(rsp as *mut u64) = 0xdeadbeefdeadbeef; }
        
        // Push the function arguments (rdi, rsi are set from r12, r13 in our trampoline)
        // But since we're jumping directly, we'll set rdi/rsi in the asm
        
        let mut name_buf = [0u8; 32];
        let len = name_str.len().min(32);
        name_buf[..len].copy_from_slice(&name_str.as_bytes()[..len]);

        Some(Self {
            id,
            state: TaskState::Ready,
            priority,
            base_priority: priority,
            ticks: 0,
            ticks_used: 0,
            context: Context {
                rax: 0, rbx: 0, rcx: 0, rdx: 0,
                rbp: 0, rsi: 0, rdi: 0, r8: 0,
                r9: 0, r10: 0, r11: 0, 
                r12: entry as u64,  // Entry point function pointer
                r13: arg1, r14: arg2, r15: 0,
                rip: switch::task_start_trampoline as *const () as u64,
                cs: 0x08,
                rflags: 0x202,
                rsp: rsp,
                ss: 0x10,
            },
            page_table,
            kernel_stack_top: stack_top,
            stack_phys: phys,
            stack_virt: virt,
            user_entry: arg1,
            user_stack_top: arg2,
            name: name_buf,
            name_len: len,
            awake_pending: false,
            waiting_on: None,
            cwd: String::from("/"),
            fds: Spinlock::new(alloc::vec![None, None, None]),
            brk: 0,
        })
    }

    pub fn new(
        id: TaskId,
        entry: extern "C" fn(),
        name_str: &str,
        priority: usize,
        page_table: u64,
    ) -> Option<Self> {
        let entry_wrapped = unsafe { core::mem::transmute::<extern "C" fn(), extern "C" fn(u64, u64)>(entry) };
        Self::new_with_args(id, entry_wrapped, 0, 0, name_str, priority, page_table)
    }
    
    pub fn new_user(
        id: TaskId,
        entry: u64,
        user_stack_top: u64,
        name_str: &str,
        priority: usize,
        page_table_cr3: u64,
    ) -> Option<Self> {
        let kernel_stack_phys = pmm::alloc_contiguous(TASK_STACK_PAGES)?;
        let kernel_stack_virt = vmm::phys_to_virt(kernel_stack_phys);
        unsafe { core::ptr::write_bytes(kernel_stack_virt as *mut u8, 0, TASK_STACK_SIZE); }

        let kernel_stack_top = kernel_stack_virt + TASK_STACK_SIZE as u64;

        crate::kprintln!("[NEW_USER] id={} entry={:#x} user_stack={:#x} kstack_phys={:#x} kstack_virt={:#x} kstack_top={:#x}",
            id, entry, user_stack_top, kernel_stack_phys, kernel_stack_virt, kernel_stack_top);
        
        // Write user entry to stack for user trampoline
        let ret_addr_ptr = (kernel_stack_top - 8) as *mut u64;
        unsafe {
            ret_addr_ptr.write(entry);
        }

        let mut name_buf = [0u8; 32];
        let len = name_str.len().min(32);
        name_buf[..len].copy_from_slice(&name_str.as_bytes()[..len]);

        const HEAP_START: u64 = 0x0000_2000_0000_0000;

        Some(Self {
            id,
            state: TaskState::Ready,
            priority,
            base_priority: priority,
            ticks: 0,
            ticks_used: 0,
            kernel_stack_top,
            context: Context {
                rax: 0, rbx: 0, rcx: 0, rdx: 0,
                rbp: 0, rsi: 0, rdi: 0, r8: 0,
                r9: 0, r10: 0, r11: 0,
                r12: user_stack_top,
                r13: entry,
                r14: 0,
                r15: 0,
                rip: switch::user_entry_trampoline as *const () as u64,
                cs: 0x23,  // User code segment
                rflags: 0x202,
                rsp: user_stack_top,
                ss: 0x1b,  // User data segment
            },
            page_table: page_table_cr3,
            stack_phys: kernel_stack_phys,
            stack_virt: kernel_stack_virt,
            user_entry: entry,
            user_stack_top,
            name: name_buf,
            name_len: len,
            awake_pending: false,
            waiting_on: None,
            cwd: String::from("/"),
            fds: Spinlock::new(alloc::vec![None, None, None]),
            brk: HEAP_START,
        })
    }

    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("???")
    }
}