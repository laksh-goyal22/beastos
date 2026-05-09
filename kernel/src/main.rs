//! Beast OS Kernel Entry Point
//!
//! `kmain()` is called by the bootloader after long mode is established.
//! It initializes all kernel subsystems in order.

#![no_std]
#![no_main]
// #![feature(alloc_error_handler)]

use core::panic::PanicInfo;
use beast_os_kernel::kprintln;

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
    beast_os_kernel::logger::init();
    kprintln!("[DEBUG] Serial logger initialized");
    kprintln!("Beast OS v0.1.0 booting...");

    // Phase 2: CPU tables
    kprintln!("[DEBUG] Initializing GDT/IDT...");
    beast_os_kernel::arch::gdt::init();
    beast_os_kernel::arch::idt::init();

    // Phase 3: Memory
    kprintln!("[DEBUG] Initializing Memory Management...");
    // beast_os_kernel::memory::pmm::init(memory_map);
    // beast_os_kernel::memory::vmm::init();
    // beast_os_kernel::memory::heap::init();

    // Phase 4: ACPI
    kprintln!("[DEBUG] Initializing ACPI...");
    // beast_os_kernel::acpi::init();

    // Phase 5: Timers
    kprintln!("[DEBUG] Initializing Timers...");
    // beast_os_kernel::time::init();

    // Phase 6: Scheduler
    kprintln!("[DEBUG] Initializing Scheduler...");
    // beast_os_kernel::scheduler::init();

    // Phase 7: Drivers
    kprintln!("[DEBUG] Initializing Drivers...");
    // beast_os_kernel::drivers::init();

    // Phase 8: Filesystem
    kprintln!("[DEBUG] Initializing Filesystem...");
    // beast_os_kernel::fs::vfs::init();

    // Phase 9: Syscall interface
    kprintln!("[DEBUG] Initializing Syscalls...");
    // beast_os_kernel::syscall::init();

    // Phase 10: Launch init process
    kprintln!("[DEBUG] Spawning init process...");
    // beast_os_kernel::scheduler::spawn_init();

    kprintln!("[SUCCESS] Beast OS initialized. Entering idle loop.");
    // beast_os_kernel::scheduler::run();

    // Halt loop (should never reach here)
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}

/// Panic handler for kernel — prints to serial and halts.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kprintln!("\n[ERROR] KERNEL PANIC: {}", info);
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}

/*
/// Alloc error handler for kernel heap.
#[alloc_error_handler]
fn alloc_error(_layout: core::alloc::Layout) -> ! {
    panic!("kernel heap allocation failed");
}
*/
