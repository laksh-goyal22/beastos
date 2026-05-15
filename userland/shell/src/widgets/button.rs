use crate::framebuf;

pub struct Button {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub label: [u8; 32],
    pub label_len: usize,
    pub hover: bool,
}

impl Button {
    pub fn new() -> Self {
        Self {
            x: 0, y: 0, w: 60, h: 24,
            label: [0; 32],
            label_len: 0,
            hover: false,
        }
    }

    pub fn draw(&self) {
        let bg = if self.hover { 0xFF555555u32 } else { 0xFF333333u32 };
        framebuf::fill_rect(self.x, self.y, self.w, self.h, bg);
        framebuf::fill_rect(self.x, self.y, self.w, 1, 0xFF666666);
        let text: [u8; 32] = self.label;
        let len = self.label_len;
        let tx = self.x + 6;
        let ty = self.y + (self.h - 16) / 2;
        for i in 0..len {
            let c = text[i] as char;
            if c.is_ascii() {
                framebuf::draw_char(tx + (i as u32) * 10, ty, c, 0xFFFFFFFF, bg, 2);
            }
        }
    }
}
