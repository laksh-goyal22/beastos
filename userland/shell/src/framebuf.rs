static mut FB_ADDR: u64 = 0;
static mut FB_WIDTH: u32 = 0;
static mut FB_HEIGHT: u32 = 0;
static mut FB_PITCH: u32 = 0;

pub fn init() -> bool {
    let mut info = [0u64; 5];
    if beast_syscall::fb_info(&mut info) >= 0 {
        let addr = info[0];
        let w = info[1] as u32;
        let h = info[2] as u32;
        if addr == 0 || w == 0 || h == 0 { return false; }
        unsafe {
            FB_ADDR = addr;
            FB_WIDTH = w;
            FB_HEIGHT = h;
            FB_PITCH = info[3] as u32;
        }
        true
    } else {
        false
    }
}

pub fn width() -> u32 {
    unsafe { FB_WIDTH }
}

pub fn height() -> u32 {
    unsafe { FB_HEIGHT }
}

pub fn pitch() -> u32 {
    unsafe { FB_PITCH }
}

pub fn put_pixel(x: u32, y: u32, color: u32) {
    unsafe {
        if x < FB_WIDTH && y < FB_HEIGHT {
            let offset = (y * (FB_PITCH / 4) + x) as isize;
            let ptr = FB_ADDR as *mut u32;
            ptr.offset(offset).write_volatile(color);
        }
    }
}

pub fn fill_rect(x: u32, y: u32, w: u32, h: u32, color: u32) {
    for dy in 0..h {
        for dx in 0..w {
            put_pixel(x + dx, y + dy, color);
        }
    }
}

pub fn clear(color: u32) {
    unsafe {
        let total = (FB_HEIGHT * (FB_PITCH / 4)) as usize;
        let ptr = FB_ADDR as *mut u32;
        for i in 0..total {
            ptr.add(i).write_volatile(color);
        }
    }
}

pub fn draw_char(x: u32, y: u32, c: char, fg: u32, bg: u32, scale: u32) {
    if let Some(bitmap) = beast_font::get_char_bitmap(c) {
        for (dy, &row) in bitmap.iter().enumerate() {
            for sy in 0..scale {
                let sy_actual = y + (dy as u32 * scale) + sy;
                if sy_actual >= unsafe { FB_HEIGHT } { continue; }
                for dx in 0..8 {
                    let color = if (row & (0x80 >> dx)) != 0 { fg } else { bg };
                    for sx in 0..scale {
                        let sx_actual = x + (dx as u32 * scale) + sx;
                        if sx_actual >= unsafe { FB_WIDTH } { break; }
                        put_pixel(sx_actual, sy_actual, color);
                    }
                }
            }
        }
    } else {
        fill_rect(x, y, 8 * scale, 8 * scale, fg);
    }
}

pub fn draw_str(mut x: u32, y: u32, s: &str, fg: u32, bg: u32, scale: u32) {
    let char_w = 8 * scale;
    for c in s.chars() {
        if c == '\n' {
            return;
        }
        draw_char(x, y, c, fg, bg, scale);
        x += char_w;
    }
}
