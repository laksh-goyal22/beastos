; ============================================================================
; Beast OS - Global Descriptor Table (16-bit setup)
; ============================================================================
; Sets up GDT for transition from Real Mode to Protected Mode.

section .text

; GDT entries
gdt_start:

; Null descriptor (required)
gdt_null:
    dq 0x0

; Code segment descriptor (0x08)
gdt_code:
    dw 0xFFFF               ; Limit (bits 0-15)
    dw 0x0000               ; Base (bits 0-15)
    db 0x00                  ; Base (bits 16-23)
    db 10011010b             ; Access: Present, Ring 0, Code, Execute/Read
    db 11001111b             ; Flags: 4KB granularity, 32-bit + Limit (bits 16-19)
    db 0x00                  ; Base (bits 24-31)

; Data segment descriptor (0x10)
gdt_data:
    dw 0xFFFF               ; Limit (bits 0-15)
    dw 0x0000               ; Base (bits 0-15)
    db 0x00                  ; Base (bits 16-23)
    db 10010010b             ; Access: Present, Ring 0, Data, Read/Write
    db 11001111b             ; Flags: 4KB granularity, 32-bit + Limit
    db 0x00                  ; Base (bits 24-31)

gdt_end:

; GDT descriptor (pointer)
gdt_descriptor:
    dw gdt_end - gdt_start - 1   ; Size of GDT - 1
    dd gdt_start                  ; Start address of GDT
