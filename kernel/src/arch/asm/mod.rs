//! Architecture-specific assembly stubs
//!
//! Placeholder module for inline assembly and external .s files.

// Context switch, interrupt stubs, and boot assembly are
// handled via global_asm! or external assembly files.

use core::arch::asm;

pub const MSR_GS_BASE: u32 = 0xC0000101;
pub const MSR_KERNEL_GS_BASE: u32 = 0xC0000102;
pub const MSR_STAR: u32 = 0xC0000081;
pub const MSR_LSTAR: u32 = 0xC0000082;
pub const MSR_FMASK: u32 = 0xC0000084;
pub const MSR_EFER: u32 = 0xC0000080;

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
