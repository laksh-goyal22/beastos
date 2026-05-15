//! PIT (Programmable Interval Timer) Driver
//!
//! Generates periodic timer interrupts (IRQ0 → vector 32).
//! Used for preemptive scheduling until APIC timer is available.

use core::arch::asm;
use core::sync::atomic::{AtomicU64, AtomicU32, Ordering};
use crate::kprintln;

const PIT_CHANNEL_0: u16 = 0x40;
const PIT_CMD: u16 = 0x43;
const PIT_FREQUENCY: u32 = 1_193_182; // ~1.19 MHz oscillator

/// Monotonic tick counter (incremented by timer ISR).
pub static TICKS: AtomicU64 = AtomicU64::new(0);

/// Current timer frequency in Hz.
static HZ: AtomicU32 = AtomicU32::new(0);

/// Configure PIT to fire at the given frequency (Hz).
pub fn init(hz: u32) {
    let divisor = if hz == 0 { 65535 } else { PIT_FREQUENCY / hz };
    let divisor = divisor.clamp(1, 65535) as u16;

    HZ.store(hz, Ordering::SeqCst);

    unsafe {
        // Channel 0, lo/hi byte, rate generator (mode 2)
        outb(PIT_CMD, 0x34);
        outb(PIT_CHANNEL_0, (divisor & 0xFF) as u8);
        outb(PIT_CHANNEL_0, (divisor >> 8) as u8);
    }

    kprintln!("    PIT configured: {} Hz (divisor {})", hz, divisor);
}

/// Called from the timer IRQ handler (vector 32).
pub fn tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

/// Read current tick count.
pub fn get_ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// Get the timer frequency in Hz.
pub fn get_hz() -> u64 {
    HZ.load(Ordering::SeqCst) as u64
}

/// Busy-wait for approximately `ms` milliseconds.
pub fn sleep_ms(ms: u64) {
    let hz = HZ.load(Ordering::SeqCst) as u64;
    if hz == 0 { return; }
    let target = get_ticks() + (ms * hz / 1000);
    while get_ticks() < target {
        unsafe { asm!("pause", options(nomem, nostack)); }
    }
}

#[inline(always)]
unsafe fn outb(port: u16, value: u8) {
    asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack));
}
