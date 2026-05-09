//! Syscall Entry Point (LSTAR MSR)
//!
//! Fast syscall via SYSCALL/SYSRET with swapgs.

use core::arch::asm;

const MSR_STAR: u32 = 0xC0000081;
const MSR_LSTAR: u32 = 0xC0000082;
const MSR_FMASK: u32 = 0xC0000084;

pub fn init() {
    unsafe {
        let star: u64 = (0x0008u64 << 32) | (0x0018u64 << 48);
        wrmsr(MSR_STAR, star);
        wrmsr(MSR_LSTAR, syscall_entry as u64);
        wrmsr(MSR_FMASK, 0x0600);
        let efer = rdmsr(0xC0000080);
        wrmsr(0xC0000080, efer | 1);
    }
}

#[naked]
unsafe extern "C" fn syscall_entry() {
    asm!(
        "swapgs",
        "mov gs:[0x10], rsp",
        "mov rsp, gs:[0x08]",
        "push rcx",
        "push r11",
        "mov rcx, r10",
        "call {dispatch}",
        "pop r11",
        "pop rcx",
        "mov rsp, gs:[0x10]",
        "swapgs",
        "sysretq",
        dispatch = sym syscall_dispatch,
        options(noreturn)
    );
}

#[no_mangle]
extern "C" fn syscall_dispatch(num: u64, a1: u64, a2: u64, a3: u64, _a4: u64, _a5: u64) -> i64 {
    match num {
        0 => -1,  // sys_read
        1 => -1,  // sys_write
        2 => -1,  // sys_open
        3 => 0,   // sys_close
        39 => 1,  // sys_getpid (V-DSO in prod)
        60 => loop { unsafe { asm!("hlt") } },
        _ => -1,
    }
}

#[inline]
unsafe fn wrmsr(msr: u32, val: u64) {
    asm!("wrmsr", in("ecx") msr, in("eax") val as u32, in("edx") (val >> 32) as u32, options(nostack));
}

#[inline]
unsafe fn rdmsr(msr: u32) -> u64 {
    let (lo, hi): (u32, u32);
    asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nostack));
    (hi as u64) << 32 | lo as u64
}
