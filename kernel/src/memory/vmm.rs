//! Virtual Memory Manager — Page Table Walker
//!
//! 4-level page tables (PML4 → PDPT → PD → PT).
//! Provides map/unmap/translate for kernel and user virtual addresses.

use core::sync::atomic::{AtomicU64, Ordering};
use crate::arch::paging::{self, PageTable, PageTableEntry, flags, PAGE_SIZE};
use crate::memory::pmm;
use crate::kprintln;

/// Higher-half kernel base (Limine maps kernel here).
pub const KERNEL_OFFSET: u64 = 0xFFFF_8000_0000_0000;

/// Physical-to-virtual offset used by Limine for HHDM (Higher Half Direct Map).
/// Limine maps all physical memory at this offset.
static HHDM_OFFSET: AtomicU64 = AtomicU64::new(0);

/// Set the HHDM offset (called during init from Limine response).
pub fn set_hhdm_offset(offset: u64) {
    HHDM_OFFSET.store(offset, Ordering::SeqCst);
}

/// Convert physical address to virtual (via HHDM).
#[inline(always)]
pub fn phys_to_virt(phys: u64) -> u64 {
    phys + HHDM_OFFSET.load(Ordering::SeqCst)
}

/// Convert virtual address to physical (subtract HHDM).
#[inline(always)]
pub fn virt_to_phys(virt: u64) -> u64 {
    virt - HHDM_OFFSET.load(Ordering::SeqCst)
}

use spin::Mutex;

/// Global instance of the active VirtualMemoryManager
pub static VMM: Mutex<Option<VirtualMemoryManager>> = Mutex::new(None);

/// The Virtual Memory Manager manages a 4-level page table hierarchy.
pub struct VirtualMemoryManager {
    /// Physical address of the PML4 table
    pub pml4_phys: u64,
}

impl VirtualMemoryManager {
    /// PML4 index used for recursive mapping
    pub const RECURSIVE_ENTRY: usize = 510;

    /// Create a new VirtualMemoryManager using the given PML4 physical address.
    pub fn new(pml4_phys: u64) -> Self {
        let mut vmm = Self { pml4_phys };
        vmm.setup_recursive_mapping();
        vmm
    }

    /// Create a VirtualMemoryManager from the currently active CR3.
    pub fn active() -> Self {
        let cr3 = paging::read_cr3() & !0xFFF;
        Self::new(cr3)
    }

    /// Sets up recursive mapping at PML4[RECURSIVE_ENTRY].
    /// This allows the kernel to access page tables without relying entirely on HHDM.
    fn setup_recursive_mapping(&mut self) {
        unsafe {
            let pml4 = &mut *(phys_to_virt(self.pml4_phys) as *mut PageTable);
            let flags = flags::PRESENT | flags::WRITABLE | flags::NO_EXECUTE;
            pml4.entry_mut(Self::RECURSIVE_ENTRY).set_address(self.pml4_phys, flags);
            paging::invlpg(phys_to_virt(self.pml4_phys));
        }
    }

    /// Map a virtual address to a physical address with specific flags.
    pub fn map_page_with_flags(&mut self, virt: u64, phys: u64, flags: u64) -> Result<(), &'static str> {
        unsafe {
            let pml4 = &mut *(phys_to_virt(self.pml4_phys) as *mut PageTable);

            // Walk PML4 → PDPT (level 1)
            let pdpt = get_or_create_table(pml4, paging::pml4_index(virt), flags, 1)?;

            // Walk PDPT → PD (level 2)
            let pd = get_or_create_table(pdpt, paging::pdpt_index(virt), flags, 2)?;

            // Walk PD → PT (level 3)
            let pt = get_or_create_table(pd, paging::pd_index(virt), flags, 3)?;

            // Set PT entry
            let pt_idx = paging::pt_index(virt);
            pt.entry_mut(pt_idx).set_address(phys, flags);
            
            // Flush TLB for this page to ensure consistency
            paging::invlpg(virt);
        }

        Ok(())
    }

    /// Map a virtual address to a physical address.
    pub fn map_page(&mut self, virt: u64, phys: u64, user: bool) -> Result<(), &'static str> {
        let flags = flags::PRESENT | flags::WRITABLE
            | if user { flags::USER } else { 0 };

        unsafe {
            let pml4 = &mut *(phys_to_virt(self.pml4_phys) as *mut PageTable);

            // Walk PML4 → PDPT (level 1)
            let pdpt = get_or_create_table(pml4, paging::pml4_index(virt), flags, 1)?;

            // Walk PDPT → PD (level 2)
            let pd = get_or_create_table(pdpt, paging::pdpt_index(virt), flags, 2)?;

            // Walk PD → PT (level 3)
            let pt = get_or_create_table(pd, paging::pd_index(virt), flags, 3)?;

            // Set PT entry
            let pt_idx = paging::pt_index(virt);
            pt.entry_mut(pt_idx).set_address(phys, flags);
            
            paging::invlpg(virt);
        }

        Ok(())
    }

    /// Allocate and map a page for userspace. Useful for on-demand paging.
    /// If `on_demand` is true, the mapping is created without a physical page (PRESENT bit clear).
    pub fn map_user_page(&mut self, virt: u64, on_demand: bool) -> Result<(), &'static str> {
        if on_demand {
            let flags = flags::USER | flags::WRITABLE | (1 << 9);
            unsafe {
                let pml4 = &mut *(phys_to_virt(self.pml4_phys) as *mut PageTable);
                let pdpt = get_or_create_table(pml4, paging::pml4_index(virt), flags, 1)?;
                let pd = get_or_create_table(pdpt, paging::pdpt_index(virt), flags, 2)?;
                let pt = get_or_create_table(pd, paging::pd_index(virt), flags, 3)?;
                let pt_idx = paging::pt_index(virt);
                pt.entry_mut(pt_idx).set_address(0, flags);
                paging::invlpg(virt);
            }
            Ok(())
        } else {
            let phys = pmm::alloc_page().ok_or("OOM: cannot allocate physical page for user mapping")?;
            self.map_page(virt, phys, true)
        }
    }

    /// Handle a page fault by checking for on-demand paging.
    /// Returns Ok(()) if the fault was handled, or Err if it's a real segmentation fault.
    pub fn handle_fault(&mut self, virt: u64, _error_code: u64) -> Result<(), &'static str> {
        // We only care about addresses in user-space range or those we marked as on-demand.
        // For now, let's look up the entry.
        unsafe {
            let pml4 = &mut *(phys_to_virt(self.pml4_phys) as *mut PageTable);

            let pml4_e = pml4.entry(paging::pml4_index(virt));
            if !pml4_e.is_present() { 
                kprintln!("[PF] PML4 entry not present for {:#x}", virt);
                return Err("PML4 entry not present"); 
            }

            let pdpt = &mut *(phys_to_virt(pml4_e.physical_address()) as *mut PageTable);
            let pdpt_e = pdpt.entry(paging::pdpt_index(virt));
            if !pdpt_e.is_present() { 
                kprintln!("[PF] PDPT entry not present for {:#x}", virt);
                return Err("PDPT entry not present"); 
            }

            let pd = &mut *(phys_to_virt(pdpt_e.physical_address()) as *mut PageTable);
            let pd_e = pd.entry(paging::pd_index(virt));
            if !pd_e.is_present() { 
                kprintln!("[PF] PD entry not present for {:#x}", virt);
                return Err("PD entry not present"); 
            }

            let pt = &mut *(phys_to_virt(pd_e.physical_address()) as *mut PageTable);
            let pt_idx = paging::pt_index(virt);
            let pt_e = pt.entry_mut(pt_idx);

            kprintln!("[PF] Fault at {:#x}, Error: {:#x}, PTE flags: {:#x}", virt, _error_code, pt_e.flags());

            // Check if bit 9 (ON_DEMAND) is set and it's NOT present
            if !pt_e.is_present() && (pt_e.flags() & (1 << 9)) != 0 {
                // It's an on-demand page! Allocate a physical page and map it.
                let phys = pmm::alloc_page().ok_or("OOM: cannot allocate physical page for fault")?;
                // Zero the page
                core::ptr::write_bytes(phys_to_virt(phys) as *mut u8, 0, PAGE_SIZE);

                // Update entry: set present, set physical address, KEEP existing flags (except bit 9)
                // Also ensure NO_EXECUTE is NOT set.
                let mut flags = (pt_e.flags() & !(1 << 9)) | flags::PRESENT;
                flags &= !flags::NO_EXECUTE;

                pt_e.set_address(phys, flags);

                // Invalidate TLB
                paging::invlpg(virt);

                Ok(())
            } else {
                Err("Not an on-demand page")
            }
        }
    }

    /// Unmap a virtual address. Returns the physical address that was mapped.
    pub fn unmap_page(&mut self, virt: u64) -> Result<u64, &'static str> {
        unsafe {
            let pml4 = &mut *(phys_to_virt(self.pml4_phys) as *mut PageTable);

            let pml4_e = pml4.entry(paging::pml4_index(virt));
            if !pml4_e.is_present() { return Err("PML4 not present"); }

            let pdpt = &mut *(phys_to_virt(pml4_e.physical_address()) as *mut PageTable);
            let pdpt_e = pdpt.entry(paging::pdpt_index(virt));
            if !pdpt_e.is_present() { return Err("PDPT not present"); }

            let pd = &mut *(phys_to_virt(pdpt_e.physical_address()) as *mut PageTable);
            let pd_e = pd.entry(paging::pd_index(virt));
            if !pd_e.is_present() { return Err("PD not present"); }

            let pt = &mut *(phys_to_virt(pd_e.physical_address()) as *mut PageTable);
            let pt_idx = paging::pt_index(virt);
            let pt_e = pt.entry(pt_idx);
            
            // Check if it's an on-demand page (not present, but has our custom bit 9)
            if !pt_e.is_present() && (pt_e.flags() & (1 << 9)) == 0 {
                return Err("PT not present");
            }

            let phys = pt_e.physical_address();
            pt.entry_mut(pt_idx).set_address(0, 0); // Clear entry

            // Invalidate TLB for this address
            paging::invlpg(virt);

            Ok(phys)
        }
    }

    /// Translate a virtual address to its physical address.
    pub fn translate(&self, virt: u64) -> Option<u64> {
        unsafe {
            let pml4 = &*(phys_to_virt(self.pml4_phys) as *const PageTable);

            let pml4_e = pml4.entry(paging::pml4_index(virt));
            if !pml4_e.is_present() { return None; }

            let pdpt = &*(phys_to_virt(pml4_e.physical_address()) as *const PageTable);
            let pdpt_e = pdpt.entry(paging::pdpt_index(virt));
            if !pdpt_e.is_present() { return None; }

            // Check for 1GiB huge page
            if pdpt_e.flags() & flags::HUGE_PAGE != 0 {
                let offset = virt & 0x3FFF_FFFF; // 30 bits
                return Some(pdpt_e.physical_address() + offset);
            }

            let pd = &*(phys_to_virt(pdpt_e.physical_address()) as *const PageTable);
            let pd_e = pd.entry(paging::pd_index(virt));
            if !pd_e.is_present() { return None; }

            // Check for 2MiB huge page
            if pd_e.flags() & flags::HUGE_PAGE != 0 {
                let offset = virt & 0x1F_FFFF; // 21 bits
                return Some(pd_e.physical_address() + offset);
            }

            let pt = &*(phys_to_virt(pd_e.physical_address()) as *const PageTable);
            let pt_e = pt.entry(paging::pt_index(virt));
            
            // If it's an on-demand page (not present), translation technically fails
            // until a page fault resolves it.
            if !pt_e.is_present() { return None; }

            let offset = virt & 0xFFF; // 12 bits
            Some(pt_e.physical_address() + offset)
        }
    }

    /// Switch to this virtual memory manager's page table.
    pub fn switch(&self) {
        unsafe {
            paging::load_cr3(self.pml4_phys, 0); // PCID 0 for now
        }
    }
}

/// Split a 1GiB huge page (PDPT level) into a PD table with 512 x 2MiB entries.
unsafe fn split_1gib_page(
    entry: &mut PageTableEntry,
    new_flags: u64,
) -> Result<&'static mut PageTable, &'static str> {
    let base_phys = entry.physical_address();
    let huge_flags = entry.flags();
    // Keep existing flags but ensure USER|WRITABLE are included from new_flags
    let child_flags = (huge_flags | (new_flags & (flags::USER | flags::WRITABLE))) & !flags::HUGE_PAGE;
    let child_huge_flags = child_flags | flags::HUGE_PAGE; // PD entries are still 2MiB huge pages

    let pd_phys = crate::memory::pmm::alloc_page().ok_or("OOM")?;
    let pd_virt = phys_to_virt(pd_phys) as *mut PageTable;
    core::ptr::write_bytes(pd_virt, 0, 4096);

    let pd = &mut *pd_virt;
    for i in 0..512 {
        pd.entry_mut(i).set_address(base_phys + (i as u64) * 0x200000, child_huge_flags);
    }

    let intermediate_flags = flags::PRESENT | flags::WRITABLE | (new_flags & flags::USER);
    entry.set_address(pd_phys, intermediate_flags);
    Ok(pd)
}

/// Split a 2MiB huge page (PD level) into a PT table with 512 x 4KiB entries.
unsafe fn split_2mib_page(
    entry: &mut PageTableEntry,
    new_flags: u64,
) -> Result<&'static mut PageTable, &'static str> {
    let base_phys = entry.physical_address();
    let huge_flags = entry.flags();
    let child_flags = (huge_flags | (new_flags & (flags::USER | flags::WRITABLE))) & !flags::HUGE_PAGE;

    let pt_phys = crate::memory::pmm::alloc_page().ok_or("OOM")?;
    let pt_virt = phys_to_virt(pt_phys) as *mut PageTable;
    core::ptr::write_bytes(pt_virt, 0, 4096);

    let pt = &mut *pt_virt;
    for i in 0..512 {
        pt.entry_mut(i).set_address(base_phys + (i as u64) * 0x1000, child_flags);
    }

    let intermediate_flags = flags::PRESENT | flags::WRITABLE | (new_flags & flags::USER);
    entry.set_address(pt_phys, intermediate_flags);
    Ok(pt)
}

/// Get or create a child page table at the given index.
/// `level` indicates the granularity: 1 = PML4→PDPT, 2 = PDPT→PD, 3 = PD→PT.
unsafe fn get_or_create_table(
    parent: &mut PageTable,
    index: usize,
    new_flags: u64,
    level: u8,
) -> Result<&'static mut PageTable, &'static str> {
    let entry = parent.entry_mut(index);
    if entry.is_present() {
        let present_flags = entry.flags();
        
        // Check for huge pages — must split to allow 4KiB page manipulation
        if present_flags & flags::HUGE_PAGE != 0 {
            return match level {
                2 => split_1gib_page(entry, new_flags),
                3 => split_2mib_page(entry, new_flags),
                _ => return Err("Unexpected huge page at PML4 level"),
            };
        }

        // If the table exists, ensure it has the necessary permissions (e.g. USER bit)
        let current_flags = present_flags;
        let needed_flags = new_flags & (flags::USER | flags::WRITABLE);
        if (current_flags & needed_flags) != needed_flags {
            let updated_flags = current_flags | needed_flags;
            entry.set_address(entry.physical_address(), updated_flags);
        }
        Ok(&mut *(phys_to_virt(entry.physical_address()) as *mut PageTable))
    } else {
        // Allocate a new page table
        let new_phys = crate::memory::pmm::alloc_page().ok_or("OOM")?;
        let new_virt = phys_to_virt(new_phys) as *mut PageTable;

        // CRITICAL: Zero out the new page table!
        unsafe {
            core::ptr::write_bytes(new_virt as *mut u8, 0, 4096);
        }

        // We ensure that intermediate tables are mapped with standard generic permissions
        // like USER and WRITABLE so they don't restrict the PT level permissions.
        let intermediate_flags = flags::PRESENT | flags::WRITABLE | (new_flags & flags::USER);
        parent.entry_mut(index).set_address(new_phys, intermediate_flags);
        Ok(&mut *new_virt)
    }
}

/// Initialize the VMM.
pub fn init() {
    let vmm = VirtualMemoryManager::active();
    kprintln!("    VMM: Active PML4 at {:#x}, Recursive mapping at index {}", vmm.pml4_phys, VirtualMemoryManager::RECURSIVE_ENTRY);
    
    *VMM.lock() = Some(vmm);
}

/// Create a new page table for a user process.
///
/// Allocates a fresh PML4, copies kernel-space entries (indices 256–511)
/// from the kernel CR3, and sets up the recursive mapping at [510] pointing
/// to the new table.  The result: per-process user address spaces (indices
/// 0–255) while sharing a single kernel image — true process isolation.
pub fn create_user_page_table() -> Option<u64> {
    let new_pml4_phys = pmm::alloc_page()?;
    let new_pml4_virt = phys_to_virt(new_pml4_phys) as *mut paging::PageTable;

    unsafe {
        core::ptr::write_bytes(new_pml4_virt as *mut u8, 0, PAGE_SIZE);

        let kernel_cr3 =
            crate::arch::idt::KERNEL_PML4.load(core::sync::atomic::Ordering::Relaxed);
        let kernel_pml4_virt = phys_to_virt(kernel_cr3) as *const paging::PageTable;

        // Copy higher-half entries (kernel mappings: indices 256–511)
        for i in 256..512 {
            let entry = (*kernel_pml4_virt).entry(i);
            if entry.is_present() {
                (*new_pml4_virt).entry_mut(i).set_raw(entry.raw());
            }
        }

        // Override the recursive-mapping entry so it points to *this* PML4
        let flags = flags::PRESENT | flags::WRITABLE | flags::NO_EXECUTE;
        (*new_pml4_virt)
            .entry_mut(VirtualMemoryManager::RECURSIVE_ENTRY)
            .set_address(new_pml4_phys, flags);
    }

    Some(new_pml4_phys)
}
