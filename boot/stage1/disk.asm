; ============================================================================
; Beast OS - Disk Loading (Stage 1)
; ============================================================================
; Loads the second-stage bootloader from disk using BIOS INT 13h.

section .text

STAGE2_LOAD_ADDR equ 0x10000    ; Load address for stage 2
STAGE2_SECTORS   equ 32         ; Number of sectors to load

load_stage2:
    pusha

    mov si, msg_loading
    call print_string

    ; Reset disk controller
    xor ah, ah
    mov dl, [boot_drive]
    int 0x13
    jc .disk_error

    ; Read sectors using INT 13h (CHS addressing)
    mov ah, 0x02             ; BIOS read sectors
    mov al, STAGE2_SECTORS   ; Number of sectors
    mov ch, 0                ; Cylinder 0
    mov cl, 2                ; Start from sector 2 (sector 1 = boot sector)
    mov dh, 0                ; Head 0
    mov dl, [boot_drive]     ; Boot drive

    ; Set buffer address ES:BX
    push es
    mov bx, STAGE2_LOAD_ADDR >> 4
    mov es, bx
    xor bx, bx
    int 0x13
    pop es

    jc .disk_error

    ; Verify sectors read
    cmp al, STAGE2_SECTORS
    jne .disk_error

    mov si, msg_loaded
    call print_string

    popa
    ret

.disk_error:
    mov si, msg_disk_error
    call print_string
    cli
    hlt

; Messages
msg_loading:    db "Loading stage 2...", 13, 10, 0
msg_loaded:     db "Stage 2 loaded OK", 13, 10, 0
msg_disk_error: db "DISK ERROR!", 13, 10, 0
