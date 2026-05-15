; ============================================================================
; Beast OS - Page Table Setup
; ============================================================================
; Identity maps the first 2GB using 2MB pages for initial boot.
; PML4 → PDPT → PD (2MB pages)

[BITS 32]

section .bss
align 4096

; Page table structures (each 4KB aligned)
global pml4_table
pml4_table:  resb 4096
pdpt_table:  resb 4096
pd_table:    resb 4096
pd_table2:   resb 4096       ; Second PD for 1-2GB range

section .text
global setup_page_tables

setup_page_tables:
    ; Clear page tables
    mov edi, pml4_table
    xor eax, eax
    mov ecx, 4096 * 4 / 4   ; 4 tables × 4096 bytes / 4 bytes per stosd
    rep stosd

    ; PML4[0] → PDPT
    mov eax, pdpt_table
    or eax, 0x03             ; Present + Writable
    mov [pml4_table], eax

    ; PDPT[0] → PD (first 1GB)
    mov eax, pd_table
    or eax, 0x03
    mov [pdpt_table], eax

    ; PDPT[1] → PD2 (second 1GB)
    mov eax, pd_table2
    or eax, 0x03
    mov [pdpt_table + 8], eax

    ; Fill PD entries with 2MB pages (identity map first 2GB)
    mov ecx, 512             ; 512 entries × 2MB = 1GB
    mov edi, pd_table
    mov eax, 0x83            ; Present + Writable + 2MB page (PS bit)
.fill_pd1:
    mov [edi], eax
    add eax, 0x200000        ; Next 2MB page
    add edi, 8
    loop .fill_pd1

    mov ecx, 512
    mov edi, pd_table2
    ; eax continues from where it left off (1GB mark)
.fill_pd2:
    mov [edi], eax
    add eax, 0x200000
    add edi, 8
    loop .fill_pd2

    ret
