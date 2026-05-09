//! Ring 3 (User Mode) transitions.

use crate::arch::gdt;

/// Enter user mode by setting up an IRETQ frame and jumping to Ring 3.
///
/// # Safety
/// This is extremely unsafe. It switches the CPU to a lower privilege level.
/// The `entry` and `stack` must be valid virtual addresses in the user portion
/// of the address space (lower half).
pub unsafe fn enter_user_mode(entry: u64, stack: u64) -> ! {
    let cs = gdt::USER_CODE_SELECTOR | 3; // RPL 3
    let ss = gdt::USER_DATA_SELECTOR | 3; // RPL 3

    // IRETQ frame on the kernel stack:
    // SS
    // RSP
    // RFLAGS (bit 9 is IF - enable interrupts)
    // CS
    // RIP
    core::arch::asm!(
        "cli",                  // Disable interrupts before IRETQ
        "push {ss:r}",            // SS
        "push {stack:r}",         // RSP
        "push 0x202",           // RFLAGS (IF=1, Reserved=1)
        "push {cs:r}",            // CS
        "push {entry:r}",         // RIP
        "iretq",
        ss = in(reg) ss,
        stack = in(reg) stack,
        cs = in(reg) cs,
        entry = in(reg) entry,
        options(noreturn)
    );
}
