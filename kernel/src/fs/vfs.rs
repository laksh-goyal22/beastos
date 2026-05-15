//! Virtual File System - Minimal version

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::Spinlock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    NotFound,
    PermissionDenied,
    IoError,
    IsDirectory,
    NotDirectory,
    AlreadyExists,
}

pub trait File: Send + Sync {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, FsError>;
    fn write(&mut self, buf: &[u8]) -> Result<usize, FsError>;
    fn size(&self) -> u64;
    fn seek(&mut self, _offset: u64) -> Result<u64, FsError> { Err(FsError::IoError) }
}

pub trait FileSystem: Send + Sync {
    fn open(&self, path: &str) -> Result<Box<dyn File>, FsError>;
    fn read_dir(&self, path: &str) -> Result<Vec<String>, FsError>;
    fn mkdir(&self, _path: &str) -> Result<(), FsError> { Err(FsError::PermissionDenied) }
    fn remove(&self, _path: &str) -> Result<(), FsError> { Err(FsError::PermissionDenied) }
}

pub struct Vfs {
    filesystems: Vec<(String, Box<dyn FileSystem>)>,
}

impl Vfs {
    pub const fn new() -> Self {
        Self { filesystems: Vec::new() }
    }
    
    pub fn mount(&mut self, prefix: &str, fs: Box<dyn FileSystem>) {
        self.filesystems.push((String::from(prefix), fs));
    }
    
    pub fn open(&self, path: &str) -> Result<Box<dyn File>, FsError> {
        for (prefix, fs) in self.filesystems.iter().rev() {
            if path.starts_with(prefix) {
                let relative_path = &path[prefix.len()..];
                let relative_path = if relative_path.is_empty() { "/" } else { relative_path };
                return fs.open(relative_path);
            }
        }
        Err(FsError::NotFound)
    }
    
    pub fn read_dir(&self, path: &str) -> Result<Vec<String>, FsError> {
        for (prefix, fs) in self.filesystems.iter().rev() {
            if path.starts_with(prefix) {
                let relative_path = &path[prefix.len()..];
                let relative_path = if relative_path.is_empty() { "/" } else { relative_path };
                return fs.read_dir(relative_path);
            }
        }
        Err(FsError::NotFound)
    }

    pub fn mkdir(&self, path: &str) -> Result<(), FsError> {
        for (prefix, fs) in self.filesystems.iter().rev() {
            if path.starts_with(prefix) {
                let relative_path = &path[prefix.len()..];
                let relative_path = if relative_path.is_empty() { "/" } else { relative_path };
                return fs.mkdir(relative_path);
            }
        }
        Err(FsError::NotFound)
    }

    pub fn remove(&self, path: &str) -> Result<(), FsError> {
        for (prefix, fs) in self.filesystems.iter().rev() {
            if path.starts_with(prefix) {
                let relative_path = &path[prefix.len()..];
                let relative_path = if relative_path.is_empty() { "/" } else { relative_path };
                return fs.remove(relative_path);
            }
        }
        Err(FsError::NotFound)
    }
}

pub static VFS: Spinlock<Vfs> = Spinlock::new(Vfs::new());

pub fn init() {
    crate::kprintln!("  [VFS] Initialized");
}