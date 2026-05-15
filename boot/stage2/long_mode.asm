; ============================================================================
; Beast OS - Long Mode Transition (Protected → 64-bit)
; ============================================================================
; Transitions CPU from 32-bit Protected Mode to 64-bit Long Mode.

[BITS 32]

section .text
global enter_long_mode

enter_long_mode:
    ; Verify CPU supports long mode
    call check_long_mode_support

    ; Set up page tables for identity mapping
    call setup_page_tables

    ; Enable PAE (Physical Address Extension)
    mov eax, cr4
    or eax, (1 << 5)        ; CR4.PAE
    mov cr4, eax

    ; Load PML4 address into CR3
    mov eax, pml4_table
    mov cr3, eax

    ; Enable long mode via EFER MSR
    mov ecx, 0xC0000080      ; IA32_EFER MSR
    rdmsr
    or eax, (1 << 8)         ; Set LME (Long Mode Enable)
    wrmsr

    ; Enable paging
    mov eax, cr0
    or eax, (1 << 31)        ; CR0.PG
    mov cr0, eax

    ; Load 64-bit GDT
    lgdt [gdt64_pointer]

    ; Far jump to 64-bit code segment
    jmp 0x08:long_mode_entry

; ---------------------------------------------------------------------------
; Check if CPU supports long mode
; ---------------------------------------------------------------------------
check_long_mode_support:
    ; Check extended CPUID
    mov eax, 0x80000000
    cpuid
    cmp eax, 0x80000001
    jb .no_long_mode

    ; Check long mode bit
    mov eax, 0x80000001
    cpuid
    test edx, (1 << 29)      ; LM bit
    jz .no_long_mode
    ret

.no_long_mode:
    ; Print error and halt
    mov esi, msg_no_long_mode
    call print32
    cli
    hlt

msg_no_long_mode: db "ERROR: CPU does not support 64-bit long mode!", 0

; ---------------------------------------------------------------------------
; 64-bit GDT
; ---------------------------------------------------------------------------
align 16
gdt64_start:
    dq 0                     ; Null descriptor
gdt64_code:
    dq 0x00AF9A000000FFFF    ; Code: 64-bit, Present, Execute/Read
gdt64_data:
    dq 0x00CF92000000FFFF    ; Data: Present, Read/Write
gdt64_end:

gdt64_pointer:
    dw gdt64_end - gdt64_start - 1
    dq gdt64_start

; ---------------------------------------------------------------------------
; 64-bit Entry Point
; ---------------------------------------------------------------------------
[BITS 64]

long_mode_entry:
    ; Set up 64-bit data segments
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    ; Set up 64-bit stack
    mov rsp, 0x200000        ; 2MB stack pointer

    ; Jump to kernel entry point
    extern kmain
    call kmain

    ; Should never reach here
    cli
    hlt

; Simple 32-bit print (VGA text mode)
print32:
    pusha
    mov edi, 0xB8000
.loop32:
    lodsb
    cmp al, 0
    je .done32
    mov ah, 0x0F             ; White on black
    stosw
    jmp .loop32
.done32:
    popa
    ret
