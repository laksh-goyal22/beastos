/* ==========================================================================
 * Beast OS - Kernel Linker Script
 * ==========================================================================
 * Higher-half kernel for Limine bootloader.
 *
 * PHDRS enforce strict W^X page permissions.
 * ALIGN(4K) *inside* each section pads the file so that the next segment
 * starts on a fresh page — preventing file-offset overlaps.
 *
 * Requires linker flag: -z max-page-size=0x1000
 * ==========================================================================
 */

OUTPUT_FORMAT("elf64-x86-64")
OUTPUT_ARCH(i386:x86-64)
ENTRY(kmain)

PHDRS
{
    text     PT_LOAD FLAGS(5);   /* R-X: code                     */
    rodata   PT_LOAD FLAGS(4);   /* R--: read-only data            */
    requests PT_LOAD FLAGS(6);   /* RW-: Limine boot requests      */
    data     PT_LOAD FLAGS(6);   /* RW-: writable data + GOT + BSS */
}

SECTIONS
{
    /* Limine maps the kernel at this virtual address */
    . = 0xFFFFFFFF80100000;

    /* ── Code ─────────────────────────────────────────────────────────── */
    .text : {
        *(.text .text.*)
        . = ALIGN(4K);
    } :text

    /* ── Read-Only Data ───────────────────────────────────────────────── */
    .rodata : {
        *(.rodata .rodata.*)
        . = ALIGN(4K);
    } :rodata

    /* ── Limine Boot Requests (MUST be writable — bootloader writes back) ── */
    .requests : {
        KEEP(*(.requests_start_marker))
        KEEP(*(.requests .requests.*))
        KEEP(*(.requests_end_marker))
        . = ALIGN(4K);
    } :requests

    /* ── Writable Data (includes GOT to avoid .text collision) ────── */
    .data : {
        *(.data .data.*)
        *(.got .got.*)
        . = ALIGN(4K);
    } :data

    /* ── Zero-Init Data ───────────────────────────────────────────────── */
    .bss (NOLOAD) : {
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(4K);
    } :data

    _kernel_end = .;

    /* ── Discard unneeded sections ─────────────────────────────────── */
    /DISCARD/ : {
        *(.eh_frame .eh_frame_hdr)
        *(.note .note.*)
        *(.comment)
        *(.gnu.hash)
        *(.debug_*)
    }
}
