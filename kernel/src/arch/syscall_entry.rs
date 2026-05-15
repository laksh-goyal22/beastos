//! Syscall Entry Point (LSTAR MSR)
//!
//! Fast syscall via SYSCALL/SYSRET with swapgs.

use core::arch::naked_asm;

use crate::arch::asm::{wrmsr, rdmsr, MSR_STAR, MSR_LSTAR, MSR_FMASK, MSR_EFER};

/// Scratch space for saving user RSP during syscall
static mut USER_RSP_SCRATCH: u64 = 0;

/// Scratch space for saving user CR3 during syscall  
static mut USER_CR3_SCRATCH: u64 = 0;

/// Global kernel stack pointer (used before per-CPU data is ready)
pub static mut KERNEL_STACK_PTR: u64 = 0;

/// Global kernel CR3 (for switching during syscalls)
pub static mut KERNEL_CR3: u64 = 0;

/// Initialize the kernel stack pointer for syscalls (called once during GDT init)
pub fn init_kernel_stack_ptr(stack_top: u64) {
    unsafe {
        KERNEL_STACK_PTR = stack_top;
    }
}

/// Set the kernel CR3 for syscalls
pub fn set_kernel_cr3(cr3: u64) {
    unsafe {
        KERNEL_CR3 = cr3;
    }
}

/// Get the user CR3 saved during syscall entry
pub fn get_user_cr3() -> u64 {
    unsafe { USER_CR3_SCRATCH }
}

/// Update the kernel stack pointer for the current task (called on context switch)
pub fn set_kernel_stack_ptr(stack_top: u64) {
    unsafe {
        KERNEL_STACK_PTR = stack_top;
    }
}

pub fn init() {
    unsafe {
        // STAR MSR:
        // [47:32] SYSCALL CS  → CS = 0x08 (kernel code), SS = 0x10 (kernel data)
        // [63:48] SYSRET base → CS = base+16, SS = base+8
        // Desired SYSRET: CS = 0x23 (user code, RPL=3), SS = 0x1B (user data, RPL=3)
        // base = CS - 16 = 0x23 - 0x10 = 0x13, SS = 0x13 + 8 = 0x1B ✓
        let star: u64 = (0x0008u64 << 32) | (0x0013u64 << 48);
        wrmsr(MSR_STAR, star);
        wrmsr(MSR_LSTAR, syscall_entry as *const () as u64);
        wrmsr(MSR_FMASK, 0x0600); // Clear IF and other flags
        let efer = rdmsr(MSR_EFER);
        wrmsr(MSR_EFER, efer | 1); // SCE bit
    }
}

#[unsafe(naked)]
pub unsafe extern "C" fn syscall_entry() {
    naked_asm!(
            "swapgs",
            
            // Save user CR3 and switch to kernel page table
            "mov r10, cr3",
            "mov [rip + {ucr3}], r10",
            "mov r10, [rip + {kcr3}]",
            "mov cr3, r10",
            
            // Save user RSP
            "mov [rip + {usp}], rsp",
            
            // Load kernel stack
            "mov rsp, [rip + {kstack}]",
            "test rsp, rsp",
            "jnz 1f",
            "1:",
            
            // Create a pseudo-iretq frame
            "push 0x1b",             // User SS
            "push [rip + {usp}]",    // User RSP
            "push r11",              // User RFLAGS
            "push 0x23",             // User CS
            "push rcx",              // User RIP
            
            // Save ALL registers
            "push r15",
            "push r14",
            "push r13",
            "push r12",
            "push r10",
            "push r9",
            "push r8",
            "push rbp",
            "push rdi",
            "push rsi",
            "push rdx",
            "push rbx",
            
            // Stack alignment
            "push 0",
            
            // Pass arguments from user registers to Rust
            "mov r9, r8",   // a5
            "mov r8, r10",  // a4
            "mov rcx, rdx", // a3
            "mov rdx, rsi", // a2
            "mov rsi, rdi", // a1
            "mov rdi, rax", // num

            "call {dispatch}",
            
            "add rsp, 8",    // Remove alignment
            
            // Restore ALL registers
            "pop rbx",
            "pop rdx",
            "pop rsi",
            "pop rdi",
            "pop rbp",
            "pop r8",
            "pop r9",
            "pop r10",
            "pop r12",
            "pop r13",
            "pop r14",
            "pop r15",
            
            // Restore from pseudo-iretq frame
            "pop rcx",       // User RIP
            "add rsp, 8",    // Skip CS
            "pop r11",       // User RFLAGS
            
            // Switch back to user page table
            "mov r10, [rip + {ucr3}]",
            "mov cr3, r10",
            
            // Load user RSP
            "mov rsp, [rsp]",
            
            "swapgs",
            "sysretq",
            dispatch = sym crate::syscall::syscall_dispatch,
            usp = sym USER_RSP_SCRATCH,
            ucr3 = sym USER_CR3_SCRATCH,
            kstack = sym KERNEL_STACK_PTR,
            kcr3 = sym KERNEL_CR3,
        );
}
