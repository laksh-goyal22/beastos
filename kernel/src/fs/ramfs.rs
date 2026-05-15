//! RAM Filesystem for Beast OS
//!
//! A simple in-memory filesystem that supports files and directories.
//! Uses a nested tree structure with Spinlocks for thread-safe access.

use alloc::sync::Arc;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::boxed::Box;
use crate::sync::Spinlock;
use crate::fs::vfs::{File, FileSystem, FsError};

/// A node in the RAM filesystem.
#[derive(Clone)]
pub enum RamFsNode {
    /// A file node.
    File(Arc<Spinlock<RamFsFile>>),
    /// A directory node.
    Directory(Arc<Spinlock<RamFsDirectory>>),
}

/// A file in the RAM filesystem. Stores its data in a Vec<u8>.
pub struct RamFsFile {
    data: Vec<u8>,
}

/// A directory in the RAM filesystem. Stores its children in a BTreeMap.
pub struct RamFsDirectory {
    entries: BTreeMap<String, RamFsNode>,
}

/// The RAM filesystem implementation.
pub struct RamFs {
    root: Arc<Spinlock<RamFsDirectory>>,
}

/// A handle to a file in the RAM filesystem, implementing the File trait.
pub struct RamFsFileHandle {
    file: Arc<Spinlock<RamFsFile>>,
    cursor: usize,
}

impl File for RamFsFileHandle {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, FsError> {
        let file = self.file.lock();
        if self.cursor >= file.data.len() {
            return Ok(0);
        }
        
        let available = file.data.len() - self.cursor;
        let to_read = available.min(buf.len());
        buf[..to_read].copy_from_slice(&file.data[self.cursor..self.cursor + to_read]);
        self.cursor += to_read;
        Ok(to_read)
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize, FsError> {
        let mut file = self.file.lock();
        let end = self.cursor + buf.len();
        
        if end > file.data.len() {
            file.data.resize(end, 0);
        }
        
        file.data[self.cursor..end].copy_from_slice(buf);
        self.cursor = end;
        Ok(buf.len())
    }

    fn size(&self) -> u64 {
        self.file.lock().data.len() as u64
    }

    fn seek(&mut self, offset: u64) -> Result<u64, FsError> {
        let file = self.file.lock();
        if offset as usize > file.data.len() {
            return Err(FsError::IoError);
        }
        self.cursor = offset as usize;
        Ok(offset)
    }
}

impl RamFs {
    /// Create a new RAM filesystem.
    pub fn new() -> Self {
        Self {
            root: Arc::new(Spinlock::new(RamFsDirectory {
                entries: BTreeMap::new(),
            })),
        }
    }

    /// Resolve a path to its parent directory and the name of the leaf.
    fn resolve_parent(&self, path: &str) -> Result<(Arc<Spinlock<RamFsDirectory>>, String), FsError> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return Err(FsError::NotFound);
        }

        let mut current = self.root.clone();
        for i in 0..parts.len() - 1 {
            let next_node = {
                let dir = current.lock();
                match dir.entries.get(parts[i]) {
                    Some(RamFsNode::Directory(d)) => d.clone(),
                    Some(RamFsNode::File(_)) => return Err(FsError::NotDirectory),
                    None => return Err(FsError::NotFound),
                }
            };
            current = next_node;
        }

        Ok((current, parts.last().unwrap().to_string()))
    }
}

impl FileSystem for RamFs {
    fn open(&self, path: &str) -> Result<Box<dyn File>, FsError> {
        if path == "/" || path.is_empty() {
            return Err(FsError::IsDirectory);
        }

        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_dir = self.root.clone();

        for (i, part) in parts.iter().enumerate() {
            let is_last = i == parts.len() - 1;
            let node = {
                let dir = current_dir.lock();
                dir.entries.get(*part).cloned()
            };

            match node {
                Some(RamFsNode::Directory(d)) => {
                    if is_last {
                        return Err(FsError::IsDirectory);
                    }
                    current_dir = d;
                }
                Some(RamFsNode::File(f)) => {
                    if is_last {
                        return Ok(Box::new(RamFsFileHandle {
                            file: f,
                            cursor: 0,
                        }));
                    } else {
                        return Err(FsError::NotDirectory);
                    }
                }
                None => {
                    if is_last {
                        // Create the file if it doesn't exist (acting as O_CREAT)
                        let new_file = Arc::new(Spinlock::new(RamFsFile { data: Vec::new() }));
                        let mut dir = current_dir.lock();
                        dir.entries.insert(part.to_string(), RamFsNode::File(new_file.clone()));
                        return Ok(Box::new(RamFsFileHandle {
                            file: new_file,
                            cursor: 0,
                        }));
                    } else {
                        return Err(FsError::NotFound);
                    }
                }
            }
        }
        
        Err(FsError::NotFound)
    }

    fn read_dir(&self, path: &str) -> Result<Vec<String>, FsError> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_dir = self.root.clone();

        for part in parts {
            let next_node = {
                let dir = current_dir.lock();
                match dir.entries.get(part) {
                    Some(RamFsNode::Directory(d)) => d.clone(),
                    _ => return Err(FsError::NotDirectory),
                }
            };
            current_dir = next_node;
        }

        let dir = current_dir.lock();
        let mut entries = Vec::new();
        for (name, node) in &dir.entries {
            let mut entry = name.clone();
            if let RamFsNode::Directory(_) = node {
                entry.push('/');
            }
            entries.push(entry);
        }
        Ok(entries)
    }

    fn mkdir(&self, path: &str) -> Result<(), FsError> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return Err(FsError::AlreadyExists);
        }

        let mut current_dir = self.root.clone();
        for (i, part) in parts.iter().enumerate() {
            let is_last = i == parts.len() - 1;
            let next_node = {
                let dir = current_dir.lock();
                dir.entries.get(*part).cloned()
            };

            match next_node {
                Some(RamFsNode::Directory(d)) => {
                    if is_last {
                        return Err(FsError::AlreadyExists);
                    }
                    current_dir = d;
                }
                Some(RamFsNode::File(_)) => return Err(FsError::NotDirectory),
                None => {
                    if is_last {
                        let new_dir = Arc::new(Spinlock::new(RamFsDirectory {
                            entries: BTreeMap::new(),
                        }));
                        let mut dir = current_dir.lock();
                        dir.entries.insert(part.to_string(), RamFsNode::Directory(new_dir));
                        return Ok(());
                    } else {
                        return Err(FsError::NotFound);
                    }
                }
            }
        }
        
        Ok(())
    }

    fn remove(&self, path: &str) -> Result<(), FsError> {
        let (parent, name) = self.resolve_parent(path)?;
        let mut parent_dir = parent.lock();
        if parent_dir.entries.remove(&name).is_some() {
            Ok(())
        } else {
            Err(FsError::NotFound)
        }
    }
}
