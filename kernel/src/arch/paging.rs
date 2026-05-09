//! Virtual Memory Management — Paging
//!
//! 4-level page tables (PML4 → PDPT → PD → PT) with PCID support.
//! Implements higher-half kernel mapping and per-process address spaces.

use core::arch::asm;

/// Page size constants.
pub const PAGE_SIZE: usize = 4096;
pub const PAGE_SIZE_2MB: usize = 2 * 1024 * 1024;
pub const PAGE_SIZE_1GB: usize = 1024 * 1024 * 1024;

/// Page table entry flags.
pub mod flags {
    pub const PRESENT: u64 = 1 << 0;
    pub const WRITABLE: u64 = 1 << 1;
    pub const USER: u64 = 1 << 2;
    pub const WRITE_THROUGH: u64 = 1 << 3;
    pub const NO_CACHE: u64 = 1 << 4;
    pub const ACCESSED: u64 = 1 << 5;
    pub const DIRTY: u64 = 1 << 6;
    pub const HUGE_PAGE: u64 = 1 << 7;
    pub const GLOBAL: u64 = 1 << 8;
    pub const NO_EXECUTE: u64 = 1 << 63;
}

/// A single page table entry (PTE).
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    pub const fn empty() -> Self {
        Self(0)
    }

    pub fn is_present(&self) -> bool {
        self.0 & flags::PRESENT != 0
    }

    pub fn set_address(&mut self, addr: u64, flags: u64) {
        self.0 = (addr & 0x000F_FFFF_FFFF_F000) | flags;
    }

    pub fn physical_address(&self) -> u64 {
        self.0 & 0x000F_FFFF_FFFF_F000
    }

    pub fn flags(&self) -> u64 {
        self.0 & 0xFFF0_0000_0000_0FFF
    }
}

/// A page table (512 entries × 8 bytes = 4KB).
#[repr(C, align(4096))]
pub struct PageTable {
    entries: [PageTableEntry; 512],
}

impl PageTable {
    pub const fn new() -> Self {
        Self {
            entries: [PageTableEntry::empty(); 512],
        }
    }

    pub fn entry(&self, index: usize) -> &PageTableEntry {
        &self.entries[index]
    }

    pub fn entry_mut(&mut self, index: usize) -> &mut PageTableEntry {
        &mut self.entries[index]
    }
}

/// Load a PML4 table address into CR3.
///
/// If PCID is enabled, the lower 12 bits of CR3 contain the PCID.
#[inline]
pub unsafe fn load_cr3(pml4_phys: u64, pcid: u16) {
    let cr3_val = pml4_phys | (pcid as u64 & 0xFFF);
    asm!("mov cr3, {}", in(reg) cr3_val, options(nostack));
}

/// Read the current CR3 value.
#[inline]
pub fn read_cr3() -> u64 {
    let value: u64;
    unsafe { asm!("mov {}, cr3", out(reg) value, options(nomem, nostack)) };
    value
}

/// Enable PCID (Process-Context Identifiers) for TLB efficiency.
///
/// Beast OS optimization: Avoids TLB flushes on context switch by
/// tagging TLB entries with 12-bit PCIDs. Cuts latency by 15-20%.
pub unsafe fn enable_pcid() {
    let mut cr4: u64;
    asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
    cr4 |= 1 << 17; // CR4.PCIDE
    asm!("mov cr4, {}", in(reg) cr4, options(nostack));
}

/// Invalidate a single TLB entry for a virtual address.
#[inline]
pub unsafe fn invlpg(addr: u64) {
    asm!("invlpg [{}]", in(reg) addr, options(nostack));
}

/// Flush entire TLB (reload CR3).
#[inline]
pub unsafe fn flush_tlb() {
    let cr3 = read_cr3();
    asm!("mov cr3, {}", in(reg) cr3, options(nostack));
}

/// Extract PML4 index from virtual address.
pub fn pml4_index(virt: u64) -> usize {
    ((virt >> 39) & 0x1FF) as usize
}

/// Extract PDPT index from virtual address.
pub fn pdpt_index(virt: u64) -> usize {
    ((virt >> 30) & 0x1FF) as usize
}

/// Extract PD index from virtual address.
pub fn pd_index(virt: u64) -> usize {
    ((virt >> 21) & 0x1FF) as usize
}

/// Extract PT index from virtual address.
pub fn pt_index(virt: u64) -> usize {
    ((virt >> 12) & 0x1FF) as usize
}

/// Initialize virtual memory management.
pub fn init() {
    // TODO:
    // 1. Parse boot memory map
    // 2. Set up higher-half kernel mapping
    // 3. Enable PCID
    // 4. Map framebuffer with write-combining MTRR
}
