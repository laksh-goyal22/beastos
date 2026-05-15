#![no_std]
#![no_main]

extern crate alloc;

use core::panic::PanicInfo;
use beast_os_kernel::kprintln;
use beast_os_kernel::scheduler::switch;
use limine::request::{ModulesRequest, MemmapRequest, HhdmRequest};
use limine::{BaseRevision, RequestsStartMarker, RequestsEndMarker};

#[used]
#[link_section = ".requests_base_revision"]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[link_section = ".requests_start_marker"]
static REQUESTS_START_MARKER: RequestsStartMarker = RequestsStartMarker::new();

#[link_section = ".requests"]
static MODULE_REQUEST: ModulesRequest = ModulesRequest::new();

#[link_section = ".requests"]
static MEMMAP_REQUEST: MemmapRequest = MemmapRequest::new();

#[link_section = ".requests"]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[link_section = ".requests_end_marker"]
static REQUESTS_END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

// Test kernel task
extern "C" fn test_kernel_task() {
    unsafe {
        let msg = b"[TASK0] Running test task\r\n\0";
        let mut i = 0;
        while msg[i] != 0 {
            core::arch::asm!("out dx, al", in("dx") 0x3f8u16, in("al") msg[i]);
            i += 1;
        }
    }
    loop { unsafe { core::arch::asm!("hlt"); } }
}

#[unsafe(no_mangle)]
pub extern "C" fn kmain() -> ! {
    kprintln!("Beast OS v0.1.0 booting...");
    
    // Low-level initialization
    beast_os_kernel::arch::gdt::init();
    beast_os_kernel::drivers::init();
    beast_os_kernel::arch::idt::init();

    // Enable timer now that IDT (including vector 32) is ready
    beast_os_kernel::drivers::pic::enable_timer();

    // Initialize memory using Limine information
    let memmap = MEMMAP_REQUEST.response().expect("Failed to get memory map");
    let hhdm = HHDM_REQUEST.response().expect("Failed to get HHDM offset");

    beast_os_kernel::memory::init(
        memmap.entries(),
        hhdm.offset,
        0,
        0x2000000,
    );

    beast_os_kernel::scheduler::init();
    beast_os_kernel::syscall::init();
    
    // Save kernel's original PML4 for syscalls and page fault handling
    let kernel_cr3 = beast_os_kernel::arch::paging::read_cr3() & !0xFFF;
    beast_os_kernel::arch::idt::KERNEL_PML4.store(kernel_cr3, core::sync::atomic::Ordering::Relaxed);
    beast_os_kernel::arch::syscall_entry::set_kernel_cr3(kernel_cr3);

    // Initialize VFS with initrd if present
    if let Some(module_res) = MODULE_REQUEST.response() {
        for module in module_res.modules() {
            let name = module.path();
            if name.contains("initrd.tar") {
                let tar_data = module.data();
                beast_os_kernel::syscall::set_initrd_data(tar_data);
                let archive = beast_os_kernel::fs::tar::TarArchive::new(tar_data);
                beast_os_kernel::fs::vfs::VFS.lock().mount("/", alloc::boxed::Box::new(archive));
                
                let ramfs = alloc::boxed::Box::new(beast_os_kernel::fs::ramfs::RamFs::new());
                beast_os_kernel::fs::vfs::VFS.lock().mount("/home", ramfs);
                break;
            }
        }
    }

    // Spawn test kernel task first
    kprintln!("[BOOT] Spawning test kernel task...");
    beast_os_kernel::scheduler::spawn(test_kernel_task, "test_task", 1);
    
    
    // Load and spawn user task from initrd via VFS
    // Create user page table first so ELF + stack are mapped into it
    if let Some(user_cr3) = beast_os_kernel::memory::vmm::create_user_page_table() {
        let mut vmm = beast_os_kernel::memory::vmm::VirtualMemoryManager::new(user_cr3);
        
        match beast_os_kernel::fs::vfs::VFS.lock().open("/bin/hello.beast") {
            Ok(mut file) => {
                let size = file.size() as usize;
                let mut buffer = alloc::vec::Vec::with_capacity(size);
                buffer.resize(size, 0u8);
                match file.read(&mut buffer) {
                    Ok(_size) => {
                        kprintln!("[BOOT] Loaded hello.beast ({} bytes)", size);
                        if let Ok(image) = beast_os_kernel::fs::universal_exec::load(&buffer, &mut vmm) {
                            kprintln!("[BOOT] ELF loaded, entry={:#x}", image.entry);
                            
                            // Ensure low memory (0x0 - 0x1000) has page table entries
                            // This guards against null pointer derefs and ensures PD[0] exists
                            if let Some(phys) = beast_os_kernel::memory::pmm::alloc_page() {
                                let _ = vmm.map_page_with_flags(
                                    0x0,
                                    phys,
                                    beast_os_kernel::arch::paging::flags::PRESENT | 
                                    beast_os_kernel::arch::paging::flags::USER
                                );
                                kprintln!("[BOOT] Mapped guard page at 0x0 -> {:#x}", phys);
                            }
                            
                            
                            // Create a user-space trampoline - copy the kernel trampoline code to a new page
                            // This avoids issues with trying to map kernel code into user space
                            // IMPORTANT: Don't use 0x400000 - that's where ELF code is!
                            let trampoline_page = beast_os_kernel::memory::pmm::alloc_page();
                            if let Some(trampoline_phys) = trampoline_page {
                                let trampoline_virt = beast_os_kernel::memory::vmm::phys_to_virt(trampoline_phys);
                                // Copy the trampoline code (64 bytes — covers all instructions + iretq)
                                let src = switch::user_entry_trampoline as *const u8;
                                unsafe {
                                    core::ptr::copy_nonoverlapping(src, trampoline_virt as *mut u8, 64);
                                }
                                // Map at user address 0x500000 (NOT 0x400000 where ELF code is!)
                                let _ = vmm.map_page_with_flags(
                                    0x500000,
                                    trampoline_phys,
                                    beast_os_kernel::arch::paging::flags::PRESENT | 
                                    beast_os_kernel::arch::paging::flags::USER
                                );
                                // Also map extra pages for trampoline to prevent faults
                                for page_addr in [0x501000, 0x502000, 0x503000, 0x504000] {
                                    if let Some(p) = beast_os_kernel::memory::pmm::alloc_page() {
                                        let _ = vmm.map_page_with_flags(
                                            page_addr,
                                            p,
                                            beast_os_kernel::arch::paging::flags::PRESENT | 
                                            beast_os_kernel::arch::paging::flags::USER
                                        );
                                    }
                                }
                                kprintln!("[BOOT] Copied trampoline to user page at 0x500000 -> {:#x}", trampoline_phys);
                            }
                            
                            // Update task context to use user-space trampoline address
                            // We'll need to modify this in the task creation
                            
                            let stack_top = 0x0000_7FFF_FFFF_F000u64;
                            
                            for i in 0..16 {
                                let stack_page_virt = stack_top - (i + 1) * 4096;
                                if let Some(phys) = beast_os_kernel::memory::pmm::alloc_page() {
                                    let _ = vmm.map_page_with_flags(
                                        stack_page_virt, 
                                        phys, 
                                        beast_os_kernel::arch::paging::flags::PRESENT | 
                                        beast_os_kernel::arch::paging::flags::WRITABLE |
                                        beast_os_kernel::arch::paging::flags::USER
                                    );
                                    unsafe { 
                                        core::ptr::write_bytes(
                                            beast_os_kernel::memory::vmm::phys_to_virt(phys) as *mut u8, 
                                            0, 
                                            4096
                                        ); 
                                    }
                                }
                            }
                            
                            let hello_entry = image.entry;
                            // Verify the entry page is mapped in the user page table
                            match vmm.translate(hello_entry) {
                                Some(phys) => kprintln!("[BOOT] Entry {:#x} maps to phys {:#x} in user CR3 {:#x}", hello_entry, phys, user_cr3),
                                None => kprintln!("[BOOT] WARNING: Entry {:#x} NOT MAPPED in user page table!", hello_entry),
                            }
                            // Use user-space trampoline at 0x500000 instead of 0x400000 (ELF code location)
                            const USER_TRAMPOLINE: u64 = 0x500000;
                            beast_os_kernel::scheduler::spawn_user_with_page_table_and_trampoline(
                                hello_entry, stack_top, "hello", 1, user_cr3, USER_TRAMPOLINE,
                            );
                        } else {
                            kprintln!("[BOOT] Failed to load ELF");
                        }
                    },
                    Err(e) => kprintln!("[BOOT] Failed to read file: {:?}", e),
                }
            },
            Err(e) => kprintln!("[BOOT] Failed to open /bin/hello.beast: {:?}", e),
        }
    }
    
    kprintln!("[SUCCESS] Beast OS initialized. Starting scheduler.");
    beast_os_kernel::scheduler::run();
    
    // Should never reach here
    loop { unsafe { core::arch::asm!("hlt") }; }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kprintln!("\n[ERROR] KERNEL PANIC: {}", info);
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}