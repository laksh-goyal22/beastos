//! Beast OS Kernel Entry Point
//!
//! `kmain()` is called by the bootloader after long mode is established.
//! It initializes all kernel subsystems in order.

#![no_std]
#![no_main]

use core::panic::PanicInfo;

/// Kernel main entry point.
///
/// Called from assembly after CPU is in 64-bit long mode with paging enabled.
/// Initializes subsystems in dependency order:
/// 1. Serial/logging (for debug output)
/// 2. GDT & IDT (CPU tables)
/// 3. Memory management (PMM → VMM → heap)
/// 4. ACPI (hardware detection)
/// 5. Timer (PIT/HPET/APIC)
/// 6. Scheduler (task management)
/// 7. Drivers (framebuffer, keyboard)
/// 8. Filesystem (VFS, tmpfs)
/// 9. Syscall interface
/// 10. Init process
#[no_mangle]
pub extern "C" fn kmain() -> ! {
    // Phase 1: Early boot — serial logging
    // beast_os_kernel::logger::init();
    // kprintln!("Beast OS v0.1.0 booting...");

    // Phase 2: CPU tables
    // beast_os_kernel::arch::x86_64::gdt::init();
    // beast_os_kernel::arch::x86_64::idt::init();

    // Phase 3: Memory
    // beast_os_kernel::memory::pmm::init(memory_map);
    // beast_os_kernel::memory::vmm::init();
    // beast_os_kernel::memory::heap::init();

    // Phase 4: ACPI
    // beast_os_kernel::acpi::init();

    // Phase 5: Timers
    // beast_os_kernel::time::init();

    // Phase 6: Scheduler
    // beast_os_kernel::scheduler::init();

    // Phase 7: Drivers
    // beast_os_kernel::drivers::init();

    // Phase 8: Filesystem
    // beast_os_kernel::fs::vfs::init();

    // Phase 9: Syscall interface
    // beast_os_kernel::syscall::init();

    // Phase 10: Launch init process
    // beast_os_kernel::scheduler::spawn_init();

    // kprintln!("Beast OS initialized. Entering scheduler loop.");
    // beast_os_kernel::scheduler::run();

    // Halt loop (should never reach here)
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}

/// Panic handler for kernel — prints to serial and halts.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // TODO: Print panic info to serial console
    // kprintln!("KERNEL PANIC: {}", info);
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}

/// Alloc error handler for kernel heap.
#[alloc_error_handler]
fn alloc_error(_layout: core::alloc::Layout) -> ! {
    panic!("kernel heap allocation failed");
}
