#![no_std]
#![no_main]

extern crate beast_crt;

const FB_ADDR: u64 = 0x0000_5000_0000_0000;

#[no_mangle]
pub extern "C" fn beast_main(_argc: u64, _argv: u64) -> i64 {
    // Get framebuffer info
    let mut info = [0u64; 5];
    let ret = beast_syscall::fb_info(&mut info);
    if ret < 0 { return 1; }

    let fb_base = info[0] as *mut u32;
    let width = info[1] as u32;
    let height = info[2] as u32;
    let pitch = info[3] as u32;

    // Draw gradient background
    for y in 0..height {
        for x in 0..width {
            let r = (x * 255 / width) as u8;
            let g = (y * 255 / height) as u8;
            let b = 128u8;
            let color = (255u32 << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
            unsafe {
                let px = fb_base.add((y * pitch / 4 + x) as usize);
                *px = color;
            }
        }
    }

    loop { unsafe { core::arch::asm!("pause"); } }
}
