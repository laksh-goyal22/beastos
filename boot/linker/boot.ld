/* ==========================================================================
 * Beast OS - Bootloader Linker Script
 * ==========================================================================
 * Links the first and second stage bootloader assembly. */

OUTPUT_FORMAT("elf64-x86-64")
OUTPUT_ARCH(i386:x86-64)

SECTIONS
{
    . = 0x7C00;

    .text : {
        *(.text)
    }

    .rodata : {
        *(.rodata)
    }

    .data : {
        *(.data)
    }

    .bss : {
        *(.bss)
    }
}
