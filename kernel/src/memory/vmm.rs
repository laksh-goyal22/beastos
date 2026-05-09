//! Virtual Memory Manager — Page Table Walker
//!
//! 4-level page tables (PML4 → PDPT → PD → PT).
//! Provides map/unmap/translate for kernel and user virtual addresses.

use core::sync::atomic::{AtomicU64, Ordering};
use crate::arch::paging::{self, PageTable, flags, PAGE_SIZE};
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

    /// Map a virtual address to a physical address.
    pub fn map_page(&mut self, virt: u64, phys: u64, user: bool) -> Result<(), &'static str> {
        let flags = flags::PRESENT | flags::WRITABLE
            | if user { flags::USER } else { 0 };

        unsafe {
            let pml4 = &mut *(phys_to_virt(self.pml4_phys) as *mut PageTable);

            // Walk PML4 → PDPT
            let pdpt = get_or_create_table(pml4, paging::pml4_index(virt), flags)?;

            // Walk PDPT → PD
            let pd = get_or_create_table(pdpt, paging::pdpt_index(virt), flags)?;

            // Walk PD → PT
            let pt = get_or_create_table(pd, paging::pd_index(virt), flags)?;

            // Set PT entry
            let pt_idx = paging::pt_index(virt);
            pt.entry_mut(pt_idx).set_address(phys, flags);
        }

        Ok(())
    }

    /// Allocate and map a page for userspace. Useful for on-demand paging.
    /// If `on_demand` is true, the mapping is created without a physical page (PRESENT bit clear).
    pub fn map_user_page(&mut self, virt: u64, on_demand: bool) -> Result<(), &'static str> {
        if on_demand {
            // For on-demand paging, we leave the PRESENT bit clear but set a software bit
            // to indicate it's an allocated user page that needs to be faulted in.
            // Let's use bit 9 (Available for OS) as the "ON_DEMAND" bit.
            let flags = flags::USER | flags::WRITABLE | (1 << 9); // Bit 9 is available

            unsafe {
                let pml4 = &mut *(phys_to_virt(self.pml4_phys) as *mut PageTable);
                let pdpt = get_or_create_table(pml4, paging::pml4_index(virt), flags)?;
                let pd = get_or_create_table(pdpt, paging::pdpt_index(virt), flags)?;
                let pt = get_or_create_table(pd, paging::pd_index(virt), flags)?;

                let pt_idx = paging::pt_index(virt);
                pt.entry_mut(pt_idx).set_address(0, flags); // No physical address yet
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
            if !pml4_e.is_present() { return Err("PML4 entry not present"); }

            let pdpt = &mut *(phys_to_virt(pml4_e.physical_address()) as *mut PageTable);
            let pdpt_e = pdpt.entry(paging::pdpt_index(virt));
            if !pdpt_e.is_present() { return Err("PDPT entry not present"); }

            let pd = &mut *(phys_to_virt(pdpt_e.physical_address()) as *mut PageTable);
            let pd_e = pd.entry(paging::pd_index(virt));
            if !pd_e.is_present() { return Err("PD entry not present"); }

            let pt = &mut *(phys_to_virt(pd_e.physical_address()) as *mut PageTable);
            let pt_idx = paging::pt_index(virt);
            let pt_e = pt.entry_mut(pt_idx);

            // Check if bit 9 (ON_DEMAND) is set and it's NOT present
            if !pt_e.is_present() && (pt_e.flags() & (1 << 9)) != 0 {
                // It's an on-demand page! Allocate a physical page and map it.
                let phys = pmm::alloc_page().ok_or("OOM: cannot allocate physical page for fault")?;
                // Zero the page
                core::ptr::write_bytes(phys_to_virt(phys) as *mut u8, 0, PAGE_SIZE);
                
                // Update entry: set present, set physical address, keep user/writable flags
                let flags = pt_e.flags() | flags::PRESENT;
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

/// Get or create a child page table at the given index.
unsafe fn get_or_create_table(
    parent: &mut PageTable,
    index: usize,
    _new_flags: u64,
) -> Result<&'static mut PageTable, &'static str> {
    let entry = parent.entry(index);
    if entry.is_present() {
        Ok(&mut *(phys_to_virt(entry.physical_address()) as *mut PageTable))
    } else {
        // Allocate a new page table
        let new_phys = pmm::alloc_page().ok_or("OOM: cannot allocate page table")?;
        let new_virt = phys_to_virt(new_phys) as *mut u8;
        // Zero the new table
        core::ptr::write_bytes(new_virt, 0, PAGE_SIZE);
        
        // We ensure that intermediate tables are mapped with standard generic permissions
        // like USER and WRITABLE so they don't restrict the PT level permissions.
        let intermediate_flags = flags::PRESENT | flags::WRITABLE | flags::USER;
        parent.entry_mut(index).set_address(new_phys, intermediate_flags);
        Ok(&mut *(new_virt as *mut PageTable))
    }
}

/// Initialize the VMM.
pub fn init() {
    let vmm = VirtualMemoryManager::active();
    kprintln!("    VMM: Active PML4 at {:#x}, Recursive mapping at index {}", vmm.pml4_phys, VirtualMemoryManager::RECURSIVE_ENTRY);
    
    *VMM.lock() = Some(vmm);
}
