#![no_std]
#![no_main]

extern crate beast_crt;

fn print_str(s: &str) {
    beast_syscall::write(1, s.as_bytes());
}

#[no_mangle]
pub extern "C" fn beast_main(_argc: u64, _argv: u64) -> i64 {
    print_str("KEYBOARD TEST: Type something...\n");

    let mut buf = [0u8; 64];

    loop {
        let len = beast_syscall::read(0, &mut buf);
        if len > 0 {
            print_str("Got: ");
            for i in 0..len as usize {
                let c = buf[i];
                if c == b'\n' {
                    print_str("\\n");
                } else {
                    beast_syscall::write(1, &[c]);
                }
            }
            print_str("\n");
        }
        beast_syscall::yield_now();
    }
}
