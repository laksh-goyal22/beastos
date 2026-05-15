#![no_std]

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
pub const SYS_BRK: u64 = 45;
pub const SYS_EXIT: u64 = 60;
pub const SYS_WAIT: u64 = 61;
pub const SYS_CHDIR: u64 = 80;
pub const SYS_GETCWD: u64 = 81;
pub const SYS_LS: u64 = 82;
pub const SYS_CLEAR: u64 = 105;
pub const SYS_PS: u64 = 111;
pub const SYS_UPTIME: u64 = 120;
pub const SYS_CLIPBOARD_COPY: u64 = 150;
pub const SYS_CLIPBOARD_PASTE: u64 = 151;
pub const SYS_CLIPBOARD_CLEAR: u64 = 152;
pub const SYS_GET_MOUSE: u64 = 153;
pub const SYS_FB_INFO: u64 = 210;
pub const SYS_MKDIR: u64 = 500;
pub const SYS_SNAPSHOT: u64 = 100;
pub const SYS_RESTORE: u64 = 101;
pub const SYS_TAG: u64 = 103;
pub const SYS_BOND_WIFI: u64 = 104;
pub const SYS_SHARE: u64 = 106;
pub const SYS_TIME_TRAVEL: u64 = 110;
pub const SYS_IO_URING_SETUP: u64 = 300;
pub const SYS_IO_URING_ENTER: u64 = 301;
pub const SYS_IO_URING_REGISTER: u64 = 302;
pub const SYS_IO_URING_STATS: u64 = 303;
pub const SYS_SET_LATENCY_CLASS: u64 = 310;
pub const SYS_PRESSURE: u64 = 311;
pub const SYS_SYSTEM_METRICS: u64 = 312;

#[inline(always)]
pub unsafe fn syscall0(num: u64) -> i64 {
    let ret: i64;
    core::arch::asm!("syscall",
        in("rax") num,
        lateout("rax") ret,
        out("rcx") _, out("r11") _,
        options(nostack, preserves_flags));
    ret
}

#[inline(always)]
pub unsafe fn syscall1(num: u64, arg1: u64) -> i64 {
    let ret: i64;
    core::arch::asm!("syscall",
        in("rax") num, in("rdi") arg1,
        lateout("rax") ret,
        out("rcx") _, out("r11") _,
        options(nostack, preserves_flags));
    ret
}

#[inline(always)]
pub unsafe fn syscall2(num: u64, arg1: u64, arg2: u64) -> i64 {
    let ret: i64;
    core::arch::asm!("syscall",
        in("rax") num, in("rdi") arg1, in("rsi") arg2,
        lateout("rax") ret,
        out("rcx") _, out("r11") _,
        options(nostack, preserves_flags));
    ret
}

#[inline(always)]
pub unsafe fn syscall3(num: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    let ret: i64;
    core::arch::asm!("syscall",
        in("rax") num, in("rdi") arg1, in("rsi") arg2, in("rdx") arg3,
        lateout("rax") ret,
        out("rcx") _, out("r11") _,
        options(nostack, preserves_flags));
    ret
}

#[inline(always)]
pub unsafe fn syscall4(num: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64) -> i64 {
    let ret: i64;
    core::arch::asm!("syscall",
        in("rax") num, in("rdi") arg1, in("rsi") arg2, in("rdx") arg3, in("r10") arg4,
        lateout("rax") ret,
        out("rcx") _, out("r11") _,
        options(nostack, preserves_flags));
    ret
}

#[inline(always)]
pub unsafe fn syscall5(num: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> i64 {
    let ret: i64;
    core::arch::asm!("syscall",
        in("rax") num, in("rdi") arg1, in("rsi") arg2, in("rdx") arg3,
        in("r10") arg4, in("r8") arg5,
        lateout("rax") ret,
        out("rcx") _, out("r11") _,
        options(nostack, preserves_flags));
    ret
}

pub fn read(fd: u64, buf: &mut [u8]) -> i64 {
    unsafe { syscall3(SYS_READ, fd, buf.as_mut_ptr() as u64, buf.len() as u64) }
}

pub fn write(fd: u64, buf: &[u8]) -> i64 {
    unsafe { syscall3(SYS_WRITE, fd, buf.as_ptr() as u64, buf.len() as u64) }
}

pub fn open(path: &str) -> i64 {
    unsafe { syscall2(SYS_OPEN, path.as_ptr() as u64, path.len() as u64) }
}

pub fn close(fd: u64) -> i64 {
    unsafe { syscall1(SYS_CLOSE, fd) }
}

pub fn exit(code: u64) -> ! {
    unsafe { syscall1(SYS_EXIT, code); }
    loop { unsafe { core::arch::asm!("hlt") } }
}

pub fn yield_now() {
    unsafe { syscall0(SYS_YIELD); }
}

pub fn getpid() -> u64 {
    unsafe { syscall0(SYS_GETPID) as u64 }
}

pub fn brk(addr: u64) -> i64 {
    unsafe { syscall1(SYS_BRK, addr) }
}

pub fn exec(path: &str) -> i64 {
    unsafe { syscall3(SYS_EXEC, path.as_ptr() as u64, path.len() as u64, 0) }
}

pub fn chdir(path: &str) -> i64 {
    unsafe { syscall2(SYS_CHDIR, path.as_ptr() as u64, path.len() as u64) }
}

pub fn getcwd(buf: &mut [u8]) -> i64 {
    unsafe { syscall2(SYS_GETCWD, buf.as_mut_ptr() as u64, buf.len() as u64) }
}

pub fn ls(path: &str, buf: &mut [u8]) -> i64 {
    unsafe { syscall4(SYS_LS, path.as_ptr() as u64, path.len() as u64,
                      buf.as_mut_ptr() as u64, buf.len() as u64) }
}

pub fn mkdir(path: &str) -> i64 {
    unsafe { syscall2(SYS_MKDIR, path.as_ptr() as u64, path.len() as u64) }
}

pub fn ps(buf: &mut [u8]) -> i64 {
    unsafe { syscall2(SYS_PS, buf.as_mut_ptr() as u64, buf.len() as u64) }
}

pub fn uptime() -> u64 {
    unsafe { syscall0(SYS_UPTIME) as u64 }
}

pub fn clear_screen() {
    unsafe { syscall0(SYS_CLEAR); }
}

pub fn fb_info(buf: &mut [u64; 5]) -> i64 {
    unsafe { syscall1(SYS_FB_INFO, buf.as_mut_ptr() as u64) }
}

pub fn get_mouse(state: &mut [i32; 5]) -> i64 {
    unsafe { syscall1(SYS_GET_MOUSE, state.as_mut_ptr() as u64) }
}

pub fn clipboard_copy(data: &[u8]) -> i64 {
    unsafe { syscall2(SYS_CLIPBOARD_COPY, data.as_ptr() as u64, data.len() as u64) }
}

pub fn clipboard_paste(buf: &mut [u8]) -> i64 {
    unsafe { syscall2(SYS_CLIPBOARD_PASTE, buf.as_mut_ptr() as u64, buf.len() as u64) }
}

pub fn clipboard_clear() {
    unsafe { syscall0(SYS_CLIPBOARD_CLEAR); }
}

pub fn wait(pid: u64) {
    unsafe { syscall1(SYS_WAIT, pid); }
}

pub fn lseek(fd: u64, offset: u64) -> i64 {
    unsafe { syscall2(SYS_LSEEK, fd, offset) }
}

pub fn create(path: &str) -> i64 {
    unsafe { syscall2(SYS_CREATE, path.as_ptr() as u64, path.len() as u64) }
}

pub fn delete(path: &str) -> i64 {
    unsafe { syscall2(SYS_DELETE, path.as_ptr() as u64, path.len() as u64) }
}

pub fn move_file(src: &str, dst: &str) {
    unsafe { syscall4(SYS_MOVE, src.as_ptr() as u64, src.len() as u64,
                      dst.as_ptr() as u64, dst.len() as u64); }
}

pub fn copy_file(src: &str, dst: &str) {
    unsafe { syscall4(SYS_COPY, src.as_ptr() as u64, src.len() as u64,
                      dst.as_ptr() as u64, dst.len() as u64); }
}

pub fn snapshot(msg: &str) -> u64 {
    unsafe { syscall2(SYS_SNAPSHOT, msg.as_ptr() as u64, msg.len() as u64) as u64 }
}

pub fn restore(id: u64) {
    unsafe { syscall1(SYS_RESTORE, id); }
}

pub fn tag(id: u64, tag_str: &str) {
    unsafe { syscall3(SYS_TAG, id, tag_str.as_ptr() as u64, tag_str.len() as u64); }
}

pub fn bond_wifi(interface: &str) {
    unsafe { syscall2(SYS_BOND_WIFI, interface.as_ptr() as u64, interface.len() as u64); }
}

pub fn share(file: &str, user: u64) {
    unsafe { syscall3(SYS_SHARE, file.as_ptr() as u64, file.len() as u64, user); }
}

pub fn time_travel(seconds: u64) {
    unsafe { syscall1(SYS_TIME_TRAVEL, seconds); }
}

// Driver syscall wrappers
pub fn map_mmio(phys_addr: u64, size: u64) -> i64 {
    unsafe { syscall3(200, phys_addr, size, 0) }
}

pub fn bind_irq(irq: u8, ring_id: usize) -> i64 {
    unsafe { syscall2(201, irq as u64, ring_id as u64) }
}

pub fn create_irq_ring(capacity: usize) -> i64 {
    unsafe { syscall1(202, capacity as u64) }
}

pub fn port_in(port: u16) -> i64 {
    unsafe { syscall1(203, port as u64) }
}

pub fn port_out(port: u16, value: u32, width: u8) -> i64 {
    unsafe { syscall3(204, port as u64, value as u64, width as u64) }
}

pub fn io_uring_setup(entries: u64) -> i64 {
    unsafe { syscall1(SYS_IO_URING_SETUP, entries) }
}

pub fn io_uring_enter(fd: u64, to_submit: u64, min_complete: u64) -> i64 {
    unsafe { syscall3(SYS_IO_URING_ENTER, fd, to_submit, min_complete) }
}

pub fn io_uring_register(fd: u64, opcode: u64, arg: u64) -> i64 {
    unsafe { syscall3(SYS_IO_URING_REGISTER, fd, opcode, arg) }
}

pub fn io_uring_stats(fd: u64, out_buf: &mut [u64; 5]) -> i64 {
    unsafe { syscall2(SYS_IO_URING_STATS, fd, out_buf.as_mut_ptr() as u64) }
}

pub fn set_latency_class(class: u64) -> i64 {
    unsafe { syscall1(SYS_SET_LATENCY_CLASS, class) }
}

pub fn pressure() -> u64 {
    unsafe { syscall0(SYS_PRESSURE) as u64 }
}

pub fn system_metrics(buf: &mut [u64; 4]) -> i64 {
    unsafe { syscall2(SYS_SYSTEM_METRICS, buf.as_mut_ptr() as u64, 0) }
}
