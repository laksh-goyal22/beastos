//! PE Loader for Beast OS
//!
//! Implements a basic Portable Executable loader for 64-bit Windows binaries (PE32+).
//! Now supports Import Table Patching for direct syscall stubs.

use crate::memory::vmm;
use crate::arch::paging::flags as PageFlags;
use crate::fs::universal_exec::{LoadedImage, LoaderError};
use crate::fs::pe_patcher::PePatcher;
use crate::memory::pmm;
use core::mem;
use crate::kprintln;

/// DOS Header magic number ('MZ')
const IMAGE_DOS_SIGNATURE: u16 = 0x5A4D;
/// PE Header magic number ('PE\0\0')
const IMAGE_NT_SIGNATURE: u32 = 0x00004550;
/// PE32+ (64-bit) optional header magic
const IMAGE_NT_OPTIONAL_HDR64_MAGIC: u16 = 0x20b;

// Section characteristics
const IMAGE_SCN_MEM_EXECUTE: u32 = 0x20000000;
const IMAGE_SCN_MEM_WRITE: u32 = 0x80000000;

const IMAGE_DIRECTORY_ENTRY_IMPORT: usize = 1;
const IMAGE_ORDINAL_FLAG64: u64 = 0x8000000000000000;

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
struct DosHeader {
    e_magic: u16,      // Magic number
    e_cblp: u16,       // Bytes on last page of file
    e_cp: u16,        // Pages in file
    e_crlc: u16,      // Relocations
    e_cparhdr: u16,   // Size of header in paragraphs
    e_minalloc: u16,  // Minimum extra paragraphs needed
    e_maxalloc: u16,  // Maximum extra paragraphs needed
    e_ss: u16,        // Initial (relative) SS value
    e_sp: u16,        // Initial SP value
    e_csum: u16,      // Checksum
    e_ip: u16,        // Initial IP value
    e_cs: u16,        // Initial (relative) CS value
    e_lfarlc: u16,    // File address of relocation table
    e_ovno: u16,      // Overlay number
    e_res: [u16; 4],   // Reserved words
    e_oemid: u16,     // OEM identifier
    e_oeminfo: u16,   // OEM information
    e_res2: [u16; 10], // Reserved words
    e_lfanew: u32,    // File address of new exe header
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
struct PeFileHeader {
    machine: u16,
    number_of_sections: u16,
    time_date_stamp: u32,
    pointer_to_symbol_table: u32,
    number_of_symbols: u32,
    size_of_optional_header: u16,
    characteristics: u16,
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
struct ImageDataDirectory {
    virtual_address: u32,
    size: u32,
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
struct PeOptionalHeader64 {
    magic: u16,
    major_linker_version: u8,
    minor_linker_version: u8,
    size_of_code: u32,
    size_of_initialized_data: u32,
    size_of_uninitialized_data: u32,
    address_of_entry_point: u32,
    base_of_code: u32,
    image_base: u64,
    section_alignment: u32,
    file_alignment: u32,
    major_operating_system_version: u16,
    minor_operating_system_version: u16,
    major_image_version: u16,
    minor_image_version: u16,
    major_subsystem_version: u16,
    minor_subsystem_version: u16,
    win32_version_value: u32,
    size_of_image: u32,
    size_of_headers: u32,
    check_sum: u32,
    subsystem: u16,
    dll_characteristics: u16,
    size_of_stack_reserve: u64,
    size_of_stack_commit: u64,
    size_of_heap_reserve: u64,
    size_of_heap_commit: u64,
    loader_flags: u32,
    number_of_rva_and_sizes: u32,
    data_directory: [ImageDataDirectory; 16],
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
struct PeSectionHeader {
    name: [u8; 8],
    virtual_size: u32,
    virtual_address: u32,
    size_of_raw_data: u32,
    pointer_to_raw_data: u32,
    pointer_to_relocations: u32,
    pointer_to_linenumbers: u32,
    number_of_relocations: u16,
    number_of_linenumbers: u16,
    characteristics: u32,
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
struct ImageImportDescriptor {
    original_first_thunk: u32, // RVA to original unbound IAT
    time_date_stamp: u32,
    forwarder_chain: u32,
    name: u32,                // RVA to DLL name
    first_thunk: u32,         // RVA to IAT
}

#[allow(dead_code)]
#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
struct ImageThunkData64 {
    address_of_data: u64,
}

pub struct PeLoader;

impl PeLoader {
    /// Load a PE binary from a byte slice into memory
    pub fn load(data: &[u8], vmm: &mut vmm::VirtualMemoryManager) -> Result<LoadedImage, LoaderError> {
        // Create syscall stubs for direct patching
        let stubs_base = PePatcher::create_stubs(vmm).ok_or(LoaderError::OutOfMemory)?;

        if data.len() < mem::size_of::<DosHeader>() {
            return Err(LoaderError::InvalidFormat);
        }

        // 1. Parse DOS Header
        let dos_header_ptr = data.as_ptr() as *const DosHeader;
        let e_magic = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*dos_header_ptr).e_magic)) };
        if e_magic != IMAGE_DOS_SIGNATURE {
            kprintln!("[PE] Invalid DOS magic: {:#x}", e_magic);
            return Err(LoaderError::InvalidFormat);
        }

        // 2. Parse NT Headers (PE Header)
        let e_lfanew = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*dos_header_ptr).e_lfanew)) };
        let nt_header_offset = e_lfanew as usize;
        if data.len() < nt_header_offset + 4 + mem::size_of::<PeFileHeader>() {
            return Err(LoaderError::InvalidFormat);
        }

        let signature = unsafe { core::ptr::read_unaligned(data.as_ptr().add(nt_header_offset) as *const u32) };
        if signature != IMAGE_NT_SIGNATURE {
            kprintln!("[PE] Invalid NT signature: {:#x}", signature);
            return Err(LoaderError::InvalidFormat);
        }

        let file_header_ptr = unsafe { 
            data.as_ptr().add(nt_header_offset + 4) as *const PeFileHeader 
        };

        // 3. Parse Optional Header
        let optional_header_offset = nt_header_offset + 4 + mem::size_of::<PeFileHeader>();
        if data.len() < optional_header_offset + mem::size_of::<PeOptionalHeader64>() {
            return Err(LoaderError::InvalidFormat);
        }

        let optional_header_ptr = unsafe {
            data.as_ptr().add(optional_header_offset) as *const PeOptionalHeader64
        };

        let magic = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*optional_header_ptr).magic)) };
        if magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC {
            kprintln!("[PE] Not a 64-bit PE file (magic: {:#x})", magic);
            return Err(LoaderError::InvalidFormat);
        }

        let image_base = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*optional_header_ptr).image_base)) };
        let address_of_entry_point = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*optional_header_ptr).address_of_entry_point)) };
        let entry_point = image_base + address_of_entry_point as u64;

        let num_sections = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*file_header_ptr).number_of_sections)) };
        let size_of_optional_header = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*file_header_ptr).size_of_optional_header)) };

        kprintln!("[PE] Loading image base={:#x}, entry={:#x}, sections={}",
                  image_base, entry_point, num_sections);

        // 4. Parse Section Headers
        let section_header_offset = optional_header_offset + size_of_optional_header as usize;
        
        if data.len() < section_header_offset + (num_sections as usize) * mem::size_of::<PeSectionHeader>() {
            return Err(LoaderError::InvalidFormat);
        }

        let section_headers_ptr = unsafe {
            data.as_ptr().add(section_header_offset) as *const PeSectionHeader
        };

        // 5. Map Sections
        for i in 0..num_sections {
            let section_ptr = unsafe { section_headers_ptr.add(i as usize) };
            let virtual_size = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*section_ptr).virtual_size)) };
            if virtual_size == 0 {
                continue;
            }

            Self::load_section(section_ptr, image_base, data, vmm)?;
        }

        // 6. Patch Import Table
        Self::patch_imports(optional_header_ptr, image_base, vmm, stubs_base)?;

        Ok(LoadedImage {
            entry: entry_point,
        })
    }

    /// Load and map a single PE section
    fn load_section(
        section_ptr: *const PeSectionHeader,
        image_base: u64,
        data: &[u8],
        vmm: &mut vmm::VirtualMemoryManager
    ) -> Result<(), LoaderError> {
        let virtual_address = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*section_ptr).virtual_address)) };
        let virtual_size = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*section_ptr).virtual_size)) };
        let size_of_raw_data = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*section_ptr).size_of_raw_data)) };
        let pointer_to_raw_data = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*section_ptr).pointer_to_raw_data)) };
        let characteristics = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*section_ptr).characteristics)) };

        let vaddr = image_base + virtual_address as u64;
        let memsz = virtual_size as usize;
        let filesz = size_of_raw_data as usize;
        let page_size = 4096;

        // Align to page boundaries
        let page_start = vaddr & !(page_size - 1);
        let page_end = (vaddr + memsz as u64 + page_size - 1) & !(page_size - 1);
        let page_count = (page_end - page_start) / page_size;

        for i in 0..page_count {
            let page_virt = page_start + (i * page_size);
            let phys = pmm::alloc_page().ok_or(LoaderError::OutOfMemory)?;
            
            let mut flags = PageFlags::USER | PageFlags::PRESENT;
            if characteristics & IMAGE_SCN_MEM_WRITE != 0 {
                flags |= PageFlags::WRITABLE;
            }
            if characteristics & IMAGE_SCN_MEM_EXECUTE == 0 {
                flags |= PageFlags::NO_EXECUTE;
            }
            
            vmm.map_page_with_flags(page_virt, phys, flags).map_err(|_| LoaderError::OutOfMemory)?;
            
            // Physical memory pointer via HHDM
            let dest_page = vmm::phys_to_virt(phys) as *mut u8;
            
            // Zero the page initially
            unsafe {
                core::ptr::write_bytes(dest_page, 0, 4096);
            }

            // Calculate overlap between this page and the section data in file
            let sec_start_vaddr = vaddr;
            let sec_end_vaddr = vaddr + filesz as u64;
            
            let copy_start = page_virt.max(sec_start_vaddr);
            let copy_end = (page_virt + 4096).min(sec_end_vaddr);
            
            if copy_start < copy_end {
                let len = (copy_end - copy_start) as usize;
                let offset_in_file = pointer_to_raw_data as u64 + (copy_start - sec_start_vaddr);
                let offset_in_page = (copy_start - page_virt) as usize;
                
                if (offset_in_file + len as u64) as usize <= data.len() {
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            data.as_ptr().add(offset_in_file as usize),
                            dest_page.add(offset_in_page),
                            len
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// Patch imports to point to syscall stubs
    fn patch_imports(
        optional_header_ptr: *const PeOptionalHeader64,
        image_base: u64,
        vmm: &vmm::VirtualMemoryManager,
        stubs_base: u64
    ) -> Result<(), LoaderError> {
        let num_directories = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*optional_header_ptr).number_of_rva_and_sizes)) };
        if (IMAGE_DIRECTORY_ENTRY_IMPORT as u32) >= num_directories {
            return Ok(());
        }

        let import_dir_ptr = unsafe { core::ptr::addr_of!((*optional_header_ptr).data_directory[IMAGE_DIRECTORY_ENTRY_IMPORT]) };
        let import_dir_addr = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*import_dir_ptr).virtual_address)) };
        if import_dir_addr == 0 {
            return Ok(());
        }

        let mut descriptor_rva = import_dir_addr;
        let mut name_buf = [0u8; 128];
        let mut func_name_buf = [0u8; 128];

        loop {
            let descriptor_ptr = Self::rva_to_kernel_ptr(descriptor_rva, image_base, vmm)
                .ok_or(LoaderError::InvalidFormat)? as *const ImageImportDescriptor;
            
            let name_rva = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*descriptor_ptr).name)) };
            if name_rva == 0 {
                break;
            }

            if Self::read_string_to_buf(image_base + name_rva as u64, vmm, &mut name_buf) {
                if let Ok(dll_name) = core::str::from_utf8(Self::trim_null(&name_buf)) {
                    // kprintln!("[PE] Processing imports for DLL: {}", dll_name);

                    let original_first_thunk = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*descriptor_ptr).original_first_thunk)) };
                    let first_thunk = unsafe { core::ptr::read_unaligned(core::ptr::addr_of!((*descriptor_ptr).first_thunk)) };

                    let mut thunk_rva = if original_first_thunk != 0 {
                        original_first_thunk
                    } else {
                        first_thunk
                    };
                    
                    let mut iat_rva = first_thunk;

                    loop {
                        let thunk_ptr = Self::rva_to_kernel_ptr(thunk_rva, image_base, vmm)
                            .ok_or(LoaderError::InvalidFormat)? as *const u64;
                        let thunk = unsafe { core::ptr::read_unaligned(thunk_ptr) };

                        if thunk == 0 {
                            break;
                        }

                        // If not an ordinal import
                        if thunk & IMAGE_ORDINAL_FLAG64 == 0 {
                            // thunk is RVA to IMAGE_IMPORT_BY_NAME
                            // IMAGE_IMPORT_BY_NAME { u16 hint, u8 name[] }
                            let name_rva = (thunk as u32) + 2;
                            if Self::read_string_to_buf(image_base + name_rva as u64, vmm, &mut func_name_buf) {
                                if let Ok(func_name) = core::str::from_utf8(Self::trim_null(&func_name_buf)) {
                                    if let Some(stub_addr) = PePatcher::get_stub_address(stubs_base, func_name) {
                                        // Patch the IAT entry
                                        let iat_ptr = Self::vaddr_to_kernel_ptr(image_base + iat_rva as u64, vmm)
                                            .ok_or(LoaderError::InvalidFormat)? as *mut u64;
                                        unsafe {
                                            core::ptr::write_unaligned(iat_ptr, stub_addr);
                                        }
                                        kprintln!("[PE] Patched {}!{} -> {:#x}", dll_name, func_name, stub_addr);
                                    }
                                }
                            }
                        }

                        thunk_rva += 8;
                        iat_rva += 8;
                    }
                }
            }

            descriptor_rva += mem::size_of::<ImageImportDescriptor>() as u32;
        }

        Ok(())
    }

    fn rva_to_kernel_ptr(rva: u32, image_base: u64, vmm: &vmm::VirtualMemoryManager) -> Option<*mut u8> {
        Self::vaddr_to_kernel_ptr(image_base + rva as u64, vmm)
    }

    fn vaddr_to_kernel_ptr(vaddr: u64, vmm: &vmm::VirtualMemoryManager) -> Option<*mut u8> {
        let phys = vmm.translate(vaddr)?;
        let offset = vaddr % 4096;
        Some(unsafe { (vmm::phys_to_virt(phys) as *mut u8).add(offset as usize) })
    }

    fn read_string_to_buf(vaddr: u64, vmm: &vmm::VirtualMemoryManager, buf: &mut [u8]) -> bool {
        let mut len = 0;
        while len < buf.len() - 1 {
            let curr_vaddr = vaddr + len as u64;
            if let Some(phys) = vmm.translate(curr_vaddr) {
                let byte = unsafe { core::ptr::read((vmm::phys_to_virt(phys) + (curr_vaddr % 4096)) as *const u8) };
                buf[len] = byte;
                if byte == 0 {
                    return true;
                }
                len += 1;
            } else {
                return false;
            }
        }
        buf[buf.len() - 1] = 0;
        true
    }

    fn trim_null(buf: &[u8]) -> &[u8] {
        let mut len = 0;
        while len < buf.len() && buf[len] != 0 {
            len += 1;
        }
        &buf[0..len]
    }
}
