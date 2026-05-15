use crate::desktop::colors;
use crate::framebuf;

pub struct Window {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub title: [u8; 64],
    pub title_len: usize,
    pub visible: bool,
}

impl Window {
    pub fn new() -> Self {
        Self {
            x: 0, y: 0,
            w: framebuf::width(),
            h: framebuf::height(),
            title: [0; 64],
            title_len: 0,
            visible: true,
        }
    }

    pub fn set_title(&mut self, title: &str) {
        let bytes = title.as_bytes();
        let len = bytes.len().min(63);
        self.title[..len].copy_from_slice(&bytes[..len]);
        self.title_len = len;
    }

    pub fn set_bounds(&mut self, x: u32, y: u32, w: u32, h: u32) {
        self.x = x;
        self.y = y;
        self.w = w;
        self.h = h;
    }

    pub fn center(&mut self, fb_w: u32, fb_h: u32) {
        self.x = (fb_w - self.w) / 2;
        self.y = (fb_h - self.h) / 4;
    }

    pub fn draw(&self) {
        if !self.visible { return; }
        // Border
        framebuf::fill_rect(self.x.saturating_sub(2), self.y.saturating_sub(2), self.w + 4, self.h + 4, colors::BORDER);
        // Background
        framebuf::fill_rect(self.x, self.y, self.w, self.h, colors::WINDOW_BG);
        // Title bar
        let title_h = 28;
        framebuf::fill_rect(self.x, self.y, self.w, title_h, colors::TITLE_BG);
        // Title text
        let title_str = core::str::from_utf8(&self.title[..self.title_len]).unwrap_or("");
        let tx = self.x + 8;
        let ty = self.y + (title_h - 16) / 2;
        framebuf::draw_str(tx, ty, title_str, colors::TITLE_FG, colors::TITLE_BG, 2);
        // Close button area
        framebuf::fill_rect(self.x + self.w - 28, self.y + 4, 24, 20, 0xFFAA3333);
        // Separator line
        framebuf::fill_rect(self.x, self.y + title_h, self.w, 1, colors::BORDER);
    }

    pub fn client_x(&self) -> u32 { self.x + 2 }
    pub fn client_y(&self) -> u32 { self.y + 30 }
    pub fn client_w(&self) -> u32 { self.w.saturating_sub(4) }
    pub fn client_h(&self) -> u32 { self.h.saturating_sub(32) }
}
