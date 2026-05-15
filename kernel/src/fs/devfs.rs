//! Device Filesystem (devfs)
//!
//! Provides device nodes accessible through VFS:
//! - `/dev/stdin`   — keyboard character input
//! - `/dev/mouse`   — mouse state (binary: x:i32, y:i32, buttons:u8)
//! - `/dev/fb`      — framebuffer info (width, height, pitch, phys_base)
//! - `/dev/serial`  — serial port (COM1) raw I/O

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use crate::fs::vfs::{File, FileSystem, FsError};
use crate::kprintln;

// ---------------------------------------------------------------------------
// Device node entries
// ---------------------------------------------------------------------------

const DEVICE_LIST: &[&str] = &["stdin", "mouse", "fb", "serial"];

// ---------------------------------------------------------------------------
// Stdin file — reads from keyboard buffer
// ---------------------------------------------------------------------------

pub struct StdinFile;

impl File for StdinFile {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, FsError> {
        let mut copied = 0usize;

        // Block until at least one byte is available
        loop {
            let ch = crate::syscall::pop_keyboard_char();
            match ch {
                Some(byte) => {
                    buf[copied] = byte;
                    copied += 1;
                    // Drain remaining available bytes
                    for dst in buf[copied..].iter_mut() {
                        match crate::syscall::pop_keyboard_char() {
                            Some(b) => { *dst = b; copied += 1; }
                            None => break,
                        }
                    }
                    return Ok(copied);
                }
                None => {
                    // No data — yield and retry
                    crate::scheduler::yield_now();
                }
            }
        }
    }

    fn write(&mut self, _buf: &[u8]) -> Result<usize, FsError> {
        Err(FsError::PermissionDenied)
    }

    fn size(&self) -> u64 { 0 }
}

// ---------------------------------------------------------------------------
// Mouse file — reads current MouseState as binary
// ---------------------------------------------------------------------------

pub struct MouseFile;

impl File for MouseFile {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, FsError> {
        let state = crate::drivers::mouse::MOUSE_STATE.lock();
        let data: [u8; 12] = [
            state.x.to_le_bytes()[0],
            state.x.to_le_bytes()[1],
            state.x.to_le_bytes()[2],
            state.x.to_le_bytes()[3],
            state.y.to_le_bytes()[0],
            state.y.to_le_bytes()[1],
            state.y.to_le_bytes()[2],
            state.y.to_le_bytes()[3],
            state.left_button as u8 | ((state.right_button as u8) << 1) | ((state.middle_button as u8) << 2),
            0, 0, 0,
        ];
        let n = buf.len().min(12);
        buf[..n].copy_from_slice(&data[..n]);
        Ok(n)
    }

    fn write(&mut self, _buf: &[u8]) -> Result<usize, FsError> {
        Err(FsError::PermissionDenied)
    }

    fn size(&self) -> u64 { 12 }
}

// ---------------------------------------------------------------------------
// Framebuffer info file
// ---------------------------------------------------------------------------

pub struct FbFile;

impl File for FbFile {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, FsError> {
        let info = crate::drivers::framebuffer::get_phys_info().unwrap_or((0, 0));
        let w = crate::drivers::framebuffer::width();
        let h = crate::drivers::framebuffer::height();
        let p = crate::drivers::framebuffer::pitch();
        let data: [u8; 32] = {
            let mut d = [0u8; 32];
            d[0..4].copy_from_slice(&w.to_le_bytes());
            d[4..8].copy_from_slice(&h.to_le_bytes());
            d[8..12].copy_from_slice(&p.to_le_bytes());
            d[12..20].copy_from_slice(&info.0.to_le_bytes());
            d[20..28].copy_from_slice(&info.1.to_le_bytes());
            d
        };
        let n = buf.len().min(32);
        buf[..n].copy_from_slice(&data[..n]);
        Ok(n)
    }

    fn write(&mut self, _buf: &[u8]) -> Result<usize, FsError> {
        Err(FsError::PermissionDenied)
    }

    fn size(&self) -> u64 { 32 }
}

// ---------------------------------------------------------------------------
// Serial file — raw COM1 read/write
// ---------------------------------------------------------------------------

pub struct SerialFile;

impl File for SerialFile {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, FsError> {
        unsafe {
            let mut copied = 0usize;
            for byte in buf.iter_mut() {
                // Check if data is available (LSR bit 0)
                let lsr: u8;
                core::arch::asm!("in al, dx", in("dx") 0x3F8u16 + 5, out("al") lsr, options(nomem, nostack));
                if lsr & 1 == 0 {
                    break;
                }
                core::arch::asm!("in al, dx", in("dx") 0x3F8u16, out("al") *byte, options(nomem, nostack));
                copied += 1;
            }
            Ok(copied)
        }
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize, FsError> {
        unsafe {
            for &byte in buf {
                // Wait for transmitter holding register empty (LSR bit 5)
                loop {
                    let lsr: u8;
                    core::arch::asm!("in al, dx", in("dx") 0x3F8u16 + 5, out("al") lsr, options(nomem, nostack));
                    if lsr & (1 << 5) != 0 {
                        break;
                    }
                }
                core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") byte, options(nomem, nostack));
            }
            Ok(buf.len())
        }
    }

    fn size(&self) -> u64 { 0 }
}

// ---------------------------------------------------------------------------
// DevFs — device filesystem
// ---------------------------------------------------------------------------

pub struct DevFs;

impl FileSystem for DevFs {
    fn open(&self, path: &str) -> Result<Box<dyn File>, FsError> {
        let path = path.trim_start_matches('/');
        match path {
            "stdin"  => Ok(Box::new(StdinFile)),
            "mouse"  => Ok(Box::new(MouseFile)),
            "fb"     => Ok(Box::new(FbFile)),
            "serial" => Ok(Box::new(SerialFile)),
            _ => Err(FsError::NotFound),
        }
    }

    fn read_dir(&self, _path: &str) -> Result<Vec<String>, FsError> {
        Ok(DEVICE_LIST.iter().map(|s| String::from(*s)).collect())
    }
}

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

use crate::fs::vfs::VFS;

pub fn init() {
    let fs = DevFs;
    VFS.lock().mount("/dev", Box::new(fs));
    kprintln!("  [DEVFS] Mounted at /dev (stdin, mouse, fb, serial)");
}
