//! Interrupt Descriptor Table (IDT) for x86_64
//!
//! Registers handlers for:
//! - CPU exceptions (page fault, GPF, double fault, etc.)
//! - Hardware IRQs (timer, keyboard, serial, etc.)
//! - Software interrupts (system calls)

use core::arch::asm;
use crate::kprintln;

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
    /// Create an empty (not present) IDT entry.
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

    /// Set handler for this IDT entry.
    ///
    /// # Arguments
    /// - `handler`: Function pointer to the ISR
    /// - `selector`: Code segment selector (0x08 for kernel)
    /// - `ist_index`: Interrupt Stack Table index (0 = none)
    /// - `ring`: DPL (0 = kernel only, 3 = user callable)
    fn set_handler(&mut self, handler: u64, selector: u16, ist_index: u8, ring: u8) {
        self.offset_low = handler as u16;
        self.offset_mid = (handler >> 16) as u16;
        self.offset_hi = (handler >> 32) as u32;
        self.selector = selector;
        self.ist = ist_index;
        // Type: 0xE = 64-bit Interrupt Gate, Present bit, DPL
        self.type_attr = 0x8E | ((ring & 3) << 5);
        self._reserved = 0;
    }
}

use crate::sync::Spinlock;

/// The IDT (256 entries).
static IDT: Spinlock<[IdtEntry; IDT_ENTRIES]> = Spinlock::new([IdtEntry::empty(); IDT_ENTRIES]);

/// IDT Pointer for `lidt` instruction.
#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

/// Initialize the IDT with exception and IRQ handlers.
pub fn init() {
    let mut idt = IDT.lock();

    // Exception handlers (vectors 0-31)
    register_exception_handlers(&mut idt);

    // Hardware IRQ handlers (vectors 32+)
    register_irq_handlers(&mut idt);

    unsafe {
        // Load the IDT
        let idt_ptr = IdtPointer {
            limit: (core::mem::size_of::<[IdtEntry; IDT_ENTRIES]>() - 1) as u16,
            base: (&*idt as *const _) as u64,
        };
        asm!("lidt [{}]", in(reg) &idt_ptr, options(nostack));

        // Enable interrupts
        asm!("sti", options(nomem, nostack));
    }

    kprintln!("  [IDT] Loaded {} entries, interrupts enabled", IDT_ENTRIES);
}

/// Register CPU exception handlers (vectors 0-31).
fn register_exception_handlers(idt: &mut [IdtEntry; IDT_ENTRIES]) {
    // Vector 0: Division Error
    idt[0].set_handler(
        division_error_handler as *const () as u64,
        0x08, 0, 0,
    );

    // Vector 6: Invalid Opcode
    idt[6].set_handler(
        invalid_opcode_handler as *const () as u64,
        0x08, 0, 0,
    );

    // Vector 8: Double Fault (uses IST1 when TSS is set up)
    idt[8].set_handler(
        double_fault_handler as *const () as u64,
        0x08, 1, 0,  // IST=1 (see gdt.rs)
    );

    // Vector 13: General Protection Fault
    idt[13].set_handler(
        general_protection_handler as *const () as u64,
        0x08, 0, 0,
    );

    // Vector 14: Page Fault
    idt[14].set_handler(
        page_fault_handler as *const () as u64,
        0x08, 0, 0,
    );
}

// ---------------------------------------------------------------------------
// Exception Handler Stubs
// ---------------------------------------------------------------------------

/// Interrupt stack frame pushed by CPU on interrupt/exception.
#[derive(Debug)]
#[repr(C)]
pub struct InterruptStackFrame {
    pub instruction_pointer: u64,
    pub code_segment: u64,
    pub rflags: u64,
    pub stack_pointer: u64,
    pub stack_segment: u64,
}

/// Division Error (#DE, vector 0)
pub extern "x86-interrupt" fn division_error_handler(
    stack_frame: &InterruptStackFrame,
) {
    kprintln!("[EXCEPTION] Division Error at {:#x}", stack_frame.instruction_pointer);
    loop { unsafe { asm!("hlt") }; }
}

/// Invalid Opcode (#UD, vector 6)
pub extern "x86-interrupt" fn invalid_opcode_handler(
    stack_frame: &InterruptStackFrame,
) {
    kprintln!("[EXCEPTION] Invalid Opcode at {:#x}", stack_frame.instruction_pointer);
    loop { unsafe { asm!("hlt") }; }
}

/// Page Fault handler (#PF, vector 14) — the most critical exception.
pub extern "x86-interrupt" fn page_fault_handler(
    stack_frame: &InterruptStackFrame,
    error_code: u64,
) {
    let cr2: u64;
    unsafe { asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack)) };

    // Try to handle as on-demand paging
    if let Some(vmm) = crate::memory::vmm::VMM.lock().as_mut() {
        if vmm.handle_fault(cr2, error_code).is_ok() {
            return; // Fault resolved, resume execution
        }
    }

    kprintln!("[EXCEPTION] Page Fault!");
    kprintln!("  Faulting address: {:#x}", cr2);
    kprintln!("  Error code: {:#x}", error_code);
    kprintln!("  RIP: {:#x}", stack_frame.instruction_pointer);

    // TODO: Kill process instead of hlt
    loop { unsafe { asm!("hlt") }; }
}

/// General Protection Fault (#GP, vector 13)
pub extern "x86-interrupt" fn general_protection_handler(
    stack_frame: &InterruptStackFrame,
    error_code: u64,
) {
    kprintln!("[EXCEPTION] General Protection Fault!");
    kprintln!("  Error code: {:#x}", error_code);
    kprintln!("  RIP: {:#x}", stack_frame.instruction_pointer);
    loop { unsafe { asm!("hlt") }; }
}

/// Double Fault handler (#DF, vector 8) — unrecoverable.
pub extern "x86-interrupt" fn double_fault_handler(
    stack_frame: &InterruptStackFrame,
    _error_code: u64,
) -> ! {
    kprintln!("[FATAL] Double Fault!");
    kprintln!("  RIP: {:#x}", stack_frame.instruction_pointer);
    loop { unsafe { asm!("hlt") }; }
}

// ---------------------------------------------------------------------------
// Hardware IRQ Handlers (vectors 32+)
// ---------------------------------------------------------------------------

/// Register PIC IRQ handlers in the IDT.
fn register_irq_handlers(idt: &mut [IdtEntry; IDT_ENTRIES]) {
    // Vector 32: PIT Timer (IRQ0)
    idt[32].set_handler(
        timer_handler as *const () as u64,
        0x08, 0, 0,
    );

    // Vector 33: PS/2 Keyboard (IRQ1)
    idt[33].set_handler(
        keyboard_handler as *const () as u64,
        0x08, 0, 0,
    );
}

/// Timer interrupt handler (IRQ0 → vector 32).
pub extern "x86-interrupt" fn timer_handler(
    _stack_frame: &InterruptStackFrame,
) {
    crate::drivers::pit::tick();
    crate::drivers::pic::send_eoi(0);

    // Scheduler integration: notify tick and check for preemption
    crate::scheduler::tick();
    crate::scheduler::check_reschedule();
}

/// Keyboard interrupt handler (IRQ1 → vector 33).
pub extern "x86-interrupt" fn keyboard_handler(
    _stack_frame: &InterruptStackFrame,
) {
    if let Some(ch) = crate::drivers::keyboard::handle_scancode() {
        kprintln!("[KBD] '{}'", ch as char);
    }
    crate::drivers::pic::send_eoi(1);
}

