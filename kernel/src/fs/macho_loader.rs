use crate::memory::vmm;
use crate::arch::paging::flags as PageFlags;
use crate::fs::universal_exec::{LoadedImage, LoaderError};
use core::mem;
use crate::kprintln;

/// Mach-O 64-bit magic number (Little Endian)
const MH_MAGIC_64: u32 = 0xfeedfacf;

/// Load Command Types
const LC_SEGMENT_64: u32 = 0x19;
const LC_UNIXTHREAD: u32 = 0x5;
const LC_MAIN: u32 = 0x80000028;

/// VM Protections
const VM_PROT_WRITE: i32 = 0x2;
const VM_PROT_EXECUTE: i32 = 0x4;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct MachHeader64 {
    magic: u32,
    cputype: i32,
    cpusubtype: i32,
    filetype: u32,
    ncmds: u32,
    sizeofcmds: u32,
    flags: u32,
    reserved: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct LoadCommand {
    cmd: u32,
    cmdsize: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct SegmentCommand64 {
    cmd: u32,
    cmdsize: u32,
    segname: [u8; 16],
    vmaddr: u64,
    vmsize: u64,
    fileoff: u64,
    filesize: u64,
    maxprot: i32,
    initprot: i32,
    nsects: u32,
    flags: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct EntryPointCommand {
    cmd: u32,
    cmdsize: u32,
    entryoff: u64,
    stacksize: u64,
}

pub struct MachOLoader;

impl MachOLoader {
    /// Load a Mach-O 64-bit binary from a byte slice into the provided VMM.
    pub fn load(data: &[u8], vmm: &mut vmm::VirtualMemoryManager) -> Result<LoadedImage, LoaderError> {
        if data.len() < mem::size_of::<MachHeader64>() {
            return Err(LoaderError::InvalidFormat);
        }

        let header = unsafe { &*(data.as_ptr() as *const MachHeader64) };
        if header.magic != MH_MAGIC_64 {
            // Check for Big Endian magic (not supported yet but good to detect)
            if header.magic == 0xcffaedfe {
                kprintln!("[Mach-O] Big Endian Mach-O not supported");
            }
            return Err(LoaderError::InvalidFormat);
        }

        let mut offset = mem::size_of::<MachHeader64>();
        let mut entry_point: Option<u64> = None;
        let mut text_vmaddr: u64 = 0;

        for _ in 0..header.ncmds {
            if offset + mem::size_of::<LoadCommand>() > data.len() {
                break;
            }

            let cmd = unsafe { &*(data.as_ptr().add(offset) as *const LoadCommand) };
            
            match cmd.cmd {
                LC_SEGMENT_64 => {
                    let seg = unsafe { &*(data.as_ptr().add(offset) as *const SegmentCommand64) };
                    
                    // Log segment name (carefully handling null bytes)
                    let mut name_len = 0;
                    while name_len < 16 && seg.segname[name_len] != 0 {
                        name_len += 1;
                    }
                    if let Ok(name) = core::str::from_utf8(&seg.segname[..name_len]) {
                        kprintln!("[Mach-O] Loading segment: {}", name);
                        if name == "__TEXT" {
                            text_vmaddr = seg.vmaddr;
                        }
                    }

                    Self::load_segment(seg, data, vmm)?;
                }
                LC_MAIN => {
                    let ep = unsafe { &*(data.as_ptr().add(offset) as *const EntryPointCommand) };
                    // entryoff is usually relative to the __TEXT segment base
                    entry_point = Some(text_vmaddr + ep.entryoff);
                    kprintln!("[Mach-O] Found LC_MAIN entry point: {:#x}", text_vmaddr + ep.entryoff);
                }
                LC_UNIXTHREAD => {
                    // For x86_64, thread state follows the flavor and count
                    let thread_state_offset = offset + 16;
                    if thread_state_offset + 144 <= data.len() {
                        // rip is at index 16 in x86_THREAD_STATE64
                        let rip_offset = thread_state_offset + (16 * 8); 
                        let rip = unsafe { core::ptr::read_unaligned(data.as_ptr().add(rip_offset) as *const u64) };
                        entry_point = Some(rip);
                        kprintln!("[Mach-O] Found LC_UNIXTHREAD entry point: {:#x}", rip);
                    }
                }
                _ => {}
            }

            offset += cmd.cmdsize as usize;
        }

        match entry_point {
            Some(entry) => Ok(LoadedImage { entry }),
            None => {
                kprintln!("[Mach-O] Error: No entry point found");
                Err(LoaderError::InvalidFormat)
            }
        }
    }

    /// Helper to map and load a segment into virtual memory.
    fn load_segment(seg: &SegmentCommand64, data: &[u8], vmm: &mut vmm::VirtualMemoryManager) -> Result<(), LoaderError> {
        if seg.vmsize == 0 {
            return Ok(());
        }

        let page_size = 4096;
        let vaddr_start = seg.vmaddr;
        let vaddr_end = vaddr_start + seg.vmsize;
        
        // Align to page boundaries
        let page_start = vaddr_start & !(page_size - 1);
        let page_end = (vaddr_end + page_size - 1) & !(page_size - 1);
        let page_count = (page_end - page_start) / page_size;

        for i in 0..page_count {
            let virt = page_start + (i * page_size);
            let phys = crate::memory::pmm::alloc_page().ok_or(LoaderError::OutOfMemory)?;

            // Translate Mach-O protections to Beast OS page flags
            let mut flags = PageFlags::USER | PageFlags::PRESENT;
            if seg.initprot & VM_PROT_WRITE != 0 {
                flags |= PageFlags::WRITABLE;
            }
            if seg.initprot & VM_PROT_EXECUTE == 0 {
                // By default Beast OS might have NX, so we set it if not executable
                flags |= PageFlags::NO_EXECUTE;
            }

            vmm.map_page_with_flags(virt, phys, flags).map_err(|_| LoaderError::OutOfMemory)?;

            // Get pointer to physical page (via HHDM)
            let dest = vmm::phys_to_virt(phys) as *mut u8;
            
            // Zero initialize the entire page (handles BSS sections)
            unsafe {
                core::ptr::write_bytes(dest, 0, page_size as usize);
            }

            // Calculate overlap with file data
            let seg_mem_end = seg.vmaddr + seg.filesize;
            
            let overlap_start = virt.max(seg.vmaddr);
            let overlap_end = (virt + page_size).min(seg_mem_end);

            if overlap_start < overlap_end {
                let copy_len = (overlap_end - overlap_start) as usize;
                let file_offset = seg.fileoff + (overlap_start - seg.vmaddr);
                let dest_offset = (overlap_start - virt) as usize;

                if file_offset + copy_len as u64 <= data.len() as u64 {
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            data.as_ptr().add(file_offset as usize),
                            dest.add(dest_offset),
                            copy_len
                        );
                    }
                }
            }
        }

        Ok(())
    }
}
