/// Beast OS - Root Build Script
///
/// Orchestrates the build process:
/// 1. Assemble boot stages (NASM)
/// 2. Link kernel with custom linker script
/// 3. Generate bootable image

use std::env;
use std::process::Command;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();

    // Assemble boot stage 1
    let asm_files = [
        "boot/stage1/boot.asm",
        "boot/stage1/gdt.asm",
        "boot/stage1/a20.asm",
        "boot/stage1/disk.asm",
        "boot/stage2/long_mode.asm",
        "boot/stage2/paging.asm",
        "boot/stage2/cpu_check.asm",
        "boot/stage2/memory_map.asm",
        "boot/stage2/kernel_load.asm",
    ];

    for asm in &asm_files {
        let obj_name = asm
            .replace('/', "_")
            .replace(".asm", ".o");

        let status = Command::new("nasm")
            .args(["-f", "elf64", asm, "-o"])
            .arg(format!("{}/{}", out_dir, obj_name))
            .status()
            .expect(&format!("Failed to assemble {}", asm));

        if !status.success() {
            panic!("NASM failed for {}", asm);
        }
    }

    // Tell Cargo to re-run if assembly files change
    for asm in &asm_files {
        println!("cargo:rerun-if-changed={}", asm);
    }

    println!("cargo:rerun-if-changed=boot/linker/kernel.ld");
    println!("cargo:rerun-if-changed=build.rs");
}
