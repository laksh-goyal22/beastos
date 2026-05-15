//! PE Direct Syscall Patcher
//!
//! Patches Windows binary imports to point directly to Beast OS syscall stubs.

use crate::memory::{pmm, vmm};
use crate::arch::paging::flags as PageFlags;

pub struct PePatcher;

impl PePatcher {
    /// Create a page of syscall stubs and return its user virtual address.
    pub fn create_stubs(vmm: &mut vmm::VirtualMemoryManager) -> Option<u64> {
        let phys = pmm::alloc_page()?;
        let virt = 0x7FFF_0000_0000; // Fixed address for stubs in user space
        
        let flags = PageFlags::USER | PageFlags::PRESENT; // Note: We might need RX
        if let Err(_) = vmm.map_page_with_flags(virt, phys, flags) {
            return None;
        }
        
        let ptr = vmm::phys_to_virt(phys) as *mut u8;
        unsafe {
            core::ptr::write_bytes(ptr, 0xCC, 4096); // Fill with 'int3' for safety
            
            // Stub 0: WriteFile (rax=1)
            // mov rdi, rcx; mov rsi, rdx; mov rdx, r8; mov rax, 1; syscall; ret
            let write_stub = [
                0x48, 0x89, 0xCF, 
                0x48, 0x89, 0xD6, 
                0x4C, 0x89, 0xC2, 
                0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00,
                0x0F, 0x05, 
                0xC3
            ];
            core::ptr::copy_nonoverlapping(write_stub.as_ptr(), ptr.add(0), write_stub.len());
            
            // Stub 1: ReadFile (rax=0)
            // mov rdi, rcx; mov rsi, rdx; mov rdx, r8; mov rax, 0; syscall; ret
            let read_stub = [
                0x48, 0x89, 0xCF, 
                0x48, 0x89, 0xD6, 
                0x4C, 0x89, 0xC2, 
                0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00,
                0x0F, 0x05, 
                0xC3
            ];
            core::ptr::copy_nonoverlapping(read_stub.as_ptr(), ptr.add(32), read_stub.len());

            // Stub 2: ExitProcess (rax=60)
            // mov rdi, rcx; mov rax, 60; syscall; ret
            let exit_stub = [
                0x48, 0x89, 0xCF,
                0x48, 0xC7, 0xC0, 0x3C, 0x00, 0x00, 0x00,
                0x0F, 0x05,
                0xC3
            ];
            core::ptr::copy_nonoverlapping(exit_stub.as_ptr(), ptr.add(64), exit_stub.len());
        }
        
        Some(virt)
    }

    pub fn get_stub_address(stubs_base: u64, name: &str) -> Option<u64> {
        match name {
            "WriteFile" => Some(stubs_base + 0),
            "ReadFile" => Some(stubs_base + 32),
            "ExitProcess" => Some(stubs_base + 64),
            _ => None,
        }
    }
}
