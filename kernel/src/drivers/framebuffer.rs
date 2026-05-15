//! Framebuffer Driver
//!
//! Limine provides a linear framebuffer. This module wraps it for
//! pixel drawing, text rendering, and the future Glass Engine compositor.

use crate::kprintln;
use core::slice;
use core::ptr;
use spin::Mutex;
use limine::framebuffer::Framebuffer as LimineFb;

/// Framebuffer descriptor (populated from bootloader info).
pub struct Framebuffer {
    pub base: *mut u32,
    pub phys_base: u64,
    pub width: u32,
    pub height: u32,
    pub pitch: u32, // bytes per row
}

unsafe impl Send for Framebuffer {}
unsafe impl Sync for Framebuffer {}

/// Global framebuffer (set during init from bootloader).
static FRAMEBUFFER: Mutex<Option<Framebuffer>> = Mutex::new(None);

/// Initialize framebuffer from Limine response.
pub fn init(fb: &LimineFb) {
    let mut global_fb = FRAMEBUFFER.lock();
    
    let addr = fb.address() as u64;
    // If the address is in the lower half, it's a physical address that needs HHDM adjustment.
    // Higher half addresses on x86_64 start at 0x8000_0000_0000_0000 and above.
    let base_addr = if addr < 0x8000_0000_0000_0000 {
        crate::memory::vmm::phys_to_virt(addr)
    } else {
        addr
    };

    *global_fb = Some(Framebuffer {
        base: base_addr as *mut u32,
        phys_base: addr,
        width: fb.width as u32,
        height: fb.height as u32,
        pitch: fb.pitch as u32,
    });
    kprintln!("    Framebuffer: {}x{}, pitch={}, base={:#x}", fb.width, fb.height, fb.pitch, base_addr);
}

/// Get framebuffer width.
pub fn width() -> u32 {
    FRAMEBUFFER.lock().as_ref().map(|fb| fb.width).unwrap_or(0)
}

/// Get framebuffer height.
pub fn height() -> u32 {
    FRAMEBUFFER.lock().as_ref().map(|fb| fb.height).unwrap_or(0)
}

/// Get framebuffer pitch (bytes per row).
pub fn pitch() -> u32 {
    FRAMEBUFFER.lock().as_ref().map(|fb| fb.pitch).unwrap_or(0)
}

/// Get framebuffer physical base address and total size in bytes.
pub fn get_phys_info() -> Option<(u64, u64)> {
    let fb = FRAMEBUFFER.lock();
    fb.as_ref().map(|f| {
        let size = (f.height as u64) * (f.pitch as u64);
        (f.phys_base, size)
    })
}

/// Helper to get a mutable slice of a row for fast SIMD operations.
unsafe fn get_row_slice<'a>(fb: &'a Framebuffer, y: u32, x_start: u32, width: u32) -> Option<&'a mut [u32]> {
    if y >= fb.height || x_start >= fb.width {
        return None;
    }
    let actual_width = core::cmp::min(width, fb.width - x_start);
    if actual_width == 0 {
        return None;
    }
    
    let offset = (y * (fb.pitch / 4) + x_start) as isize;
    Some(slice::from_raw_parts_mut(fb.base.offset(offset), actual_width as usize))
}

/// Put a pixel at (x, y) with the given 32-bit ARGB color.
pub fn put_pixel(x: u32, y: u32, color: u32) {
    if let Some(ref fb) = *FRAMEBUFFER.lock() {
        if x < fb.width && y < fb.height {
            let offset = (y * (fb.pitch / 4) + x) as isize;
            unsafe {
                fb.base.offset(offset).write_volatile(color);
            }
        }
    }
}

/// Read a rectangle of pixels from the framebuffer into `buf`.
/// Returns number of pixels read (0 on failure).
pub fn read_pixels(x: u32, y: u32, w: u32, h: u32, buf: &mut [u32]) -> usize {
    if let Some(ref fb) = *FRAMEBUFFER.lock() {
        let row_words = (fb.pitch / 4) as usize;
        let mut count = 0;
        unsafe {
            for dy in 0..h {
                let sy = y + dy;
                if sy >= fb.height { break; }
                for dx in 0..w {
                    let sx = x + dx;
                    if sx >= fb.width { break; }
                    if count >= buf.len() { return count; }
                    let offset = (sy as usize * row_words + sx as usize) as isize;
                    buf[count] = (fb.base as *const u32).offset(offset).read_volatile();
                    count += 1;
                }
            }
        }
        count
    } else {
        0
    }
}

/// Fill the entire screen with a color.
pub fn clear(color: u32) {
    if let Some(ref fb) = *FRAMEBUFFER.lock() {
        unsafe {
            // Optimized clear using slice::fill
            let len = (fb.height * (fb.pitch / 4)) as usize;
            slice::from_raw_parts_mut(fb.base, len).fill(color);
        }
    }
}

/// Fill a rectangle (SSE/SIMD accelerated via slice::fill).
pub fn fill_rect(x: u32, y: u32, w: u32, h: u32, color: u32) {
    if let Some(ref fb) = *FRAMEBUFFER.lock() {
        unsafe {
            for dy in 0..h {
                if let Some(row) = get_row_slice(fb, y + dy, x, w) {
                    row.fill(color);
                }
            }
        }
    }
}

/// Blit an image/buffer to the screen (SSE/SIMD accelerated via slice::copy_from_slice).
pub fn blit_rect(x: u32, y: u32, w: u32, h: u32, src: &[u32]) {
    if let Some(ref fb) = *FRAMEBUFFER.lock() {
        unsafe {
            for dy in 0..h {
                if let Some(row) = get_row_slice(fb, y + dy, x, w) {
                    let src_start = (dy * w) as usize;
                    let src_end = src_start + row.len();
                    if src_end <= src.len() {
                        row.copy_from_slice(&src[src_start..src_end]);
                    }
                }
            }
        }
    }
}

/// Scroll the entire framebuffer content up by `pixels` rows.
/// Newly exposed rows at the bottom are filled with `bg`.
pub fn scroll_up(pixels: u32, bg: u32) {
    if let Some(ref fb) = *FRAMEBUFFER.lock() {
        let row_words = (fb.pitch / 4) as usize;
        let pixels = pixels.min(fb.height);
        let copy_rows = (fb.height - pixels) as usize;
        let copy_words = copy_rows * row_words;

        unsafe {
            // memmove upward by `pixels` rows (handles overlap correctly)
            let src = fb.base.add(pixels as usize * row_words);
            ptr::copy(src as *const u32, fb.base, copy_words);
            // Fill the vacated bottom rows with background color
            slice::from_raw_parts_mut(fb.base.add(copy_words), pixels as usize * row_words).fill(bg);
        }
    }
}

/// Draw a character using the beast_font 8x8 font, with an integer scaling factor.
pub fn draw_char_scaled(x: u32, y: u32, c: char, fg: u32, bg: u32, scale: u32) {
    if let Some(bitmap) = beast_font::get_char_bitmap(c) {
        if let Some(ref fb) = *FRAMEBUFFER.lock() {
            unsafe {
                for (dy, row_val) in bitmap.iter().enumerate() {
                    for sy in 0..scale {
                        let screen_y = y + (dy as u32 * scale) + sy;
                        if let Some(row_slice) = get_row_slice(fb, screen_y, x, 8 * scale) {
                            for dx in 0..8 {
                                let pixel_color = if (row_val & (0x80 >> dx)) != 0 { fg } else { bg };
                                for sx in 0..scale {
                                    let screen_x = (dx as u32 * scale) + sx;
                                    if (screen_x as usize) < row_slice.len() {
                                        row_slice[screen_x as usize] = pixel_color;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    } else {
        // Fallback for missing character: draw a solid block
        fill_rect(x, y, 8 * scale, 8 * scale, fg);
    }
}
