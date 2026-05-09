//! Beast OS System Call Interface
//!
//! Handles transitions from Ring 3 (user) to Ring 0 (kernel) via the
//! `SYSCALL` instruction.

use crate::kprintln;

/// Syscall numbers (following Linux-ish convention for demo).
pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_EXIT: u64 = 60;
pub const SYS_GETPID: u64 = 39;
pub const SYS_PRINT_VAL: u64 = 100;

/// Main entry point for syscalls after assembly stub has saved registers.
/// 
/// System V AMD64 arguments:
/// rdi, rsi, rdx, rcx (from r10), r8, r9
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
        SYS_WRITE => sys_write(a1, a2, a3),
        SYS_GETPID => sys_getpid(),
        SYS_EXIT => sys_exit(a1),
        SYS_PRINT_VAL => {
            kprintln!("[SYSCALL] User says: 0x{:x}", a1);
            0
        }
        _ => {
            kprintln!("[SYSCALL] Unknown syscall: {}", num);
            -1
        }
    }
}

/// write(fd, buf, count)
fn sys_write(fd: u64, buf_ptr: u64, count: u64) -> i64 {
    if fd != 1 && fd != 2 {
        return -9; // EBADF
    }

    // Safety: In a real OS, we must validate that [buf_ptr, buf_ptr + count)
    // is valid user memory and mapped with USER bit.
    // For now, we trust the user app in this demo.
    let slice = unsafe {
        core::slice::from_raw_parts(buf_ptr as *const u8, count as usize)
    };

    if let Ok(s) = core::str::from_utf8(slice) {
        crate::kprint!("{}", s);
        count as i64
    } else {
        -22 // EINVAL
    }
}

fn sys_getpid() -> i64 {
    crate::scheduler::current_slot() as i64
}

fn sys_exit(code: u64) -> ! {
    kprintln!("[SYSCALL] Task exited with code: {}", code);
    crate::scheduler::exit_current();
}

/// Initialize syscall support (MSRs).
pub fn init() {
    // We already do some init in arch/mod.rs calling syscall_entry::init
    // But we might want to consolidate or ensure consistency here.
}
