; ============================================================================
; Beast OS - CPU Feature Detection (Stage 2)
; ============================================================================
; Verifies the CPU supports required features before entering kernel.

[BITS 32]

section .text
global check_cpu_features

check_cpu_features:
    ; Check CPUID support
    pushfd
    pop eax
    mov ecx, eax
    xor eax, (1 << 21)      ; Flip CPUID bit
    push eax
    popfd
    pushfd
    pop eax
    push ecx
    popfd
    cmp eax, ecx
    je .no_cpuid

    ; Get highest basic CPUID function
    xor eax, eax
    cpuid

    ; Check feature flags (ECX from CPUID.01H)
    mov eax, 1
    cpuid

    ; Verify SSE2 (EDX bit 26)
    test edx, (1 << 26)
    jz .no_sse2

    ; Verify SSE3 (ECX bit 0)
    test ecx, (1 << 0)
    jz .no_sse3

    ; Verify SSSE3 (ECX bit 9)
    test ecx, (1 << 9)
    jz .no_ssse3

    ; Verify SSE4.1 (ECX bit 19) — Critical for Glass Engine
    test ecx, (1 << 19)
    jz .no_sse41

    ; Verify APIC (EDX bit 9)
    test edx, (1 << 9)
    jz .no_apic

    ; Verify MSR support (EDX bit 5)
    test edx, (1 << 5)
    jz .no_msr

    ; All checks passed
    ret

.no_cpuid:
    mov esi, msg_no_cpuid
    jmp .halt
.no_sse2:
    mov esi, msg_no_sse2
    jmp .halt
.no_sse3:
    mov esi, msg_no_sse3
    jmp .halt
.no_ssse3:
    mov esi, msg_no_ssse3
    jmp .halt
.no_sse41:
    mov esi, msg_no_sse41
    jmp .halt
.no_apic:
    mov esi, msg_no_apic
    jmp .halt
.no_msr:
    mov esi, msg_no_msr
    jmp .halt

.halt:
    call print32
    cli
    hlt

section .rodata
msg_no_cpuid:  db "ERROR: CPUID not supported", 0
msg_no_sse2:   db "ERROR: SSE2 required", 0
msg_no_sse3:   db "ERROR: SSE3 required", 0
msg_no_ssse3:  db "ERROR: SSSE3 required", 0
msg_no_sse41:  db "ERROR: SSE4.1 required (Glass Engine)", 0
msg_no_apic:   db "ERROR: APIC required", 0
msg_no_msr:    db "ERROR: MSR support required", 0
