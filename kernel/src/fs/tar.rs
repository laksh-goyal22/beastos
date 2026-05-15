//! Simple tarfs (Tar Filesystem) loader
//!
//! Because Beast OS uses Limine to load modules (like an initrd tarball),
//! we can implement a basic read-only filesystem over a tar archive to
//! load our ELF binaries before building a full VFS/ext4 driver.

use core::str;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::boxed::Box;
use crate::fs::vfs::{File, FileSystem, FsError};

/// A file entry in the Tar archive.
#[derive(Debug, Clone)]
pub struct TarFile {
    pub name: String,
    pub data: Vec<u8>,
}

impl File for TarFile {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, FsError> {
        // Simple implementation: always read from start since we don't have seek yet
        let len = self.data.len().min(buf.len());
        buf[..len].copy_from_slice(&self.data[..len]);
        Ok(len)
    }
    
    fn write(&mut self, _buf: &[u8]) -> Result<usize, FsError> {
        Err(FsError::PermissionDenied)
    }
    
    fn size(&self) -> u64 {
        self.data.len() as u64
    }
}

impl TarFile {
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        let offset = offset as usize;
        if offset >= self.data.len() {
            return Ok(0);
        }
        
        let available = self.data.len() - offset;
        let to_copy = available.min(buf.len());
        
        buf[..to_copy].copy_from_slice(&self.data[offset..offset + to_copy]);
        Ok(to_copy)
    }
}

/// A simple Tar archive parser.
pub struct TarArchive {
    raw_data: &'static [u8],
}

impl TarArchive {
    pub fn new(data: &'static [u8]) -> Self {
        Self { raw_data: data }
    }
    
    pub fn get_file(&self, name: &str) -> Option<TarFile> {
        let name = name.trim_start_matches('/');
        let mut offset = 0;
        
        while offset + 512 <= self.raw_data.len() {
            let header = &self.raw_data[offset..offset + 512];
            if header[0] == 0 { break; }
            
            let name_len = header[0..100].iter().position(|&c| c == 0).unwrap_or(100);
            let file_name = str::from_utf8(&header[0..name_len]).unwrap_or("");
            
            let size_bytes = &header[124..136];
            let size_str = str::from_utf8(size_bytes).unwrap_or("0").trim_matches(|c| c == ' ' || c == '\0');
            let size = usize::from_str_radix(size_str, 8).unwrap_or(0);
            
            let data_offset = offset + 512;
            let next_offset = data_offset + ((size + 511) & !511);
            
            if file_name == name {
                if data_offset + size > self.raw_data.len() {
                    return None;
                }
                let data = self.raw_data[data_offset..data_offset + size].to_vec();
                return Some(TarFile {
                    name: String::from(file_name),
                    data,
                });
            }
            
            offset = next_offset;
        }
        None
    }
}

impl FileSystem for TarArchive {
    fn open(&self, path: &str) -> Result<Box<dyn File>, FsError> {
        let path = path.trim_start_matches('/');
        let mut offset = 0;
        
        while offset + 512 <= self.raw_data.len() {
            let header = &self.raw_data[offset..offset + 512];
            if header[0] == 0 { break; }
            
            let name_len = header[0..100].iter().position(|&c| c == 0).unwrap_or(100);
            let file_name = str::from_utf8(&header[0..name_len]).unwrap_or("");
            
            let size_bytes = &header[124..136];
            let size_str = str::from_utf8(size_bytes).unwrap_or("0").trim_matches(|c| c == ' ' || c == '\0');
            let size = usize::from_str_radix(size_str, 8).unwrap_or(0);
            
            let data_offset = offset + 512;
            let next_offset = data_offset + ((size + 511) & !511);
            
            if file_name == path {
                if data_offset + size > self.raw_data.len() {
                    return Err(FsError::IoError);
                }
                let data = self.raw_data[data_offset..data_offset + size].to_vec();
                return Ok(Box::new(TarFile { name: String::from(file_name), data }));
            }
            
            offset = next_offset;
        }
        
        Err(FsError::NotFound)
    }
    
    fn read_dir(&self, path: &str) -> Result<Vec<String>, FsError> {
        let path = path.trim_matches('/');
        let mut entries = Vec::new();
        let mut offset = 0;
        
        while offset + 512 <= self.raw_data.len() {
            let header = &self.raw_data[offset..offset + 512];
            if header[0] == 0 { break; }
            
            let name_len = header[0..100].iter().position(|&c| c == 0).unwrap_or(100);
            let file_name = str::from_utf8(&header[0..name_len]).unwrap_or("");
            
            let size_bytes = &header[124..136];
            let size_str = str::from_utf8(size_bytes).unwrap_or("0").trim_matches(|c| c == ' ' || c == '\0');
            let size = usize::from_str_radix(size_str, 8).unwrap_or(0);
            
            let data_offset = offset + 512;
            let next_offset = data_offset + ((size + 511) & !511);
            
            if path.is_empty() {
                // Root directory
                let first_part = file_name.split('/').next().unwrap();
                if file_name.contains('/') {
                    let mut dir_name = String::from(first_part);
                    dir_name.push('/');
                    if !entries.contains(&dir_name) {
                        entries.push(dir_name);
                    }
                } else {
                    if !entries.contains(&String::from(file_name)) {
                        entries.push(String::from(file_name));
                    }
                }
            } else {
                let normalized_file_name = file_name.trim_start_matches('/');
                if normalized_file_name.starts_with(path) && normalized_file_name != path {
                    let relative = &normalized_file_name[path.len()..].trim_start_matches('/');
                    if !relative.is_empty() {
                        let first_part = relative.split('/').next().unwrap();
                        if relative.contains('/') {
                            let mut dir_name = String::from(first_part);
                            dir_name.push('/');
                            if !entries.contains(&dir_name) {
                                entries.push(dir_name);
                            }
                        } else {
                            if !entries.contains(&String::from(first_part)) {
                                entries.push(String::from(first_part));
                            }
                        }
                    }
                }
            }
            
            offset = next_offset;
        }
        
        Ok(entries)
    }
}
