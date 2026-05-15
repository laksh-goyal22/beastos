#![no_std]
#![no_main]

extern crate beast_crt;
use beast_font::FONT_8X8;

const FB_ADDR: u64 = 0x0000_5000_0000_0000;

struct Framebuffer {
    base: *mut u32, width: u32, height: u32, pitch: u32,
}

impl Framebuffer {
    fn new() -> Option<Self> {
        let mut info = [0u64; 5];
        if beast_syscall::fb_info(&mut info) < 0 { return None; }
        Some(Self { base: info[0] as *mut u32, width: info[1] as u32, height: info[2] as u32, pitch: info[3] as u32 })
    }
    unsafe fn px(&self, x: u32, y: u32) -> &mut u32 {
        &mut *self.base.add((y * self.pitch / 4 + x) as usize)
    }
    unsafe fn fill(&self, x: u32, y: u32, w: u32, h: u32, c: u32) {
        for dy in 0..h { for dx in 0..w { *self.px(x+dx, y+dy) = c; } }
    }
    unsafe fn blend(&self, x: u32, y: u32, c: u32) {
        let bg = *self.px(x, y); let a = (c>>24) as u32;
        let ro = ((c>>16&0xFF)*a + (bg>>16&0xFF)*(255-a))/255;
        let go = ((c>>8&0xFF)*a + (bg>>8&0xFF)*(255-a))/255;
        let bo = ((c&0xFF)*a + (bg&0xFF)*(255-a))/255;
        *self.px(x, y) = (255<<24)|(ro<<16)|(go<<8)|bo;
    }
    unsafe fn rrect(&self, x: u32, y: u32, w: u32, h: u32, r: u32, c: u32) {
        for dy in 0..h { for dx in 0..w {
            let in_c = (dx<r&&dy<r)||(dx>=w-r&&dy<r)||(dx<r&&dy>=h-r)||(dx>=w-r&&dy>=h-r);
            if in_c {
                let cx = if dx<r {dx as i32} else {(w-1-dx) as i32};
                let cy = if dy<r {dy as i32} else {(h-1-dy) as i32};
                if cx*cx+cy*cy > (r*r) as i32 { continue; }
            }
            self.blend(x+dx, y+dy, c);
        }}
    }
    unsafe fn text(&self, x: u32, y: u32, s: &[u8], c: u32) {
        let mut cx = x; let mut cy = y;
        for &ch in s {
            if ch == b'\n' { cx = x; cy += 10; continue; }
            if ch < 0x20 || ch > 0x7E { cx += 7; continue; }
            let glyph = FONT_8X8[(ch - 0x20) as usize];
            for dy in 0..8 { for dx in 0..8 {
                if (glyph[dy] >> (7-dx)) & 1 != 0 {
                    if cx+dx < self.width && cy+dy < self.height {
                        self.blend(cx+dx, cy+dy, c);
                    }
                }
            }}
            cx += 9;
        }
    }
}

#[no_mangle]
pub extern "C" fn beast_main(_argc: u64, _argv: u64) -> i64 {
    let fb = match Framebuffer::new() { Some(f) => f, None => return 1 };
    let (w, h) = (fb.width, fb.height);

    loop {
        // Gradient background
        for y in 0..h { for x in 0..w {
            let r = (x*128/w) as u8; let g = (y*200/h) as u8; let b = ((x+y)*80/(w+h)) as u8;
            unsafe { *fb.px(x, y) = (255<<24)|((r as u32)<<16)|((g as u32)<<8)|b as u32; }
        }}

        // Translucent taskbar
        let tb = 48;
        for y in (h-tb)..h { for x in 0..w {
            let bg = unsafe { *fb.px(x, y) }; let a = 200u32;
            let r = (30*a + ((bg>>16)&0xFF)*(255-a))/255;
            let g = (30*a + ((bg>>8)&0xFF)*(255-a))/255;
            let b = (40*a + (bg&0xFF)*(255-a))/255;
            unsafe { *fb.px(x, y) = (255<<24)|(r<<16)|(g<<8)|b; }
        }}

        // Main window
        let wx = w/6; let wy = h/8; let ww = w*2/3; let wh = h*3/5; let th = 36;
        unsafe { fb.rrect(wx+4, wy+4, ww, wh, 16, 0x40000000); }
        unsafe { fb.rrect(wx, wy, ww, wh, 16, 0xFFF0F0F0); }
        unsafe { fb.fill(wx, wy, ww, th, 0xFF2D5B8C); }
        unsafe { fb.rrect(wx, wy, ww, th, 16, 0xFF2D5B8C); }
        unsafe { fb.text(wx+12, wy+10, b"Welcome to Beast OS", 0xFFFFFFFF); }
        unsafe { fb.text(wx+12, wy+56, b"Zero-copy I/O, lock-free IPC, O(1) everywhere", 0xFF333333); }
        unsafe { fb.text(wx+12, wy+70, b"Press any key in the terminal to interact", 0xFF555555); }

        // Status area in taskbar
        unsafe { fb.fill(w-130, h-tb+10, 120, 28, 0xFF1A1A2E); }
        unsafe { fb.text(w-125, h-tb+16, b"Beast OS v0.1", 0xFF88AACC); }

        // Throttle
        for _ in 0..60 { unsafe { core::arch::asm!("pause"); } }
    }
}
