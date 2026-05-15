#![no_std]
#![no_main]

extern crate beast_crt;

#[no_mangle]
pub extern "C" fn beast_main(_argc: u64, _argv: u64) -> i64 {
    beast_syscall::write(1, b"Hello from Beast OS userspace!\n");
    beast_syscall::write(1, b"User mode works!\n");

    for _ in 0..5 {
        beast_syscall::yield_now();
    }

    beast_syscall::write(1, b"Launching shell...\n");
    beast_syscall::exec("/bin/shell.beast");
    beast_syscall::write(1, b"Exec failed!\n");
    1
}
