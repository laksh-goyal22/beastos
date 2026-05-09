/* ==========================================================================
 * Beast OS - Kernel Linker Script
 * ==========================================================================
 * Places kernel sections at higher-half virtual address.
 * Physical load at 1MB, virtual at 0xFFFFFFFF80000000. */

OUTPUT_FORMAT("elf64-x86-64")
OUTPUT_ARCH(i386:x86-64)
ENTRY(kmain)

KERNEL_PHYS_BASE = 0x100000;
KERNEL_VIRT_BASE = 0xFFFFFFFF80000000;

SECTIONS
{
    . = KERNEL_VIRT_BASE + KERNEL_PHYS_BASE;

    .text ALIGN(4K) : AT(ADDR(.text) - KERNEL_VIRT_BASE)
    {
        KEEP(*(.text.boot))
        *(.text .text.*)
    }

    .rodata ALIGN(4K) : AT(ADDR(.rodata) - KERNEL_VIRT_BASE)
    {
        *(.rodata .rodata.*)
    }

    .data ALIGN(4K) : AT(ADDR(.data) - KERNEL_VIRT_BASE)
    {
        *(.data .data.*)
    }

    .bss ALIGN(4K) : AT(ADDR(.bss) - KERNEL_VIRT_BASE)
    {
        *(COMMON)
        *(.bss .bss.*)
    }

    /* Kernel end marker */
    _kernel_end = .;

    /DISCARD/ : {
        *(.comment)
        *(.note.*)
        *(.eh_frame*)
    }
}
