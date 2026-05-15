use crate::memory::vmm;
use crate::arch::paging::flags as PageFlags;
use core::mem;

// ELF64 constants
const ELF_MAGIC: u32 = 0x464C457F; // 0x7F 'E' 'L' 'F'
const ELF_CLASS_64: u8 = 2;
const ELF_DATA_2LSB: u8 = 1;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
const PT_LOAD: u32 = 1;
// const PT_INTERP: u32 = 3;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
// const PF_R: u32 = 4;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct Elf64Ehdr {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct Elf64Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

pub struct LoadedElf {
    pub entry: u64,
    pub phdr_addr: u64,
    pub phdr_num: u16,
}

#[derive(Debug)]
pub enum ElfError {
    BadMagic,
    Not64Bit,
    WrongEndian,
    NotExecutable,
    NoProgramHeaders,
    ReadFailed,
    OutOfMemory,
}

impl LoadedElf {
    /// Load an ELF file from a byte slice into memory
    pub fn load(data: &[u8], vmm: &mut vmm::VirtualMemoryManager) -> Result<Self, ElfError> {
        // Read ELF header
        if data.len() < mem::size_of::<Elf64Ehdr>() {
            return Err(ElfError::ReadFailed);
        }
        let ehdr = unsafe { *(data.as_ptr() as *const Elf64Ehdr) };
        
        // Validate ELF header
        let magic = unsafe { core::ptr::read_unaligned(ehdr.e_ident.as_ptr() as *const u32) };
        if magic != ELF_MAGIC {
            return Err(ElfError::BadMagic);
        }
        if ehdr.e_ident[4] != ELF_CLASS_64 {
            return Err(ElfError::Not64Bit);
        }
        if ehdr.e_ident[5] != ELF_DATA_2LSB {
            return Err(ElfError::WrongEndian);
        }
        if ehdr.e_type != ET_EXEC && ehdr.e_type != ET_DYN {
            return Err(ElfError::NotExecutable);
        }
        
        // Validate program header table
        if ehdr.e_phnum == 0 {
            return Err(ElfError::NoProgramHeaders);
        }
        
        // Load all PT_LOAD segments
        for i in 0..ehdr.e_phnum {
            let phdr_offset = (ehdr.e_phoff + (i as u64 * ehdr.e_phentsize as u64)) as usize;
            if phdr_offset + mem::size_of::<Elf64Phdr>() > data.len() {
                return Err(ElfError::ReadFailed);
            }
            let phdr = unsafe { *(data.as_ptr().add(phdr_offset) as *const Elf64Phdr) };
            
            if phdr.p_type == PT_LOAD {
                Self::load_segment(&phdr, data, vmm)?;
            }
        }
        
        Ok(LoadedElf {
            entry: ehdr.e_entry,
            phdr_addr: ehdr.e_phoff,
            phdr_num: ehdr.e_phnum,
        })
    }
    
    /// Load a single PT_LOAD segment
    /// Maps at p_vaddr (user-space) with proper page permissions (W^X enforcement).
    /// Does NOT map at KERNEL_OFFSET to avoid HHDM collision with kernel stacks.
    fn load_segment(phdr: &Elf64Phdr, data: &[u8], vmm: &mut vmm::VirtualMemoryManager) -> Result<(), ElfError> {
        let seg_vaddr = phdr.p_vaddr;  // User-space virtual address
        let memsz = phdr.p_memsz as usize;
        let filesz = phdr.p_filesz as usize;
        let page_size = 4096;
        
        // Align to page boundaries
        let page_start = seg_vaddr & !(page_size - 1);
        let page_end = (seg_vaddr + memsz as u64 + page_size - 1) & !(page_size - 1);
        let page_count = (page_end - page_start) / page_size;

        crate::kprintln!("[ELF_LOAD] seg vaddr={:#x} filesz={} memsz={} pages={}", seg_vaddr, filesz, memsz, page_count);

        let user_va_base = phdr.p_vaddr & !(page_size - 1);  // Page-aligned base

        for i in 0..page_count {
            let page_virt = user_va_base + (i * page_size);  // User-space page address
            let phys = crate::memory::pmm::alloc_page().ok_or(ElfError::OutOfMemory)?;
            
            let mut flags = PageFlags::PRESENT | PageFlags::USER;
            if phdr.p_flags & PF_W != 0 {
                flags |= PageFlags::WRITABLE;
            }
            if phdr.p_flags & PF_X == 0 {
                flags |= PageFlags::NO_EXECUTE;
            }
            
            vmm.map_page_with_flags(page_virt, phys, flags).map_err(|_| ElfError::OutOfMemory)?;  // User-space mapping only
            crate::kprintln!("[ELF_LOAD]   page {:#x} -> {:#x}", page_virt, phys);

            // Physical memory pointer via HHDM
            let dest_page = vmm::phys_to_virt(phys) as *mut u8;

            // Zero the page initially (handles alignment gaps and BSS)
            unsafe {
                core::ptr::write_bytes(dest_page, 0, 4096);
            }

            // Calculate overlap between this page and the segment data (user-space vaddr)
            let seg_start = seg_vaddr;
            let seg_end = seg_vaddr + filesz as u64;

            let copy_start = page_virt.max(seg_start);
            let copy_end = (page_virt + 4096).min(seg_end);
            
            if copy_start < copy_end {
                let len = (copy_end - copy_start) as usize;
                let offset_in_file = (phdr.p_offset + (copy_start - seg_start)) as usize;
                let offset_in_page = (copy_start - page_virt) as usize;
                
                if offset_in_file + len > data.len() {
                    return Err(ElfError::ReadFailed);
                }
                
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        data.as_ptr().add(offset_in_file),
                        dest_page.add(offset_in_page),
                        len
                    );
                }
            }
        }
        
        Ok(())
    }
}
