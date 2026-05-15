use crate::syscall::driver_syscalls::forward_interrupt_to_ring;
use core::sync::atomic::{AtomicU32, Ordering};
use crate::arch::idt::AllRegisters;
use core::arch::asm;

static INTERRUPT_COUNTER: AtomicU32 = AtomicU32::new(0);

// Basic outb for PIC ack
unsafe fn outb(port: u16, value: u8) {
    asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
}

// The generic IRQ forwarder for our userspace drivers
#[no_mangle]
pub extern "C" fn irq_handler_forwarder_rust(_regs: &AllRegisters) {
    INTERRUPT_COUNTER.fetch_add(1, Ordering::Relaxed);
    
    let irq = 16;
    if !forward_interrupt_to_ring(irq) {
        // kprintln!("[IRQ] Unhandled IRQ {}", irq);
    }
    
    unsafe { 
        outb(0xA0, 0x20); // Slave PIC
        outb(0x20, 0x20); // Master PIC 
    }
}
