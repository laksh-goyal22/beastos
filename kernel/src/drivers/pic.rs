//! 8259 PIC (Programmable Interrupt Controller) Driver
//!
//! Remaps IRQs from vectors 0-15 (conflicting with CPU exceptions)
//! to vectors 32-47.

use core::arch::asm;
use crate::kprintln;

const PIC1_CMD: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_CMD: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

const ICW1_INIT: u8 = 0x11;
const ICW4_8086: u8 = 0x01;

/// PIC1 IRQ base vector (timer=32, keyboard=33, ...)
pub const PIC1_OFFSET: u8 = 32;
/// PIC2 IRQ base vector (cascade from PIC1 IRQ2)
pub const PIC2_OFFSET: u8 = 40;

/// Remap PIC to non-conflicting interrupt vectors.
pub fn init() {
    unsafe {
        // Save masks
        let _mask1 = inb(PIC1_DATA);
        let _mask2 = inb(PIC2_DATA);

        // ICW1: Start init sequence
        outb(PIC1_CMD, ICW1_INIT);
        io_wait();
        outb(PIC2_CMD, ICW1_INIT);
        io_wait();

        // ICW2: Vector offsets
        outb(PIC1_DATA, PIC1_OFFSET);
        io_wait();
        outb(PIC2_DATA, PIC2_OFFSET);
        io_wait();

        // ICW3: Cascade wiring
        outb(PIC1_DATA, 4); // PIC2 at IRQ2
        io_wait();
        outb(PIC2_DATA, 2); // Cascade identity
        io_wait();

        // ICW4: 8086 mode
        outb(PIC1_DATA, ICW4_8086);
        io_wait();
        outb(PIC2_DATA, ICW4_8086);
        io_wait();

        // Unmask: timer (IRQ0), keyboard (IRQ1), cascade (IRQ2)
        outb(PIC1_DATA, 0b1111_1000); // Enable IRQ 0, 1, 2
        outb(PIC2_DATA, 0b1111_1111); // Mask all PIC2
    }

    kprintln!("    PIC remapped: IRQ0-7 → vec {}-{}, IRQ8-15 → vec {}-{}",
        PIC1_OFFSET, PIC1_OFFSET + 7,
        PIC2_OFFSET, PIC2_OFFSET + 7);
}

/// Send End-of-Interrupt to PIC.
pub fn send_eoi(irq: u8) {
    unsafe {
        if irq >= 8 {
            outb(PIC2_CMD, 0x20);
        }
        outb(PIC1_CMD, 0x20);
    }
}

/// Disable both PICs (for APIC migration).
pub fn disable() {
    unsafe {
        outb(PIC1_DATA, 0xFF);
        outb(PIC2_DATA, 0xFF);
    }
}

#[inline(always)]
unsafe fn outb(port: u16, value: u8) {
    asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack));
}

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    asm!("in al, dx", in("dx") port, out("al") val, options(nomem, nostack));
    val
}

#[inline(always)]
fn io_wait() {
    unsafe { outb(0x80, 0); } // Port 0x80 is used for I/O delay
}
