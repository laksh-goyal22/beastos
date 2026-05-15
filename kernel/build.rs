/// Beast OS Kernel - Build Script
///
/// Assembles architecture-specific assembly stubs and
/// passes linker configuration to the compiler.

fn main() {
    // Linker script is passed via .cargo/config.toml — do NOT duplicate it here.
    // Passing -T twice causes lld to process PHDRS twice → layout errors.
    println!("cargo:rerun-if-changed=../boot/linker/kernel.ld");

    // Assemble inline asm helpers
    let asm_files = [
        "src/arch/asm/context_switch.s",
        "src/arch/asm/interrupt_stubs.s",
        "src/arch/asm/boot.s",
    ];

    for asm in &asm_files {
        println!("cargo:rerun-if-changed={}", asm);
    }
}
