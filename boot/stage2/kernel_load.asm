; ============================================================================
; Beast OS - Kernel Binary Loader
; ============================================================================
; Loads the kernel ELF binary from disk into memory.
; Uses extended BIOS INT 13h (LBA addressing) for larger reads.

[BITS 32]

section .text
global load_kernel

KERNEL_LOAD_ADDR equ 0x100000   ; Load kernel at 1MB mark
KERNEL_START_LBA equ 64         ; Kernel starts at LBA 64
KERNEL_SECTORS   equ 256        ; Read 256 sectors (128KB)

load_kernel:
    ; Use protected mode disk I/O or pre-loaded data
    ; In a Limine-based boot, the bootloader handles kernel loading.
    ; This is provided for custom boot path.

    ; For now, assume kernel is loaded by Limine at the address
    ; specified in limine.cfg. This stub validates the ELF header.

    ; Verify ELF magic at kernel load address
    mov esi, KERNEL_LOAD_ADDR
    cmp dword [esi], 0x464C457F  ; "\x7FELF"
    jne .not_elf

    ; Verify 64-bit ELF
    cmp byte [esi + 4], 2        ; EI_CLASS = ELFCLASS64
    jne .not_64bit

    ; Verify x86_64 architecture
    cmp word [esi + 18], 0x3E    ; e_machine = EM_X86_64
    jne .wrong_arch

    ; Kernel validated
    ret

.not_elf:
    mov esi, msg_not_elf
    call print32
    cli
    hlt

.not_64bit:
    mov esi, msg_not_64
    call print32
    cli
    hlt

.wrong_arch:
    mov esi, msg_wrong_arch
    call print32
    cli
    hlt

msg_not_elf:    db "ERROR: Kernel is not a valid ELF binary", 0
msg_not_64:     db "ERROR: Kernel must be 64-bit ELF", 0
msg_wrong_arch: db "ERROR: Kernel must target x86_64", 0
