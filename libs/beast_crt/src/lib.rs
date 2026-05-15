#![no_std]

/// Standard entry point for Beast OS userland programs.
///
/// The kernel's `user_entry_trampoline` jumps here after setting up:
///   rdi = argc  (from the task's r14)
///   rsi = argv  (from the task's r15)
///
/// This `_start` calls the user's `main(argc, argv)` and exits with its return code.
/// Programs that need custom setup can provide their own `_start`.
#[no_mangle]
pub extern "C" fn _start(argc: u64, argv: u64) -> ! {
    extern "C" {
        fn beast_main(argc: u64, argv: u64) -> i64;
    }
    let code = unsafe { beast_main(argc, argv) };
    beast_syscall::exit(code as u64);
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    beast_syscall::write(2, b"\n[CRT PANIC]\n");
    beast_syscall::exit(1)
}
