//! Memory Management Subsystem
//!
//! - `pmm`: Physical Memory Manager (bitmap allocator)
//! - `vmm`: Virtual Memory Manager (page table walker)
//! - `heap`: Kernel heap allocator (linked-list free list)

pub mod pmm;
pub mod vmm;
pub mod heap;

use crate::kprintln;

/// Initialize the entire memory subsystem.
///
/// `regions`: (base_addr, length, is_usable) from bootloader memory map.
/// `hhdm_offset`: Higher Half Direct Map offset from Limine.
pub fn init(entries: &[&limine::memmap::Entry], hhdm_offset: u64) {
    kprintln!("  [MEM] Setting HHDM offset: {:#x}", hhdm_offset);
    vmm::set_hhdm_offset(hhdm_offset);

    kprintln!("  [MEM] Initializing PMM...");
    pmm::init(entries);

    kprintln!("  [MEM] Initializing VMM...");
    vmm::init();

    kprintln!("  [MEM] Initializing Heap...");
    heap::init();

    kprintln!("  [MEM] Memory subsystem ready");
}
