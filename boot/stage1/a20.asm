; ============================================================================
; Beast OS - A20 Line Enable
; ============================================================================
; The A20 gate must be enabled to access memory above 1MB.
; We try multiple methods for maximum compatibility.

section .text

enable_a20:
    ; Method 1: BIOS interrupt
    mov ax, 0x2401
    int 0x15
    jnc .a20_done

    ; Method 2: Keyboard controller
    call .a20_wait_input
    mov al, 0xAD             ; Disable keyboard
    out 0x64, al

    call .a20_wait_input
    mov al, 0xD0             ; Read output port
    out 0x64, al

    call .a20_wait_output
    in al, 0x60
    push eax

    call .a20_wait_input
    mov al, 0xD1             ; Write output port
    out 0x64, al

    call .a20_wait_input
    pop eax
    or al, 2                 ; Set A20 bit
    out 0x60, al

    call .a20_wait_input
    mov al, 0xAE             ; Enable keyboard
    out 0x64, al

    call .a20_wait_input

.a20_done:
    ret

; Wait for keyboard controller input buffer to be empty
.a20_wait_input:
    in al, 0x64
    test al, 2
    jnz .a20_wait_input
    ret

; Wait for keyboard controller output buffer to be full
.a20_wait_output:
    in al, 0x64
    test al, 1
    jz .a20_wait_output
    ret
