//! Interrupt Stubs — x86_64 Assembly Entry Points
//!
//! Handles the low-level assembly for interrupts and exceptions:
//! 1. Save all registers
//! 2. swapgs if from user mode
//! 3. Call Rust handler
//! 4. swapgs if returning to user mode
//! 5. Restore all registers
//! 6. iretq

use core::arch::global_asm;

global_asm!(r#"
.macro INTERRUPT_STUB name, handler, has_error_code
.global \name
\name:
    .if \has_error_code == 0
        push 0      // Push dummy error code
    .endif
    
    // Save all registers
    push r15
    push r14
    push r13
    push r12
    push r11
    push r10
    push r9
    push r8
    push rdi
    push rsi
    push rbp
    push rdx
    push rcx
    push rbx
    push rax

    // Check if we came from user mode (CS index 4 is 0x23)
    // The interrupt frame starts at [rsp + 15*8 + 8] = [rsp + 128]
    // Order: rax(0), ..., r15(112), error(120), rip(128), cs(136), rflags(144), rsp(152), ss(160)
    mov rax, [rsp + 136]
    and rax, 0x3
    cmp rax, 0
    je 1f
    swapgs
1:
    // Call the Rust handler
    // Handler expected signature: fn(stack_frame: &InterruptStackFrame, error_code: u64)
    // We pass &frame as 1st arg (rdi) and error_code as 2nd arg (rsi)
    lea rdi, [rsp + 8]    // Pointer to registers starting from rax? No, we want standard frame.
    // Actually, let's just pass the whole thing as a struct pointer.
    mov rdi, rsp           // Pass pointer to saved registers + frame
    mov rsi, [rsp + 120]   // Error code
    
    call \handler

    // Check again if we are returning to user mode
    mov rax, [rsp + 136]
    and rax, 0x3
    cmp rax, 0
    je 2f
    swapgs
2:
    // Restore all registers
    pop rax
    pop rbx
    pop rcx
    pop rdx
    pop rbp
    pop rsi
    pop rdi
    pop r8
    pop r9
    pop r10
    pop r11
    pop r12
    pop r13
    pop r14
    pop r15

    add rsp, 8  // Remove error code
    iretq
.endm

// Define stubs for used exceptions
INTERRUPT_STUB stub_page_fault, page_fault_handler_rust, 1
INTERRUPT_STUB stub_timer, timer_handler_rust, 0
INTERRUPT_STUB stub_keyboard, keyboard_handler_rust, 0
INTERRUPT_STUB stub_gp_fault, general_protection_handler_rust, 1
INTERRUPT_STUB stub_invalid_opcode, invalid_opcode_handler_rust, 0
INTERRUPT_STUB stub_double_fault, double_fault_handler_rust, 1
INTERRUPT_STUB stub_div_error, division_error_handler_rust, 0
INTERRUPT_STUB stub_irq_4, serial_handler_rust, 0
INTERRUPT_STUB stub_mouse, mouse_handler_rust, 0
INTERRUPT_STUB stub_reschedule, reschedule_handler_rust, 0
INTERRUPT_STUB stub_irq_16, irq_handler_forwarder_rust, 0

"#);

extern "C" {
    pub fn stub_page_fault();
    pub fn stub_timer();
    pub fn stub_keyboard();
    pub fn stub_gp_fault();
    pub fn stub_invalid_opcode();
    pub fn stub_double_fault();
    pub fn stub_div_error();
    pub fn stub_irq_4();
    pub fn stub_mouse();
    pub fn stub_reschedule();
    pub fn stub_irq_16();
}
