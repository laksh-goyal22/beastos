//! Interrupt Descriptor Table (IDT) for x86_64
//!
//! Registers handlers for:
//! - CPU exceptions (page fault, GPF, double fault, etc.)
//! - Hardware IRQs (timer, keyboard, serial, etc.)

use core::arch::asm;
use crate::kprintln;
use crate::sync::Spinlock;
use crate::arch::asm::interrupt_stubs::*;
use crate::drivers::keyboard;
use crate::drivers::pit;
use crate::drivers::pic;

/// Number of IDT entries (256 vectors).
const IDT_ENTRIES: usize = 256;

/// IDT entry (16 bytes on x86_64).
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_hi: u32,
    _reserved: u32,
}

impl IdtEntry {
    const fn empty() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_hi: 0,
            _reserved: 0,
        }
    }

    fn set_handler(&mut self, handler: u64, selector: u16, ist_index: u8, ring: u8) {
        self.offset_low = handler as u16;
        self.offset_mid = (handler >> 16) as u16;
        self.offset_hi = (handler >> 32) as u32;
        self.selector = selector;
        self.ist = ist_index;
        self.type_attr = 0x8E | ((ring & 3) << 5);
        self._reserved = 0;
    }
}

/// The IDT (256 entries).
static IDT: Spinlock<[IdtEntry; IDT_ENTRIES]> = Spinlock::new([IdtEntry::empty(); IDT_ENTRIES]);

/// IDT Pointer for `lidt` instruction.
#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

/// All registers saved by our assembly stubs.
#[repr(C)]
#[derive(Debug)]
pub struct AllRegisters {
    pub rax: u64, pub rbx: u64, pub rcx: u64, pub rdx: u64,
    pub rbp: u64, pub rsi: u64, pub rdi: u64, pub r8: u64,
    pub r9: u64, pub r10: u64, pub r11: u64, pub r12: u64,
    pub r13: u64, pub r14: u64, pub r15: u64,
    pub error_code: u64,
    pub rip: u64, pub cs: u64, pub rflags: u64, pub rsp: u64, pub ss: u64,
}

#[repr(C)]
#[derive(Debug)]
#[allow(dead_code)]
pub struct InterruptStackFrame {
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

/// Initialize the IDT with exception and IRQ handlers.
pub fn init() {
    let mut idt = IDT.lock();

    // Exception handlers (vectors 0-31)
    idt[0].set_handler(stub_div_error as *const () as u64, 0x08, 0, 0);
    idt[6].set_handler(stub_invalid_opcode as *const () as u64, 0x08, 0, 0);
    idt[8].set_handler(stub_double_fault as *const () as u64, 0x08, 1, 0);
    idt[13].set_handler(stub_gp_fault as *const () as u64, 0x08, 0, 0);
    idt[14].set_handler(stub_page_fault as *const () as u64, 0x08, 0, 0);

    // IRQ handlers (vectors 32-47)
    idt[32].set_handler(stub_timer as *const () as u64, 0x08, 0, 0);
    idt[33].set_handler(stub_keyboard as *const () as u64, 0x08, 0, 0);
    idt[36].set_handler(stub_irq_4 as *const () as u64, 0x08, 0, 0);
    idt[44].set_handler(stub_mouse as *const () as u64, 0x08, 0, 0);
    idt[48].set_handler(stub_irq_16 as *const () as u64, 0x08, 0, 0);
    idt[49].set_handler(stub_reschedule as *const () as u64, 0x08, 0, 0);

    unsafe {
        let idt_ptr = IdtPointer {
            limit: (core::mem::size_of::<[IdtEntry; IDT_ENTRIES]>() - 1) as u16,
            base: (&*idt as *const _) as u64,
        };
        asm!("lidt [{}]", in(reg) &idt_ptr, options(nostack));
    }

    kprintln!("  [IDT] Loaded {} entries, interrupts enabled", IDT_ENTRIES);
}

#[no_mangle]
pub extern "C" fn serial_handler_rust(_regs: &AllRegisters) {
    crate::drivers::console::handle_serial_interrupt();
    pic::send_eoi(4);
}

// ---------------------------------------------------------------------------
// Exception Handlers
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn division_error_handler_rust(regs: &AllRegisters) {
    if regs.cs & 3 == 3 {
        kprintln!("[EXCEPTION] Division Error at {:#x} — killing user task", regs.rip);
        crate::scheduler::exit_current();
    } else {
        kprintln!("[EXCEPTION] Division Error at {:#x}", regs.rip);
        loop { unsafe { asm!("hlt") }; }
    }
}

#[no_mangle]
pub extern "C" fn invalid_opcode_handler_rust(regs: &AllRegisters) {
    if regs.cs & 3 == 3 {
        kprintln!("[EXCEPTION] Invalid Opcode at {:#x} — killing user task", regs.rip);
        crate::scheduler::exit_current();
    } else {
        kprintln!("[EXCEPTION] Invalid Opcode at {:#x}", regs.rip);
        loop { unsafe { asm!("hlt") }; }
    }
}

pub static KERNEL_PML4: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Dump a kernel backtrace by walking the RBP chain.
/// Starts from the given RBP; prints up to `max_frames` entries.
/// Returns the final RBP encountered (may be 0 or an invalid user address).
fn dump_kernel_backtrace(rbp: u64, max_frames: usize) {
    let mut frame_rbp = rbp;
    for i in 0..max_frames {
        if frame_rbp == 0 || frame_rbp < 0xFFFF800000000000 {
            break;
        }
        unsafe {
            let ret_addr = *(frame_rbp as *const u64).offset(1);
            kprintln!("    #{:02} {:#018x}", i, ret_addr);
            frame_rbp = *(frame_rbp as *const u64);
        }
    }
}

#[no_mangle]
pub extern "C" fn page_fault_handler_rust(frame_ptr: *const u64, error_code: u64) {
    // Early debug: raw serial write before anything complex
    unsafe {
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b'\r', options(nomem, nostack));
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b'\n', options(nomem, nostack));
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b'P', options(nomem, nostack));
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b'F', options(nomem, nostack));
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b'!', options(nomem, nostack));
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b'\r', options(nomem, nostack));
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b'\n', options(nomem, nostack));
    }

    let cr2: u64;
    unsafe { asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack)) };
    
    let regs = if !frame_ptr.is_null() {
        Some(unsafe { &*(frame_ptr as *const AllRegisters) })
    } else {
        None
    };

    // Determine CPL from saved CS
    let cpl = regs.map(|r| (r.cs & 3) as u64).unwrap_or(0);
    let is_user_fault = (error_code & (1 << 2)) != 0;
    
    // Get the page table to use
    let cr3_val = if cpl == 3 || is_user_fault {
        crate::arch::paging::read_cr3() & !0xFFF
    } else {
        KERNEL_PML4.load(core::sync::atomic::Ordering::Relaxed)
    };
    
    let mut vmm = crate::memory::vmm::VirtualMemoryManager::new(cr3_val);

    if vmm.handle_fault(cr2, error_code).is_ok() {
        return;
    }

    let pf_present = error_code & (1 << 0) != 0;
    let pf_write  = error_code & (1 << 1) != 0;
    let pf_user   = error_code & (1 << 2) != 0;
    let pf_resvd  = error_code & (1 << 3) != 0;
    let pf_inst   = error_code & (1 << 4) != 0;

    kprintln!("");
    kprintln!("====================================================");
    kprintln!("[EXCEPTION] Page Fault!");
    kprintln!("  Faulting addr (CR2): {:#018x}", cr2);
    kprintln!("  Error code: {:#x}", error_code);
    if let Some(r) = regs {
        kprintln!("  RIP at fault: {:#018x}", r.rip);
    }
    kprintln!("    Present={} Write={} User={} Reserved={} InstFetch={}",
              pf_present as u8, pf_write as u8, pf_user as u8,
              pf_resvd as u8, pf_inst as u8);
    
    if let Some(r) = regs {
        let saved_rsp =         if cpl == 3 { r.rsp } else { 0 };
        kprintln!("");
        kprintln!("  --- Registers ---");
        kprintln!("  RAX: {:#018x}  RBX: {:#018x}", r.rax, r.rbx);
        kprintln!("  RCX: {:#018x}  RDX: {:#018x}", r.rcx, r.rdx);
        kprintln!("  RBP: {:#018x}  RSP: {:#018x}", r.rbp, saved_rsp);
        kprintln!("  RSI: {:#018x}  RDI: {:#018x}", r.rsi, r.rdi);
        kprintln!("  R8:  {:#018x}  R9:  {:#018x}", r.r8,  r.r9);
        kprintln!("  R10: {:#018x}  R11: {:#018x}", r.r10, r.r11);
        kprintln!("  R12: {:#018x}  R13: {:#018x}", r.r12, r.r13);
        kprintln!("  R14: {:#018x}  R15: {:#018x}", r.r14, r.r15);
        kprintln!("");
        kprintln!("  RIP: {:#018x}  CS: {:#04x}  RFLAGS: {:#018x}  SS: {:#04x}",
                  r.rip, r.cs as u16, r.rflags, r.ss as u16);
        kprintln!("  CPL: {}", cpl);

        // Dump instruction bytes at RIP
        unsafe {
            let instr = core::slice::from_raw_parts(r.rip as *const u8, 16);
            kprintln!("  Instr at RIP: {:02x?}", instr);
        }

        // Task info
        let pid = crate::scheduler::current_pid();
        kprintln!("  PID: {}", pid);

        // Kernel backtrace (only useful for kernel-mode faults)
        if cpl == 0 && r.rbp >= 0xFFFF800000000000 {
            kprintln!("");
            kprintln!("  --- Kernel Backtrace ---");
            dump_kernel_backtrace(r.rbp, 24);
        }
        
        // Dump kernel stack top for kernel faults
        if cpl == 0 && !frame_ptr.is_null() {
            let frame_addr = frame_ptr as u64;
            kprintln!("");
            kprintln!("  --- Stack near frame ---");
            kprintln!("  frame_ptr: {:#018x}", frame_addr);
            // Dump 16 u64s below the frame_ptr (deeper into kernel stack)
            // and 8 u64s above (return path)
            unsafe {
                for i in (0..=16u64).rev() {
                    let off = i * 8;
                    let val = *( (frame_addr - off) as *const u64 );
                    kprintln!("  [{:#018x}] = {:#018x}", frame_addr - off, val);
                }
                kprintln!("  --- frame_ptr ---");
                for i in 0..8u64 {
                    let off = i * 8;
                    let val = *( (frame_addr + off) as *const u64 );
                    kprintln!("  [{:#018x}] = {:#018x}", frame_addr + off, val);
                }
            }
        }
    }
    
    kprintln!("====================================================");
    kprintln!("");

    if cpl == 3 {
        kprintln!("[PF] User-mode fault — killing task");
        crate::scheduler::exit_current();
    } else {
        kprintln!("[HALT] CPU halted.");
        loop { unsafe { asm!("hlt") }; }
    }
}

#[no_mangle]
pub extern "C" fn general_protection_handler_rust(frame_ptr: *const u64, error_code: u64) {
    // Early debug: raw serial write
    unsafe {
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b'\r', options(nomem, nostack));
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b'\n', options(nomem, nostack));
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b'G', options(nomem, nostack));
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b'P', options(nomem, nostack));
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b'F', options(nomem, nostack));
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b'!', options(nomem, nostack));
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b'\r', options(nomem, nostack));
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b'\n', options(nomem, nostack));
    }

    let regs = if !frame_ptr.is_null() {
        Some(unsafe { &*(frame_ptr as *const AllRegisters) })
    } else {
        None
    };
    let cpl = regs.map(|r| (r.cs & 3) as u64).unwrap_or(0);

    kprintln!("[EXCEPTION] General Protection Fault!");
    kprintln!("  Error code: {:#x}", error_code);
    kprintln!("  CPL: {}", cpl);
    
    if let Some(r) = regs {
        kprintln!("  RIP: {:#x}, CS: {:#x}", r.rip, r.cs);
    }
    
    if cpl == 3 {
        kprintln!("[GPF] User-mode fault — killing task");
        crate::scheduler::exit_current();
    } else {
        loop { unsafe { core::arch::asm!("hlt") }; }
    }
}

#[no_mangle]
pub extern "C" fn double_fault_handler_rust(regs: &AllRegisters, _error_code: u64) -> ! {
    let cr2: u64;
    unsafe { asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack)) };
    kprintln!("[FATAL] Double Fault!");
    kprintln!("  RIP: {:#x}  CS: {:#x}  RFLAGS: {:#x}", regs.rip, regs.cs, regs.rflags);
    kprintln!("  RSP: {:#x}  SS: {:#x}  CR2: {:#x}", regs.rsp, regs.ss, cr2);
    kprintln!("  RAX: {:#x}  RBX: {:#x}  RCX: {:#x}  RDX: {:#x}", regs.rax, regs.rbx, regs.rcx, regs.rdx);
    kprintln!("  RSI: {:#x}  RDI: {:#x}  RBP: {:#x}", regs.rsi, regs.rdi, regs.rbp);
    kprintln!("  R8: {:#x}  R9: {:#x}  R10: {:#x}  R11: {:#x}", regs.r8, regs.r9, regs.r10, regs.r11);
    kprintln!("  R12: {:#x}  R13: {:#x}  R14: {:#x}  R15: {:#x}", regs.r12, regs.r13, regs.r14, regs.r15);
    kprintln!("  Error code: {:#x}", _error_code);
    kprintln!("  Saved frame points to kernel stack {:#x}", regs.rsp as u64);
    // Dump stack near the IST frame to see what was on the stack
    unsafe {
        let frame_ptr = regs as *const AllRegisters as u64;
        kprintln!("  AllRegisters at: {:#x}", frame_ptr);
        for i in (0..8u64).rev() {
            let off = i * 8;
            let val = *( (frame_ptr - off) as *const u64 );
            kprintln!("  [{:#018x}] = {:#018x}", frame_ptr - off, val);
        }
    }
    loop { unsafe { asm!("hlt") }; }
}

// ---------------------------------------------------------------------------
// IRQ Handlers
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn timer_handler_rust(frame_ptr: *const u64, _error_code: u64) {
    pit::tick();
    if let Some(mut console) = crate::drivers::console::CONSOLE.try_lock() {
        console.tick();
    }
    pic::send_eoi(0);
    let regs = unsafe { &*(frame_ptr as *const AllRegisters) };
    crate::scheduler::tick(regs);
}

#[no_mangle]
pub extern "C" fn keyboard_handler_rust(_regs: &AllRegisters) {
    // Call keyboard driver to process scancode and push to syscall buffer
    keyboard::handle_scancode();
    pic::send_eoi(1);
}

#[no_mangle]
pub extern "C" fn mouse_handler_rust(_regs: &AllRegisters) {
    crate::drivers::mouse::handle_mouse_interrupt();
    pic::send_eoi(12);
}

#[no_mangle]
pub extern "C" fn reschedule_handler_rust(frame_ptr: *const u64, _error_code: u64) {
    // Read-only reference — we must NOT mutate the stack above the CPU
    // frame (offsets > 144 from frame_ptr = SS and RSP slots), because
    // for CPL=0 the CPU does NOT push SS:RSP — those slots contain the
    // calling function's stack data (e.g. the return address at +160).
    // Mutating them corrupts the caller, causing RIP=0x0 or RIP=0x10.
    let regs = unsafe { &*(frame_ptr as *const AllRegisters) };
    // Note: the SS/RSP fields in AllRegisters are valid only for CPL=3
    // (privilege change). For CPL=0 they contain garbage, but
    // reschedule() handles CPL=0 by computing rsp from frame_ptr+152
    // and hardcoding ss=0x10, so we don't need to fix them here.
    crate::scheduler::reschedule(regs);
}