//! Syscall Entry Point (LSTAR MSR)
//!
//! Fast syscall via SYSCALL/SYSRET with swapgs.

use core::arch::naked_asm;

use crate::arch::asm::{wrmsr, rdmsr, MSR_STAR, MSR_LSTAR, MSR_FMASK, MSR_EFER};

pub fn init() {
    unsafe {
        let star: u64 = (0x0008u64 << 32) | (0x0018u64 << 48);
        wrmsr(MSR_STAR, star);
        wrmsr(MSR_LSTAR, syscall_entry as *const () as u64);
        wrmsr(MSR_FMASK, 0x0600);
        let efer = rdmsr(MSR_EFER);
        wrmsr(MSR_EFER, efer | 1);
    }
}

extern "C" {
    fn syscall_dispatch(num: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> i64;
}

#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() {
    naked_asm!(
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
    );
}


