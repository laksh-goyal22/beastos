//! Ring 3 (User Mode) transitions.

/// Enter user mode by setting up an IRETQ frame and jumping to Ring 3.
/// 
/// Arguments:
/// - rdi: entry point
/// - rsi: user stack
/// - rdx: argc
/// - rcx: argv
#[unsafe(naked)]
pub unsafe extern "C" fn enter_user_mode(entry: u64, stack: u64, argc: u64, argv: u64) -> ! {
    core::arch::naked_asm!(
            "cli",
            
            // Swap GS_USER and GS_KERNEL
            "swapgs",
            
            // Load user segment selectors
            "mov ax, 0x1b",      // User data segment (0x18 | 3)
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "mov gs, ax",
            
            // Setup IRETQ frame
            "push 0x1b",         // SS
            "push rsi",          // RSP
            "push 0x202",        // RFLAGS (IF=1)
            "push 0x23",         // CS (0x20 | 3)
            "push rdi",          // RIP (entry)

            // Pass argc and argv in rdi and rsi to the user app
            "mov rdi, rdx",
            "mov rsi, rcx",
            
            // Clean up other registers
            "xor rax, rax",
            "xor rbx, rbx",
            "xor rcx, rcx", // argv was here, now in rsi
            "xor rdx, rdx", // argc was here, now in rdi
            "xor rbp, rbp",
            "xor r8, r8",
            "xor r9, r9",
            "xor r10, r10",
            "xor r11, r11",
            "xor r12, r12",
            "xor r13, r13",
            "xor r14, r14",
            "xor r15, r15",
            
            "iretq"
        );
}
