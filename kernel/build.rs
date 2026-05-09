/// Beast OS Kernel - Build Script
///
/// Assembles architecture-specific assembly stubs and
/// passes linker configuration to the compiler.

fn main() {
    // Tell cargo to use our custom linker script
    println!("cargo:rustc-link-arg=-Tboot/linker/kernel.ld");
    println!("cargo:rerun-if-changed=boot/linker/kernel.ld");

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
