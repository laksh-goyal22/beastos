; ============================================================================
; Beast OS - Memory Map Query (E820/E801)
; ============================================================================
; Queries BIOS for available RAM regions using INT 15h, AX=E820h.

[BITS 16]

section .text
global query_memory_map

; Memory map entry structure (24 bytes each)
; Offset 0:  Base address (8 bytes)
; Offset 8:  Length (8 bytes)
; Offset 16: Type (4 bytes)
; Offset 20: ACPI extended attributes (4 bytes)

MEMORY_MAP_ADDR equ 0x8000     ; Store memory map entries here
MAX_ENTRIES     equ 64         ; Maximum entries

query_memory_map:
    pusha

    mov di, MEMORY_MAP_ADDR + 4  ; Skip first 4 bytes (entry count)
    xor ebx, ebx                  ; Continuation value (0 = start)
    xor bp, bp                    ; Entry counter
    mov edx, 0x534D4150           ; 'SMAP' magic

.e820_loop:
    mov eax, 0xE820
    mov ecx, 24                   ; Entry size
    int 0x15

    jc .e820_done                 ; Carry = error or end

    cmp eax, 0x534D4150           ; Verify SMAP signature
    jne .e820_error

    ; Check if entry is valid (length > 0)
    mov eax, [di + 8]            ; Low 32 bits of length
    or eax, [di + 12]            ; High 32 bits of length
    jz .e820_skip                ; Skip zero-length entries

    inc bp                       ; Increment entry count
    add di, 24                   ; Move to next entry slot

    ; Check if we've hit max entries
    cmp bp, MAX_ENTRIES
    jge .e820_done

.e820_skip:
    cmp ebx, 0                  ; ebx=0 means last entry
    jne .e820_loop

.e820_done:
    ; Store entry count at start of buffer
    mov [MEMORY_MAP_ADDR], bp
    popa
    ret

.e820_error:
    mov si, msg_e820_fail
    call print_string
    popa
    ret

msg_e820_fail: db "WARNING: E820 memory map failed", 13, 10, 0
