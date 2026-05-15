; ============================================================================
; Beast OS - First Stage Bootloader
; ============================================================================
; Real mode → Protected mode transition
; Loaded at 0x7C00 by BIOS
;
; Responsibilities:
;   1. Set up segment registers
;   2. Enable A20 line
;   3. Load GDT
;   4. Load second stage from disk
;   5. Jump to protected mode

[BITS 16]
[ORG 0x7C00]

section .text
global _start

_start:
    ; Disable interrupts during setup
    cli

    ; Set up segment registers
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax

    ; Set up stack
    mov ss, ax
    mov sp, 0x7C00          ; Stack grows downward from boot sector

    ; Re-enable interrupts
    sti

    ; Save boot drive number
    mov [boot_drive], dl

    ; Print boot message
    mov si, msg_booting
    call print_string

    ; Enable A20 line
    call enable_a20

    ; Load second stage from disk
    call load_stage2

    ; Load GDT
    lgdt [gdt_descriptor]

    ; Switch to protected mode
    cli
    mov eax, cr0
    or eax, 1               ; Set PE (Protection Enable) bit
    mov cr0, eax

    ; Far jump to flush pipeline and enter 32-bit mode
    jmp 0x08:protected_mode_entry

; ---------------------------------------------------------------------------
; Print null-terminated string (SI = pointer)
; ---------------------------------------------------------------------------
print_string:
    pusha
.loop:
    lodsb
    cmp al, 0
    je .done
    mov ah, 0x0E
    mov bh, 0
    int 0x10
    jmp .loop
.done:
    popa
    ret

; ---------------------------------------------------------------------------
; Include sub-modules
; ---------------------------------------------------------------------------
%include "boot/stage1/a20.asm"
%include "boot/stage1/gdt.asm"
%include "boot/stage1/disk.asm"

; ---------------------------------------------------------------------------
; Data
; ---------------------------------------------------------------------------
boot_drive:     db 0
msg_booting:    db "Beast OS Booting...", 13, 10, 0

; ---------------------------------------------------------------------------
; Protected Mode Entry (32-bit)
; ---------------------------------------------------------------------------
[BITS 32]
protected_mode_entry:
    ; Set up 32-bit segment registers
    mov ax, 0x10             ; Data segment selector
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    mov esp, 0x90000         ; Protected mode stack

    ; Jump to second stage
    jmp 0x10000              ; Second stage loaded here

; ---------------------------------------------------------------------------
; Boot sector padding and magic number
; ---------------------------------------------------------------------------
times 510 - ($ - $$) db 0
dw 0xAA55                   ; Boot signature
