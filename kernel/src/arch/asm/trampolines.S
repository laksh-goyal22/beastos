.section .text
.global task_start_trampoline
.type task_start_trampoline, @function
task_start_trampoline:
    mov rdi, r12
    mov rsi, r13
    sti
    jmp r12

.section .text
.global user_entry_trampoline
.type user_entry_trampoline, @function
user_entry_trampoline:
    // NOTE: segment registers are set to user values after user iretq
    // via GDT (0x23 = CS, 0x1B = SS/DS/ES/FS/GS)
    push 0x1b
    push r12
    push 0x202
    push 0x23
    push r13
    mov rdi, r14
    mov rsi, r15
    xor rax, rax
    xor rbx, rbx
    xor rcx, rcx
    xor rdx, rdx
    xor rbp, rbp
    xor r8, r8
    xor r9, r9
    xor r10, r10
    xor r11, r11
    iretq