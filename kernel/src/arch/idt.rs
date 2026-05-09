//! Interrupt Descriptor Table (IDT) for x86_64
//!
//! Registers handlers for:
//! - CPU exceptions (page fault, GPF, double fault, etc.)
//! - Hardware IRQs (timer, keyboard, serial, etc.)
//! - Software interrupts (system calls)

use core::arch::asm;

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
    pub fn set_handler(&mut self, handler: u64, selector: u16, ist_index: u8, ring: u8) {
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

/// The IDT (256 entries).
static mut IDT: [IdtEntry; IDT_ENTRIES] = [IdtEntry::empty(); IDT_ENTRIES];

/// IDT Pointer for `lidt` instruction.
#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

/// Initialize the IDT with exception and IRQ handlers.
pub fn init() {
    unsafe {
        // Exception handlers (vectors 0-31)
        register_exception_handlers();

        // IRQ handlers (vectors 32-47 via PIC remapping)
        register_irq_handlers();

        // Load the IDT
        let idt_ptr = IdtPointer {
            limit: (core::mem::size_of::<[IdtEntry; IDT_ENTRIES]>() - 1) as u16,
            base: IDT.as_ptr() as u64,
        };
        asm!("lidt [{}]", in(reg) &idt_ptr, options(nostack));
    }
}

/// Register CPU exception handlers (vectors 0-31).
unsafe fn register_exception_handlers() {
    // Vector 0: Division Error
    // Vector 6: Invalid Opcode
    // Vector 8: Double Fault (uses IST1)
    // Vector 13: General Protection Fault
    // Vector 14: Page Fault
    // TODO: Wire up actual handler functions
}

/// Register hardware IRQ handlers (vectors 32+).
unsafe fn register_irq_handlers() {
    // Vector 32: Timer (PIT / APIC)
    // Vector 33: Keyboard
    // Vector 36: Serial (COM1)
    // TODO: Wire up actual handler functions
}

// ---------------------------------------------------------------------------
// Exception Handler Stubs
// ---------------------------------------------------------------------------

/// Page Fault handler — the most critical exception for demand paging.
pub extern "x86-interrupt" fn page_fault_handler(
    _stack_frame: &InterruptStackFrame,
    _error_code: u64,
) {
    // Read CR2 for faulting address
    let cr2: u64;
    unsafe { asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack)) };

    // TODO: Handle demand paging, CoW, or kill process
    // For now, halt
    loop {
        unsafe { asm!("hlt") };
    }
}

/// Double Fault handler — unrecoverable, halt immediately.
pub extern "x86-interrupt" fn double_fault_handler(
    _stack_frame: &InterruptStackFrame,
    _error_code: u64,
) -> ! {
    // kprintln!("DOUBLE FAULT!");
    loop {
        unsafe { asm!("hlt") };
    }
}

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
