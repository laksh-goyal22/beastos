//! Beast OS Kernel Entry Point
//!
//! `kmain()` is called by the bootloader after long mode is established.
//! It initializes all kernel subsystems in order.

#![no_std]
#![no_main]
// #![feature(alloc_error_handler)]

use core::panic::PanicInfo;
use beast_os_kernel::kprintln;
use limine::request::{FramebufferRequest, HhdmRequest, MemmapRequest};
use limine::{BaseRevision, RequestsStartMarker, RequestsEndMarker};

/// Marks the start of the Limine requests section.
/// The bootloader scanner uses this as the lower bound when scanning for requests.
#[used]
#[link_section = ".requests_start_marker"]
static _REQUESTS_START_MARKER: RequestsStartMarker = RequestsStartMarker::new();

/// Set the base revision to 0 for maximum compatibility.
/// Limine v8.x only supports API revisions 0-2 (limine.h: `#if LIMINE_API_REVISION > 2`).
/// The limine crate's `new()` requests revision 6, which is far too high.
#[used]
#[link_section = ".requests"]
static BASE_REVISION: BaseRevision = BaseRevision::with_revision(0);

/// Request the framebuffer from Limine.
#[used]
#[link_section = ".requests"]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

/// Request the HHDM (Higher Half Direct Map) offset from Limine.
#[used]
#[link_section = ".requests"]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

/// Request the memory map from Limine.
#[used]
#[link_section = ".requests"]
static MEMMAP_REQUEST: MemmapRequest = MemmapRequest::new();

/// Marks the end of the Limine requests section.
/// The bootloader scanner uses this as the upper bound when scanning for requests.
#[used]
#[link_section = ".requests_end_marker"]
static _REQUESTS_END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

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

    // Get HHDM offset (needed for physical memory access)
    if let Some(response) = HHDM_REQUEST.response() {
        beast_os_kernel::memory::vmm::set_hhdm_offset(response.offset);
        kprintln!("[DEBUG] HHDM offset: {:#x}", response.offset);
    } else {
        kprintln!("\x1B[1;31m[ERROR] HHDM request failed!\x1B[0m");
    }

    // Diagnostic: print the actual revision and support status before asserting
    if let Some(rev) = BASE_REVISION.actual_revision() {
        kprintln!("[DEBUG] Limine actual revision: {}", rev);
    } else {
        kprintln!("[DEBUG] Limine actual revision: None (bootloader didn't overwrite magic)");
    }
    kprintln!("[DEBUG] BASE_REVISION.is_supported() = {}", BASE_REVISION.is_supported());
    assert!(BASE_REVISION.is_supported(), "Limine base revision handshake failed!");

    // Get the framebuffer from Limine
    if let Some(response) = FRAMEBUFFER_REQUEST.response() {
        if let Some(framebuffer) = response.framebuffers().get(0) {
            kprintln!("[DEBUG] Framebuffer: {}x{} at {:#x}", framebuffer.width, framebuffer.height, framebuffer.address() as u64);
            beast_os_kernel::drivers::framebuffer::init(framebuffer);
        } else {
            kprintln!("[WARN] No framebuffers found!");
        }
    } else {
        kprintln!("[ERROR] Framebuffer request failed!");
    }

    // Phase 2: CPU tables
    kprintln!("[DEBUG] Initializing GDT...");
    beast_os_kernel::arch::gdt::init();

    // Phase 3: Drivers (PIC must be remapped BEFORE idt::init enables interrupts)
    kprintln!("[DEBUG] Initializing Drivers...");
    beast_os_kernel::drivers::init();

    // Now that console is initialized, print the boot banner on-screen
    kprintln!("============================================");
    kprintln!("  Beast OS v0.1.0");
    kprintln!("============================================");
    kprintln!("");

    // Phase 4: IDT (loads interrupt table and enables interrupts via sti)
    kprintln!("[DEBUG] Initializing IDT...");
    beast_os_kernel::arch::idt::init();

    // Phase 5: Memory
    kprintln!("[DEBUG] Initializing Memory Management...");
    let hhdm_offset = HHDM_REQUEST.response().map(|r| r.offset).unwrap_or(0);
    
    if let Some(memmap_response) = MEMMAP_REQUEST.response() {
        beast_os_kernel::memory::init(memmap_response.entries(), hhdm_offset);
    } else {
        kprintln!("\x1B[1;31m[ERROR] Failed to get memory map from Limine!\x1B[0m");
    }

    // Phase 6: Scheduler
    kprintln!("[DEBUG] Initializing Scheduler...");
    beast_os_kernel::scheduler::init();

    // Phase 7: Syscalls
    kprintln!("[DEBUG] Initializing Syscalls...");
    beast_os_kernel::arch::syscall_entry::init();

    // Spawn userspace test task
    beast_os_kernel::scheduler::spawn(userspace_task_launcher, "userspace_launcher", 1);

    // Spawn IPC test tasks: producer/consumer over SPSC channel
    beast_os_kernel::scheduler::spawn(ipc_producer, "producer", 2);
    beast_os_kernel::scheduler::spawn(ipc_consumer, "consumer", 2);

    // Also keep a heartbeat to prove the scheduler still runs normally
    beast_os_kernel::scheduler::spawn(heartbeat_task, "heartbeat", 3);

    kprintln!("[SUCCESS] Beast OS initialized. Starting scheduler.");
    beast_os_kernel::scheduler::run();
}

// ── IPC Test: Producer / Consumer ──────────────────────────────────────

use beast_os_kernel::ipc::channel::Channel;

/// Static channel: 16-slot ring of u64 values.
/// Small capacity to force blocking and test the priority boost path.
static IPC_CHAN: Channel<u64, 16> = Channel::new();

/// Producer task — sends sequential values through the IPC channel.
///
/// Every ~500ms it pushes a value. If the ring fills up (unlikely with
/// 16 slots), the producer blocks and the scheduler boosts the consumer.
extern "C" fn ipc_producer() {
    let slot = beast_os_kernel::scheduler::current_slot();
    IPC_CHAN.set_producer(slot);

    let mut seq: u64 = 0;
    loop {
        seq += 1;
        IPC_CHAN.send(seq);
        beast_os_kernel::kprintln!("[PRODUCER] sent #{}", seq);

        // Throttle: wait ~500ms (50 ticks at 100 Hz) to keep output readable
        let start = beast_os_kernel::drivers::pit::get_ticks();
        while beast_os_kernel::drivers::pit::get_ticks().wrapping_sub(start) < 50 {
            unsafe { core::arch::asm!("pause", options(nomem, nostack)); }
        }
    }
}

/// Consumer task — receives values from the IPC channel.
///
/// Blocks when the ring is empty. When the producer pushes data,
/// the scheduler wakes the consumer with an MLFQ level-0 boost,
/// proving the SPSC-aware scheduling integration.
extern "C" fn ipc_consumer() {
    let slot = beast_os_kernel::scheduler::current_slot();
    IPC_CHAN.set_consumer(slot);

    loop {
        // This call blocks if the ring is empty — the scheduler
        // suspends us and boosts us to level 0 when data arrives.
        let val = IPC_CHAN.recv();
        beast_os_kernel::kprintln!("[CONSUMER] received #{}", val);
    }
}

/// Background heartbeat — proves the scheduler is still alive
/// while IPC tasks block/wake.
extern "C" fn heartbeat_task() {
    let mut last_tick: u64 = beast_os_kernel::drivers::pit::get_ticks();
    let mut count: u64 = 0;
    loop {
        let now = beast_os_kernel::drivers::pit::get_ticks();
        if now.wrapping_sub(last_tick) >= 200 {
            count += 1;
            beast_os_kernel::kprintln!("[HEARTBEAT] #{}", count);
            last_tick = now;
        }
        unsafe { core::arch::asm!("pause", options(nomem, nostack)); }
    }
}

/// Userspace task launcher
///
/// This task prepares a Ring 3 environment, maps memory, and jumps to userspace.
extern "C" fn userspace_task_launcher() {
    kprintln!("[USER] Launcher started");

    // Address for our test "program"
    let user_base = 0x400_000u64;
    let stack_top = 0x500_000u64;

    // Use on-demand paging for the code page
    {
        let mut vmm = beast_os_kernel::memory::vmm::VMM.lock();
        if let Some(ref mut vmm) = vmm.as_mut() {
            // Flag bit 9 = ON_DEMAND
            let flags = 0x07 | (1 << 9); // Present(0)|Write(1)|User(2) | bit 9
            vmm.map_page(user_base, flags, true).expect("Failed to map user page");
            vmm.map_page(stack_top - 4096, flags, true).expect("Failed to map user stack");
        }
    }

    kprintln!("[USER] Memory mapped (on-demand). Writing shellcode...");

    // This write will trigger a page fault, which handle_fault will resolve!
    // Shellcode:
    // 0: 48 c7 c0 64 00 00 00    mov rax, 100 (sys_print_val)
    // 7: 48 c7 c7 37 13 00 00    mov rdi, 0x1337
    // e: 0f 05                   syscall
    // 10: 48 c7 c0 3c 00 00 00   mov rax, 60 (sys_exit)
    // 17: 0f 05                  syscall
    let shellcode: [u8; 25] = [
        0x48, 0xc7, 0xc0, 0x64, 0x00, 0x00, 0x00,
        0x48, 0xc7, 0xc7, 0x37, 0x13, 0x00, 0x00,
        0x0f, 0x05,
        0x48, 0xc7, 0xc0, 0x3c, 0x00, 0x00, 0x00,
        0x0f, 0x05,
    ];

    unsafe {
        core::ptr::copy_nonoverlapping(
            shellcode.as_ptr(),
            user_base as *mut u8,
            shellcode.len()
        );
    }

    kprintln!("[USER] Shellcode written. Entering user mode...");

    unsafe {
        beast_os_kernel::arch::userspace::enter_user_mode(user_base, stack_top);
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
