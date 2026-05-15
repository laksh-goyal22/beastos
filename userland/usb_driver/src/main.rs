#![no_std]
#![no_main]

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use core::cell::UnsafeCell;

// Userspace copy of SPSC ring for layout matching
pub struct BeastSPSCRing<T: Copy, const N: usize> {
    buffer: UnsafeCell<[T; N]>,
    head: AtomicUsize,
    tail: AtomicUsize,
    _consumer_task: AtomicUsize,
    waiting: AtomicBool,
}

impl<T: Copy, const N: usize> BeastSPSCRing<T, N> {
    pub fn try_dequeue(&self) -> Option<T> {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);

        if head == tail {
            self.waiting.store(true, Ordering::Release);
            return None;
        }

        let idx = head & (N - 1);
        let value = unsafe { (*self.buffer.get())[idx] };

        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(value)
    }

    pub fn wait_dequeue(&self) -> T {
        loop {
            if let Some(value) = self.try_dequeue() {
                return value;
            }
            beast_syscall::yield_now();
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct InterruptEvent {
    irq: u8,
    timestamp: u64,
    counter: u32,
}

#[no_mangle]
#[no_mangle]
pub extern "C" fn beast_main(_argc: u64, _argv: u64) -> i64 {
    let ring_ptr = beast_syscall::create_irq_ring(256);
    if ring_ptr <= 0 {
        beast_syscall::write(1, b"[USB] Failed to create IRQ ring\n");
        return 1;
    }
    let ring = unsafe { &*(ring_ptr as *const BeastSPSCRing<InterruptEvent, 256>) };

    let mmio_virt = beast_syscall::map_mmio(0xFED00000, 4096);
    if mmio_virt <= 0 {
        beast_syscall::write(1, b"[USB] Failed to map MMIO\n");
        return 1;
    }

    let result = beast_syscall::bind_irq(16, 0);
    if result != 0 {
        beast_syscall::write(1, b"[USB] Failed to bind IRQ 16\n");
        return 1;
    }

    loop {
        let _event = ring.wait_dequeue();
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    const MSG: &[u8] = b"\n[USB] Panic\n";
    unsafe {
        let _ = core::arch::asm!(
            "mov rax, 1",
            "mov rdi, 1",
            "mov rsi, {0}",
            "mov rdx, {1}",
            "syscall",
            in(reg) MSG.as_ptr(),
            in(reg) MSG.len(),
            out("rax") _, out("rcx") _, out("r11") _,
        );
    }
    loop {}
}
