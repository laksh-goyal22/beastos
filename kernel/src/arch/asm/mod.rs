//! Architecture-specific assembly stubs

pub mod interrupt_stubs;

use core::arch::asm;
use core::arch::global_asm;  // Add this import

pub const MSR_GS_BASE: u32 = 0xC0000100;
pub const MSR_KERNEL_GS_BASE: u32 = 0xC0000101;
pub const MSR_STAR: u32 = 0xC0000081;
pub const MSR_LSTAR: u32 = 0xC0000082;
pub const MSR_FMASK: u32 = 0xC0000084;
pub const MSR_EFER: u32 = 0xC0000080;

// Context switch function
global_asm!(r#"
.section .text
.global context_switch
.type context_switch, @function
context_switch:
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15
    mov [rdi], rsp
    mov rsp, [rsi]
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret
"#);

extern "C" {
    pub fn context_switch(old_rsp: *mut u64, new_rsp: *const u64);
    pub fn task_start_trampoline();
    pub fn user_entry_trampoline();
}

#[inline]
pub unsafe fn wrmsr(msr: u32, val: u64) {
    asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") val as u32,
        in("edx") (val >> 32) as u32,
        options(nostack, preserves_flags)
    );
}

#[inline]
pub unsafe fn rdmsr(msr: u32) -> u64 {
    let (lo, hi): (u32, u32);
    asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") lo,
        out("edx") hi,
        options(nostack, preserves_flags)
    );
    (hi as u64) << 32 | lo as u64
}