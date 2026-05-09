//! Context Switch — x86_64 Assembly Stub
//!
//! `context_switch(old_rsp: *mut u64, new_rsp: *const u64)`
//!
//! Saves callee-saved registers onto the **current** stack, stores
//! the stack pointer into `*old_rsp`, loads the new stack pointer
//! from `*new_rsp`, restores callee-saved registers, and `ret`s
//! into the new task.

use core::arch::global_asm;

// ── Assembly Stub ────────────────────────────────────────────────────────
//
// System V AMD64 ABI:
//   rdi = 1st arg = old_rsp  (&mut Context.rsp)
//   rsi = 2nd arg = new_rsp  (&Context.rsp)
//
// We save/restore only callee-saved registers:
//   r15, r14, r13, r12, rbx, rbp
//
// The return address (rip) is implicitly saved/restored by call/ret.

global_asm!(r#"
.global context_switch
.type context_switch, @function
context_switch:
    // Save callee-saved registers onto current stack
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15

    // Save current stack pointer → *old_rsp
    mov [rdi], rsp

    // Load new stack pointer ← *new_rsp
    mov rsp, [rsi]

    // Restore callee-saved registers from new stack
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp

    // ret pops the return address from the NEW stack,
    // jumping into the new task (or resuming where it yielded).
    ret

// Trampoline for newly spawned tasks.
// context_switch `ret`s here on first dispatch.
// Enables interrupts (disabled by reschedule's `cli`),
// then `ret` pops the real entry function from the stack.
.global task_start_trampoline
.type task_start_trampoline, @function
task_start_trampoline:
    sti
    ret
"#);

extern "C" {
    /// Switch CPU context from the currently running task to another.
    pub fn context_switch(old_rsp: *mut u64, new_rsp: *const u64);

    /// Trampoline that enables interrupts before a new task's entry runs.
    pub fn task_start_trampoline();
}
