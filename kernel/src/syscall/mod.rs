//! Beast OS System Call Interface

use crate::kprintln;
use crate::sync::Spinlock;
use core::sync::atomic::{AtomicUsize, Ordering};
use crate::scheduler::task::{BlockReason, NO_TASK};
use alloc::string::ToString;

static KEYBOARD_WAITING_TASK: AtomicUsize = AtomicUsize::new(NO_TASK);

pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_LSEEK: u64 = 8;
pub const SYS_CREATE: u64 = 9;
pub const SYS_DELETE: u64 = 10;
pub const SYS_MOVE: u64 = 14;
pub const SYS_COPY: u64 = 15;
pub const SYS_YIELD: u64 = 24;
pub const SYS_EXEC: u64 = 29;
pub const SYS_GETPID: u64 = 39;
pub const SYS_EXIT: u64 = 60;
pub const SYS_WAIT: u64 = 61;
pub const SYS_CHDIR: u64 = 80;
pub const SYS_BRK: u64 = 45;
pub const SYS_GETCWD: u64 = 81;
pub const SYS_LS: u64 = 82;
pub const SYS_SNAPSHOT: u64 = 101;
pub const SYS_RESTORE: u64 = 102;
pub const SYS_TAG: u64 = 103;
pub const SYS_BOND_WIFI: u64 = 104;
pub const SYS_CLEAR: u64 = 105;
pub const SYS_SHARE: u64 = 106;
pub const SYS_TIME_TRAVEL: u64 = 110;
pub const SYS_PS: u64 = 111;
pub const SYS_UPTIME: u64 = 120;
pub const SYS_CLIPBOARD_COPY: u64 = 150;
pub const SYS_CLIPBOARD_PASTE: u64 = 151;
pub const SYS_CLIPBOARD_CLEAR: u64 = 152;
pub const SYS_GET_MOUSE: u64 = 153;
pub const SYS_FB_INFO: u64 = 210;
pub const SYS_MKDIR: u64 = 500;

static CLIPBOARD_BUFFER: Spinlock<alloc::vec::Vec<u8>> = Spinlock::new(alloc::vec::Vec::new());

pub mod driver_syscalls;
use driver_syscalls::*;

// Simple circular keyboard buffer
struct KeyboardBuffer {
    data: [u8; 256],
    read_idx: usize,
    write_idx: usize,
    count: usize,
}

impl KeyboardBuffer {
    const fn new() -> Self {
        Self {
            data: [0; 256],
            read_idx: 0,
            write_idx: 0,
            count: 0,
        }
    }

    fn push(&mut self, ch: u8) {
        if self.count < 256 {
            self.data[self.write_idx] = ch;
            self.write_idx = (self.write_idx + 1) % 256;
            self.count += 1;
        }
    }

    fn pop(&mut self) -> Option<u8> {
        if self.count > 0 {
            let ch = self.data[self.read_idx];
            self.read_idx = (self.read_idx + 1) % 256;
            self.count -= 1;
            Some(ch)
        } else {
            None
        }
    }

    fn is_empty(&self) -> bool { self.count == 0 }
}

static KEYBOARD_BUFFER: Spinlock<KeyboardBuffer> = Spinlock::new(KeyboardBuffer::new());

pub fn push_keyboard_char(ch: u8) {
    let mut buffer = KEYBOARD_BUFFER.lock();
    buffer.push(ch);
    
    // Wake up waiting task if any
    let waiting_task = KEYBOARD_WAITING_TASK.swap(NO_TASK, Ordering::SeqCst);
    if waiting_task != NO_TASK {
        crate::scheduler::wake_task(waiting_task);
    }
}

/// Pop a character from the keyboard buffer (non-blocking).
pub fn pop_keyboard_char() -> Option<u8> {
    KEYBOARD_BUFFER.lock().pop()
}

#[no_mangle]
pub extern "C" fn syscall_dispatch(
    num: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    _a4: u64,
    _a5: u64,
    ) -> i64 {
    match num {
        0 => sys_read(a1, a2, a3),
        1 => sys_write(a1, a2, a3),
        2 => sys_open(a1, a2),
        3 => sys_close(a1),
        8 => sys_lseek(a1, a2),
        9 => sys_create(a1, a2),
        10 => sys_delete(a1, a2),
        14 => sys_move(a1, a2, a3, _a4),
        15 => sys_copy(a1, a2, a3, _a4),
        24 => { crate::scheduler::yield_now(); 0 },
         29 => sys_exec(a1, a2, a3),
        39 => sys_getpid(),
        45 => sys_brk(a1),
        60 => sys_exit(a1),
        61 => { crate::scheduler::wait_task(a1 as usize); 0 },
        80 => sys_chdir(a1, a2),
        81 => sys_getcwd(a1, a2),
        82 => sys_ls(a1, a2, a3, _a4),
        101 => sys_snapshot(a1, a2),
        102 => sys_restore(a1),
        103 => sys_tag(a1, a2, a3),
        104 => sys_bond_wifi(a1, a2),
        105 => sys_clear_screen(),
        106 => sys_share(a1, a2, a3),
        110 => sys_time_travel(a1),
        111 => sys_ps(a1, a2),
        120 => crate::drivers::pit::get_ticks() as i64,
        150 => sys_clipboard_copy(a1, a2),
        151 => sys_clipboard_paste(a1, a2),
        152 => { CLIPBOARD_BUFFER.lock().clear(); 0 },
        153 => sys_get_mouse(a1),
        200 => match sys_map_mmio(a1, a2) { Ok(v) => v as i64, Err(e) => e as i64 },
        201 => match sys_bind_irq(a1 as u8, a2 as usize) { Ok(_) => 0, Err(e) => e as i64 },
        202 => match sys_create_irq_ring(a1 as usize) { Ok(id) => id as i64, Err(e) => e as i64 },
        203 => match sys_port_in(a1 as u16) { Ok(v) => v as i64, Err(e) => e as i64 },
        204 => match sys_port_out(a1 as u16, a2 as u32, a3 as u8) { Ok(_) => 0, Err(e) => e as i64 },
         210 => sys_fb_info(a1),
         300 => sys_io_uring_setup(a1, a2),
         301 => sys_io_uring_enter(a1, a2, a3),
         302 => sys_io_uring_register(a1, a2, a3),
         303 => sys_io_uring_stats(a1, a2),
         310 => sys_set_latency_class(a1),
         311 => sys_pressure(),
         312 => sys_system_metrics(a1),
         500 => sys_mkdir(a1, a2),
        _ => {
            -1
        }
    }
}

pub fn sys_read(fd: u64, buf_ptr: u64, count: u64) -> i64 {
    kprintln!("[SYS_READ] fd={} buf={:#x} count={}", fd, buf_ptr, count);
    if buf_ptr == 0 || count == 0 { return -22; }

    // Get current task's user CR3 and kernel CR3
    let (user_cr3, kernel_cr3, needs_switch) = {
        let sched = crate::scheduler::SCHEDULER.lock();
        let slot = sched.current;
        let ucr3 = sched.tasks[slot].as_ref().map(|t| t.page_table).unwrap_or(0);
        let kcr3 = unsafe { crate::arch::syscall_entry::KERNEL_CR3 };
        // Skip CR3 switch if user_cr3 == kernel_cr3 (same page table)
        (ucr3, kcr3, ucr3 != kcr3 && ucr3 != 0)
    };

    if user_cr3 == 0 {
        return -1;
    }

    if fd == 0 {
        loop {
            {
                let mut buffer = KEYBOARD_BUFFER.lock();
                if !buffer.is_empty() {
                    kprintln!("[SYS_READ] Buffer has data, copying {} chars", buffer.count);
                    let to_copy = (count as usize).min(buffer.count);
                    // Switch to user CR3 to write to user buffer (only if different)
                    unsafe {
                        if needs_switch {
                            crate::arch::paging::write_cr3(user_cr3);
                        }
                        let buf = core::slice::from_raw_parts_mut(buf_ptr as *mut u8, to_copy);
                        for i in 0..to_copy {
                            if let Some(ch) = buffer.pop() {
                                buf[i] = ch;
                            } else {
                                break;
                            }
                        }
                        if needs_switch {
                            crate::arch::paging::write_cr3(kernel_cr3);
                        }
                    }
                    return to_copy as i64;
                }
                kprintln!("[SYS_READ] Buffer empty, blocking task {}", crate::scheduler::current_slot());
                let current_slot = crate::scheduler::current_slot();
                KEYBOARD_WAITING_TASK.store(current_slot, Ordering::SeqCst);
            }
            crate::scheduler::block_current(BlockReason::Waiting);
        }
    }

    let mut sched = crate::scheduler::SCHEDULER.lock();
    let slot = sched.current;
    if let Some(ref mut task) = sched.tasks[slot] {
        let mut fds = task.fds.lock();
        if (fd as usize) < fds.len() {
            if let Some(ref mut file) = fds[fd as usize] {
                unsafe {
                    if needs_switch {
                        crate::arch::paging::write_cr3(user_cr3);
                    }
                    let buf = core::slice::from_raw_parts_mut(buf_ptr as *mut u8, count as usize);
                    let result = file.read(buf);
                    if needs_switch {
                        crate::arch::paging::write_cr3(kernel_cr3);
                    }
                    match result {
                        Ok(n) => return n as i64,
                        Err(_) => return -1,
                    }
                }
            }
        }
    }
    -9 // EBADF
}

/// Read from a file descriptor into a physical page (for zero-copy io_uring).
/// Returns bytes read, or negative on error.
pub fn read_file_into_phys(fd: u64, phys_addr: u64, count: u64) -> i64 {
    let mut sched = crate::scheduler::SCHEDULER.lock();
    let slot = sched.current;
    if let Some(ref mut task) = sched.tasks[slot] {
        let mut fds = task.fds.lock();
        if (fd as usize) < fds.len() {
            if let Some(ref mut file) = fds[fd as usize] {
                let kvirt = crate::memory::vmm::phys_to_virt(phys_addr);
                let count = count.min(4096) as usize;
                let buf = unsafe { core::slice::from_raw_parts_mut(kvirt as *mut u8, count) };
                return match file.read(buf) {
                    Ok(n) => n as i64,
                    Err(_) => -1,
                };
            }
        }
    }
    -9
}

pub fn sys_open(path_ptr: u64, path_len: u64) -> i64 {
    let user_cr3 = {
        let sched = crate::scheduler::SCHEDULER.lock();
        let slot = sched.current;
        sched.tasks[slot].as_ref().map(|t| t.page_table).unwrap_or(0)
    };
    let kernel_cr3 = unsafe { crate::arch::syscall_entry::KERNEL_CR3 };

    let path = match resolve_user_path(path_ptr, path_len, user_cr3, kernel_cr3) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let file = match crate::fs::vfs::VFS.lock().open(&path) {
        Ok(f) => f,
        Err(_) => return -2, // ENOENT
    };

    let mut sched = crate::scheduler::SCHEDULER.lock();
    let slot = sched.current;
    if let Some(ref mut task) = sched.tasks[slot] {
        let mut fds = task.fds.lock();
        for i in 3..fds.len() {
            if fds[i].is_none() {
                fds[i] = Some(file);
                return i as i64;
            }
        }
        let id = fds.len();
        if id < 3 {
            while fds.len() < 3 { fds.push(None); }
            let final_id = fds.len();
            fds.push(Some(file));
            return final_id as i64;
        }
        fds.push(Some(file));
        id as i64
    } else {
        -1
    }
}

pub fn sys_close(fd: u64) -> i64 {
    let mut sched = crate::scheduler::SCHEDULER.lock();
    let slot = sched.current;
    if let Some(ref mut task) = sched.tasks[slot] {
        let mut fds = task.fds.lock();
        if (fd as usize) < fds.len() {
            fds[fd as usize] = None;
            return 0;
        }
    }
    -9 // EBADF
}

fn sys_lseek(fd: u64, offset: u64) -> i64 {
    let mut sched = crate::scheduler::SCHEDULER.lock();
    let slot = sched.current;
    if let Some(ref mut task) = sched.tasks[slot] {
        let mut fds = task.fds.lock();
        if (fd as usize) < fds.len() {
            if let Some(ref mut file) = fds[fd as usize] {
                match file.seek(offset) {
                    Ok(new_off) => return new_off as i64,
                    Err(_) => return -1,
                }
            }
        }
    }
    -9 // EBADF
}

pub fn sys_write(fd: u64, buf_ptr: u64, count: u64) -> i64 {
    if buf_ptr == 0 || count == 0 { return -22; }

    let (user_cr3, kernel_cr3, needs_switch) = {
        let sched = crate::scheduler::SCHEDULER.lock();
        let slot = sched.current;
        let ucr3 = sched.tasks[slot].as_ref().map(|t| t.page_table).unwrap_or(0);
        let kcr3 = unsafe { crate::arch::syscall_entry::KERNEL_CR3 };
        (ucr3, kcr3, ucr3 != kcr3 && ucr3 != 0)
    };
    if user_cr3 == 0 { return -1; }

    let read_user_buf = |offset: u64, len: usize| -> Option<alloc::vec::Vec<u8>> {
        unsafe {
            if needs_switch {
                crate::arch::paging::write_cr3(user_cr3);
            }
            // Check that the read is within user-accessible range
            if buf_ptr + offset + len as u64 > 0x0000_8000_0000_0000 {
                if needs_switch { crate::arch::paging::write_cr3(kernel_cr3); }
                return None;
            }
            let mut data = alloc::vec::Vec::with_capacity(len);
            let src = core::slice::from_raw_parts((buf_ptr + offset) as *const u8, len);
            data.extend_from_slice(src);
            if needs_switch {
                crate::arch::paging::write_cr3(kernel_cr3);
            }
            Some(data)
        }
    };

    if fd == 1 || fd == 2 {
        let data = match read_user_buf(0, count as usize) {
            Some(d) => d,
            None => return -22,
        };
        if let Ok(s) = core::str::from_utf8(&data) {
            for byte in s.bytes() {
                unsafe { core::arch::asm!("out dx, al", in("dx") 0x3f8u16, in("al") byte); }
            }
            use crate::drivers::console::CONSOLE;
            let mut console = CONSOLE.lock();
            for byte in s.bytes() {
                console.write_byte(byte);
            }
            return count as i64;
        } else {
            return -22;
        }
    }

    let data = match read_user_buf(0, count as usize) {
        Some(d) => d,
        None => return -22,
    };

    let mut sched = crate::scheduler::SCHEDULER.lock();
    let slot = sched.current;
    if let Some(ref mut task) = sched.tasks[slot] {
        let mut fds = task.fds.lock();
        if (fd as usize) < fds.len() {
            if let Some(ref mut file) = fds[fd as usize] {
                match file.write(&data) {
                    Ok(n) => return n as i64,
                    Err(_) => return -1,
                }
            }
        }
    }
    -9 // EBADF
}

fn sys_clear_screen() -> i64 {
    let mut console = crate::drivers::console::CONSOLE.lock();
    let w = console.width;
    let h = console.height;
    console.bg_color = 0xFF000000;
    console.fg_color = 0xFFFFFFFF;
    console.init(w, h);
    0
}

fn sys_getpid() -> i64 {
    crate::scheduler::current_pid() as i64
}

fn sys_brk(addr: u64) -> i64 {
    let mut sched = crate::scheduler::SCHEDULER.lock();
    let slot = sched.current;
    if let Some(ref mut task) = sched.tasks[slot] {
        if addr == 0 {
            return task.brk as i64;
        }
        if addr < task.brk {
            // Shrinking — just update the break (pages stay mapped for now)
            task.brk = addr;
            return task.brk as i64;
        }
        // Growing — allocate pages up to the new break
        const MAX_HEAP: u64 = 32 * 1024 * 1024; // 32 MB
        let initial_brk = task.brk;
        if addr > initial_brk + MAX_HEAP {
            return -1; // ENOMEM
        }
        let start_page = (initial_brk + 4095) & !0xFFF;
        let end_page = addr & !0xFFF;
        if end_page > start_page {
            let cr3 = task.page_table;
            drop(sched);
            let mut vmm = crate::memory::vmm::VirtualMemoryManager::new(cr3);
            for page in (start_page..end_page).step_by(4096) {
                let phys = match crate::memory::pmm::alloc_page() {
                    Some(p) => p,
                    None => return -1,
                };
                unsafe {
                    core::ptr::write_bytes(
                        crate::memory::vmm::phys_to_virt(phys) as *mut u8, 0, 4096);
                }
                if let Err(_) = vmm.map_page_with_flags(page, phys,
                    crate::arch::paging::flags::PRESENT |
                    crate::arch::paging::flags::WRITABLE |
                    crate::arch::paging::flags::USER)
                {
                    return -1;
                }
            }
            let mut sched = crate::scheduler::SCHEDULER.lock();
            if let Some(ref mut task) = sched.tasks[slot] {
                task.brk = addr;
            }
        } else {
            let mut sched = crate::scheduler::SCHEDULER.lock();
            if let Some(ref mut task) = sched.tasks[slot] {
                task.brk = addr;
            }
        }
        addr as i64
    } else {
        -1
    }
}

fn sys_exit(code: u64) -> ! {
    kprintln!("[SYSCALL] PID {} exited with code: {}", crate::scheduler::current_pid(), code);
    crate::scheduler::exit_current();
}

fn sys_chdir(path_ptr: u64, path_len: u64) -> i64 {
    let user_cr3 = {
        let sched = crate::scheduler::SCHEDULER.lock();
        let slot = sched.current;
        sched.tasks[slot].as_ref().map(|t| t.page_table).unwrap_or(0)
    };
    let kernel_cr3 = unsafe { crate::arch::syscall_entry::KERNEL_CR3 };

    let path = match read_user_string_with_len(path_ptr, path_len, user_cr3, kernel_cr3) {
        Ok(s) => s,
        Err(e) => return e,
    };

    let mut sched = crate::scheduler::SCHEDULER.lock();
    let slot = sched.current;
    if let Some(ref mut task) = sched.tasks[slot] {
        use crate::fs::path_cache::PathNormalizer;
        let new_cwd = if path.starts_with('/') {
            PathNormalizer::normalize(&path)
        } else {
            PathNormalizer::join(&task.cwd, &path)
        };
        task.cwd = new_cwd;
        0
    } else {
        -1
    }
}

fn sys_getcwd(buf_ptr: u64, buf_len: u64) -> i64 {
    let user_cr3 = {
        let sched = crate::scheduler::SCHEDULER.lock();
        let slot = sched.current;
        sched.tasks[slot].as_ref().map(|t| t.page_table).unwrap_or(0)
    };
    let kernel_cr3 = unsafe { crate::arch::syscall_entry::KERNEL_CR3 };

    let sched = crate::scheduler::SCHEDULER.lock();
    let slot = sched.current;
    if let Some(ref task) = sched.tasks[slot] {
        let cwd = task.cwd.as_bytes();
        let to_copy = cwd.len().min(buf_len as usize);
        unsafe {
            crate::arch::paging::write_cr3(user_cr3);
            let buf = core::slice::from_raw_parts_mut(buf_ptr as *mut u8, to_copy);
            buf.copy_from_slice(&cwd[..to_copy]);
            crate::arch::paging::write_cr3(kernel_cr3);
        }
        to_copy as i64
    } else {
        -1
    }
}

fn read_user_string(ptr: u64, user_cr3: u64, kernel_cr3: u64) -> Result<alloc::string::String, i64> {
    if ptr == 0 { return Err(-22); }
    let mut s = alloc::string::String::new();
    let mut i = 0;
    unsafe {
        crate::arch::paging::write_cr3(user_cr3);
        loop {
            let c = *( (ptr + i) as *const u8 );
            if c == 0 { break; }
            s.push(c as char);
            i += 1;
            if i > 4096 { crate::arch::paging::write_cr3(kernel_cr3); return Err(-36); }
        }
        crate::arch::paging::write_cr3(kernel_cr3);
    }
    Ok(s)
}

fn read_user_string_with_len(ptr: u64, len: u64, user_cr3: u64, kernel_cr3: u64) -> Result<alloc::string::String, i64> {
    if ptr == 0 { return Err(-22); }
    unsafe {
        crate::arch::paging::write_cr3(user_cr3);
        let slice = core::slice::from_raw_parts(ptr as *const u8, len as usize);
        let result = match core::str::from_utf8(slice) {
            Ok(s) => Ok(alloc::string::String::from(s)),
            Err(_) => Err(-22),
        };
        crate::arch::paging::write_cr3(kernel_cr3);
        result
    }
}

fn resolve_user_path(path_ptr: u64, path_len: u64, user_cr3: u64, kernel_cr3: u64) -> Result<alloc::string::String, i64> {
    let path = if path_len == 0 {
        read_user_string(path_ptr, user_cr3, kernel_cr3)?
    } else {
        read_user_string_with_len(path_ptr, path_len, user_cr3, kernel_cr3)?
    };

    let sched = crate::scheduler::SCHEDULER.lock();
    let slot = sched.current;
    if let Some(ref task) = sched.tasks[slot] {
        use crate::fs::path_cache::PathNormalizer;
        if path.starts_with('/') {
            Ok(PathNormalizer::normalize(&path))
        } else if path == "." {
            Ok(task.cwd.clone())
        } else {
            Ok(PathNormalizer::join(&task.cwd, &path))
        }
    } else {
        Err(-1)
    }
}

static mut INITRD_DATA: &'static [u8] = &[];

pub fn set_initrd_data(data: &'static [u8]) {
    unsafe { INITRD_DATA = data; }
}

fn sys_exec(path_ptr: u64, path_len: u64, _argv_ptr: u64) -> i64 {
    let (user_cr3, kernel_cr3) = {
        let sched = crate::scheduler::SCHEDULER.lock();
        let slot = sched.current;
        let ucr3 = sched.tasks[slot].as_ref().map(|t| t.page_table).unwrap_or(0);
        let kcr3 = unsafe { crate::arch::syscall_entry::KERNEL_CR3 };
        (ucr3, kcr3)
    };
    if user_cr3 == 0 { return -1; }

    let path = if path_len > 0 {
        unsafe {
            crate::arch::paging::write_cr3(user_cr3);
            let slice = core::slice::from_raw_parts(path_ptr as *const u8, path_len as usize);
            let result = match core::str::from_utf8(slice) {
                Ok(s) => alloc::string::String::from(s),
                Err(_) => { crate::arch::paging::write_cr3(kernel_cr3); return -22; },
            };
            crate::arch::paging::write_cr3(kernel_cr3);
            result
        }
    } else {
        return -22;
    };

    // Convert to tar path (e.g., /bin/hello.beast -> bin/hello.beast)
    let tar_path = if let Some(stripped) = path.strip_prefix('/') {
        stripped.to_string()
    } else {
        path.to_string()
    };
    

    // Get file from archive
    let archive = unsafe { crate::fs::tar::TarArchive::new(INITRD_DATA) };
    let file = match archive.get_file(&tar_path) {
        Some(f) => f,
        None => return -2,
    };
    
    // Create user page table
    let page_table = match crate::memory::vmm::create_user_page_table() {
        Some(pt) => pt,
        None => return -12,
    };
    
    let mut vmm = crate::memory::vmm::VirtualMemoryManager::new(page_table);

    // Load ELF
    let image = match crate::fs::universal_exec::load(&file.data, &mut vmm) {
        Ok(img) => img,
        Err(_) => return -8,
    };
    
    // Ensure low memory (0x0 - 0x1000) has page table entries
    // This guards against null pointer derefs and ensures PD[0] exists
    if let Some(phys) = crate::memory::pmm::alloc_page() {
        let r = vmm.map_page_with_flags(
            0x0,
            phys,
            crate::arch::paging::flags::PRESENT |
            crate::arch::paging::flags::USER
        );
        kprintln!("[EXEC] Guard page at 0x0 -> {:#x} result={:?}", phys, r);
    } else {
        kprintln!("[EXEC] WARNING: Failed to allocate guard page");
    }
    
    // Setup user stack — MUST include USER flag so user mode (CPL=3) can access it
    let stack_top = 0x0000_7FFF_FFFF_F000u64;
    for i in 0..16 {
        let stack_page_virt = stack_top - (i + 1) * 4096;
        let phys = crate::memory::pmm::alloc_page().expect("Failed to alloc stack page");
        vmm.map_page_with_flags(stack_page_virt, phys, 
            crate::arch::paging::flags::PRESENT | 
            crate::arch::paging::flags::WRITABLE |
            crate::arch::paging::flags::USER)
            .expect("Failed to map stack page");
        unsafe { core::ptr::write_bytes(crate::memory::vmm::phys_to_virt(phys) as *mut u8, 0, 4096); }
    }

    // Replace current task with user program (entry is now a user-space address)
    let user_entry = image.entry;
    
    crate::scheduler::replace_current_task(user_entry, stack_top, "shell", page_table, 0, stack_top - 8);
}

fn sys_ls(path_ptr: u64, path_len: u64, buf_ptr: u64, buf_len: u64) -> i64 {
    let user_cr3 = {
        let sched = crate::scheduler::SCHEDULER.lock();
        let slot = sched.current;
        sched.tasks[slot].as_ref().map(|t| t.page_table).unwrap_or(0)
    };
    let kernel_cr3 = unsafe { crate::arch::syscall_entry::KERNEL_CR3 };

    let full_path = match resolve_user_path(path_ptr, path_len, user_cr3, kernel_cr3) {
        Ok(p) => p,
        Err(e) => return e,
    };

    match crate::fs::vfs::VFS.lock().read_dir(&full_path) {
        Ok(entries) => {
            let mut result = alloc::string::String::new();
            for entry in entries {
                result.push_str("  ");
                result.push_str(&entry);
            }
            let bytes = result.as_bytes();
            let to_copy = bytes.len().min(buf_len as usize);
            unsafe {
                crate::arch::paging::write_cr3(user_cr3);
                let buf = core::slice::from_raw_parts_mut(buf_ptr as *mut u8, to_copy);
                buf.copy_from_slice(&bytes[..to_copy]);
                crate::arch::paging::write_cr3(kernel_cr3);
            }
            to_copy as i64
        }
        Err(_) => -1,
    }
}

fn sys_create(path_ptr: u64, path_len: u64) -> i64 {
    let user_cr3 = {
        let sched = crate::scheduler::SCHEDULER.lock();
        let slot = sched.current;
        sched.tasks[slot].as_ref().map(|t| t.page_table).unwrap_or(0)
    };
    let kernel_cr3 = unsafe { crate::arch::syscall_entry::KERNEL_CR3 };

    let path = match resolve_user_path(path_ptr, path_len, user_cr3, kernel_cr3) {
        Ok(p) => p,
        Err(e) => return e,
    };
    match crate::fs::vfs::VFS.lock().open(&path) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

fn sys_mkdir(path_ptr: u64, path_len: u64) -> i64 {
    let user_cr3 = {
        let sched = crate::scheduler::SCHEDULER.lock();
        let slot = sched.current;
        sched.tasks[slot].as_ref().map(|t| t.page_table).unwrap_or(0)
    };
    let kernel_cr3 = unsafe { crate::arch::syscall_entry::KERNEL_CR3 };

    let path = match resolve_user_path(path_ptr, path_len, user_cr3, kernel_cr3) {
        Ok(p) => p,
        Err(e) => return e,
    };
    match crate::fs::vfs::VFS.lock().mkdir(&path) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

fn sys_fb_info(buf_ptr: u64) -> i64 {
    if buf_ptr == 0 { return -22; }
    let (fb_phys, fb_size) = match crate::drivers::framebuffer::get_phys_info() {
        Some(i) => i,
        None => return -19,
    };
    if fb_size == 0 { return -19; }

    let user_addr = 0x0000_5000_0000_0000u64;
    let cr3 = crate::arch::paging::read_cr3() & !0xFFF;
    let mut vmm = crate::memory::vmm::VirtualMemoryManager::new(cr3);

    for offset in (0..fb_size).step_by(4096) {
        let flags = crate::arch::paging::flags::PRESENT
            | crate::arch::paging::flags::WRITABLE
            | crate::arch::paging::flags::USER;
        if let Err(e) = vmm.map_page_with_flags(user_addr + offset, fb_phys + offset, flags) {
            kprintln!("[SYS_FB_INFO] map_page failed: {}", e);
            return -12;
        }
    }

    let w = crate::drivers::framebuffer::width();
    let h = crate::drivers::framebuffer::height();
    let p = crate::drivers::framebuffer::pitch();
    unsafe {
        let u = buf_ptr as *mut u64;
        *u = user_addr;
        let ww = (buf_ptr + 8) as *mut u32;
        *ww = w;
        let hh = (buf_ptr + 12) as *mut u32;
        *hh = h;
        let pp = (buf_ptr + 16) as *mut u32;
        *pp = p;
    }
    0
}

fn sys_delete(path_ptr: u64, path_len: u64) -> i64 {
    let user_cr3 = {
        let sched = crate::scheduler::SCHEDULER.lock();
        let slot = sched.current;
        sched.tasks[slot].as_ref().map(|t| t.page_table).unwrap_or(0)
    };
    let kernel_cr3 = unsafe { crate::arch::syscall_entry::KERNEL_CR3 };

    let path = match resolve_user_path(path_ptr, path_len, user_cr3, kernel_cr3) {
        Ok(p) => p,
        Err(e) => return e,
    };
    match crate::fs::vfs::VFS.lock().remove(&path) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

fn sys_clipboard_copy(data_ptr: u64, data_len: u64) -> i64 {
    if data_ptr == 0 || data_len == 0 { return -22; }
    let user_cr3 = {
        let sched = crate::scheduler::SCHEDULER.lock();
        let slot = sched.current;
        sched.tasks[slot].as_ref().map(|t| t.page_table).unwrap_or(0)
    };
    let kernel_cr3 = unsafe { crate::arch::syscall_entry::KERNEL_CR3 };

    let mut clipboard = CLIPBOARD_BUFFER.lock();
    clipboard.clear();
    unsafe {
        crate::arch::paging::write_cr3(user_cr3);
        let src = core::slice::from_raw_parts(data_ptr as *const u8, data_len as usize);
        clipboard.extend_from_slice(src);
        crate::arch::paging::write_cr3(kernel_cr3);
    }
    data_len as i64
}

fn sys_clipboard_paste(buf_ptr: u64, buf_len: u64) -> i64 {
    let user_cr3 = {
        let sched = crate::scheduler::SCHEDULER.lock();
        let slot = sched.current;
        sched.tasks[slot].as_ref().map(|t| t.page_table).unwrap_or(0)
    };
    let kernel_cr3 = unsafe { crate::arch::syscall_entry::KERNEL_CR3 };

    let clipboard = CLIPBOARD_BUFFER.lock();
    if clipboard.is_empty() { return 0; }
    let to_copy = (buf_len as usize).min(clipboard.len());
    unsafe {
        crate::arch::paging::write_cr3(user_cr3);
        let dest = core::slice::from_raw_parts_mut(buf_ptr as *mut u8, to_copy);
        dest.copy_from_slice(&clipboard[..to_copy]);
        crate::arch::paging::write_cr3(kernel_cr3);
    }
    clipboard.len() as i64
}

fn sys_get_mouse(buf_ptr: u64) -> i64 {
    if buf_ptr == 0 { return -22; }
    let user_cr3 = {
        let sched = crate::scheduler::SCHEDULER.lock();
        let slot = sched.current;
        sched.tasks[slot].as_ref().map(|t| t.page_table).unwrap_or(0)
    };
    let kernel_cr3 = unsafe { crate::arch::syscall_entry::KERNEL_CR3 };

    let state = crate::drivers::mouse::MOUSE_STATE.lock();
    unsafe {
        crate::arch::paging::write_cr3(user_cr3);
        let buf = core::slice::from_raw_parts_mut(buf_ptr as *mut i32, 5);
        buf[0] = state.x;
        buf[1] = state.y;
        buf[2] = state.left_button as i32;
        buf[3] = state.right_button as i32;
        buf[4] = state.middle_button as i32;
        crate::arch::paging::write_cr3(kernel_cr3);
    }
    0
}

fn sys_ps(buf_ptr: u64, buf_len: u64) -> i64 {
    let user_cr3 = {
        let sched = crate::scheduler::SCHEDULER.lock();
        let slot = sched.current;
        sched.tasks[slot].as_ref().map(|t| t.page_table).unwrap_or(0)
    };
    let kernel_cr3 = unsafe { crate::arch::syscall_entry::KERNEL_CR3 };

    let mut result = alloc::string::String::new();
    result.push_str("  PID  NAME\n");
    let sched = crate::scheduler::SCHEDULER.lock();
    for task in sched.tasks.iter().flatten() {
        result.push_str("  ");
        result.push_str(&task.id.to_string());
        result.push_str("  ");
        result.push_str(task.name_str());
        result.push('\n');
    }
    let bytes = result.as_bytes();
    let to_copy = bytes.len().min(buf_len as usize);
    unsafe {
        crate::arch::paging::write_cr3(user_cr3);
        let buf = core::slice::from_raw_parts_mut(buf_ptr as *mut u8, to_copy);
        buf.copy_from_slice(&bytes[..to_copy]);
        crate::arch::paging::write_cr3(kernel_cr3);
    }
    to_copy as i64
}

fn sys_move(_src_ptr: u64, _src_len: u64, _dst_ptr: u64, _dst_len: u64) -> i64 { 0 }
fn sys_copy(_src_ptr: u64, _src_len: u64, _dst_ptr: u64, _dst_len: u64) -> i64 { 0 }
fn sys_snapshot(_msg_ptr: u64, _msg_len: u64) -> i64 { 1 }
fn sys_restore(_id: u64) -> i64 { 0 }
fn sys_tag(_id: u64, _tag_ptr: u64, _tag_len: u64) -> i64 { 0 }
fn sys_bond_wifi(_iface_ptr: u64, _iface_len: u64) -> i64 { 0 }
fn sys_share(_file_ptr: u64, _file_len: u64, _user: u64) -> i64 { 0 }
fn sys_time_travel(_seconds: u64) -> i64 { 0 }

fn sys_set_latency_class(class: u64) -> i64 {
    use crate::scheduler::task::LatencyClass;
    let lc = match class {
        0 => LatencyClass::Realtime,
        1 => LatencyClass::Interactive,
        2 => LatencyClass::Normal,
        3 => LatencyClass::Batch,
        4 => LatencyClass::PowerSave,
        _ => return -1,
    };
    let mut sched = crate::scheduler::SCHEDULER.lock();
    let slot = sched.current;
    if let Some(ref mut task) = sched.tasks[slot] {
        task.latency_class = lc;
        0
    } else { -1 }
}

fn sys_pressure() -> i64 {
    crate::pressure::get_raw() as i64
}

fn sys_system_metrics(out_ptr: u64) -> i64 {
    let (user_cr3, kernel_cr3) = {
        let sched = crate::scheduler::SCHEDULER.lock();
        let slot = sched.current;
        let ucr3 = sched.tasks[slot].as_ref().map(|t| t.page_table).unwrap_or(0);
        let kcr3 = unsafe { crate::arch::syscall_entry::KERNEL_CR3 };
        (ucr3, kcr3)
    };
    let metrics = [
        crate::pressure::get_raw() as u64,        // [0]: pressure
        crate::pressure::current_phase() as u64,   // [1]: phase
        crate::pressure::recommended_batch_size() as u64, // [2]: batch size
        crate::pressure::batching_disabled() as u64, // [3]: batching disabled
    ];
    unsafe {
        if user_cr3 != kernel_cr3 { crate::arch::paging::write_cr3(user_cr3); }
        core::ptr::copy_nonoverlapping(metrics.as_ptr(), out_ptr as *mut u64, 4);
        if user_cr3 != kernel_cr3 { crate::arch::paging::write_cr3(kernel_cr3); }
    }
    0
}

use crate::io_uring::IoUring;
use spin::Mutex;

const MAX_IO_URINGS: usize = 64;
static IO_URINGS: Mutex<alloc::vec::Vec<Option<IoUring>>> = Mutex::new(alloc::vec::Vec::new());

fn sys_io_uring_setup(entries: u64, flags: u64) -> i64 {
    let page_table = {
        let sched = crate::scheduler::SCHEDULER.lock();
        let slot = sched.current;
        sched.tasks[slot].as_ref().map(|t| t.page_table).unwrap_or(0)
    };
    let ring = IoUring::new(entries as u32, page_table, flags);
    if ring.is_none() { return -1; }
    let ring = ring.unwrap();
    let user_addr = 0x6000_0000_0000u64;
    if !ring.map_to_user(user_addr, page_table) { return -1; }
    let mut table = IO_URINGS.lock();
    let slot = table.iter_mut().position(|r| r.is_none());
    match slot {
        Some(idx) => { table[idx] = Some(ring); idx as i64 }
        None => {
            let idx = table.len();
            table.push(Some(ring));
            idx as i64
        }
    }
}

fn sys_io_uring_enter(fd: u64, to_submit: u64, min_complete: u64) -> i64 {
    let mut table = IO_URINGS.lock();
    let idx = fd as usize;
    if idx >= table.len() || table[idx].is_none() { return -1; }
    let mut submitted = 0u32;
    if to_submit > 0 {
        if let Some(ref mut ring) = table[idx] {
            submitted = ring.submit_sqes();
        }
    }
    if submitted == 0 && min_complete > 0 {
        if let Some(ref mut ring) = table[idx] {
            // Adaptive wait: 20µs for NORMAL tasks
            ring.adaptive_wait(20);
            // After wait, try submitting again
            submitted = ring.submit_sqes();
        }
    }
    submitted as i64
}

fn sys_io_uring_register(fd: u64, opcode: u64, arg: u64) -> i64 {
    let mut table = IO_URINGS.lock();
    let idx = fd as usize;
    if idx >= table.len() || table[idx].is_none() { return -1; }
    match opcode {
        0 => {
            let ring = table[idx].as_mut().unwrap();
            let (user_cr3, kernel_cr3) = {
                let sched = crate::scheduler::SCHEDULER.lock();
                let slot = sched.current;
                let ucr3 = sched.tasks[slot].as_ref().map(|t| t.page_table).unwrap_or(0);
                let kcr3 = unsafe { crate::arch::syscall_entry::KERNEL_CR3 };
                (ucr3, kcr3)
            };
            unsafe {
                if user_cr3 != kernel_cr3 {
                    crate::arch::paging::write_cr3(user_cr3);
                }
                let reg = &*(arg as *const [u64; 3]);
                let buf_addr = (*reg)[0];
                let buf_size = (*reg)[1];
                let buf_id = (*reg)[2] as u16;
                if user_cr3 != kernel_cr3 {
                    crate::arch::paging::write_cr3(kernel_cr3);
                }
                if ring.register_buffer(buf_id, buf_addr, buf_size) { 0 } else { -1 }
            }
        }
        1 => {
            let ring = table[idx].as_mut().unwrap();
            ring.unregister_buffer(arg as u16);
            0
        }
        2 => {
            let ring = table[idx].as_mut().unwrap();
            let (user_cr3, kernel_cr3) = {
                let sched = crate::scheduler::SCHEDULER.lock();
                let slot = sched.current;
                let ucr3 = sched.tasks[slot].as_ref().map(|t| t.page_table).unwrap_or(0);
                let kcr3 = unsafe { crate::arch::syscall_entry::KERNEL_CR3 };
                (ucr3, kcr3)
            };
            unsafe {
                if user_cr3 != kernel_cr3 { crate::arch::paging::write_cr3(user_cr3); }
                let count = *(arg as *const u32) as usize;
                let ptr = (arg + 8) as *const u64;
                let mut fds: alloc::vec::Vec<Option<u64>> = alloc::vec::Vec::with_capacity(count);
                for i in 0..count {
                    let fd_val = core::ptr::read_volatile(ptr.add(i));
                    fds.push(Some(fd_val));
                }
                if user_cr3 != kernel_cr3 { crate::arch::paging::write_cr3(kernel_cr3); }
                ring.register_fds(fds);
                0
            }
        }
        3 => {
            table[idx].as_mut().unwrap().unregister_fds();
            0
        }
        _ => -1,
    }
}

fn sys_io_uring_stats(fd: u64, out_ptr: u64) -> i64 {
    let table = IO_URINGS.lock();
    let idx = fd as usize;
    if idx >= table.len() || table[idx].is_none() { return -1; }
    let ring = table[idx].as_ref().unwrap();
    let (user_cr3, kernel_cr3) = {
        let sched = crate::scheduler::SCHEDULER.lock();
        let slot = sched.current;
        let ucr3 = sched.tasks[slot].as_ref().map(|t| t.page_table).unwrap_or(0);
        let kcr3 = unsafe { crate::arch::syscall_entry::KERNEL_CR3 };
        (ucr3, kcr3)
    };
    let stats = [ring.counters.submitted, ring.counters.completed,
                 ring.counters.zero_copy, ring.counters.chains,
                 ring.counters.fixed_file_hits];
    unsafe {
        if user_cr3 != kernel_cr3 {
            crate::arch::paging::write_cr3(user_cr3);
        }
        core::ptr::copy_nonoverlapping(stats.as_ptr(), out_ptr as *mut u64, 5);
        if user_cr3 != kernel_cr3 {
            crate::arch::paging::write_cr3(kernel_cr3);
        }
    }
    0
}

pub fn init() {
    crate::arch::syscall_entry::init();
}
