use crate::framebuf;
use crate::desktop::colors;
use crate::ui::window::Window;
use crate::commands;

const SCROLLBACK_LINES: usize = 1024;
const INPUT_BUF_SIZE: usize = 256;

pub struct Terminal {
    pub window: Window,
    text_buf: [[u8; 128]; SCROLLBACK_LINES],
    text_lens: [usize; SCROLLBACK_LINES],
    text_count: usize,
    scroll_offset: usize,
    cursor_visible: bool,
    cursor_tick: u32,
    input_buf: [u8; INPUT_BUF_SIZE],
    input_len: usize,
    prompt: [u8; 128],
    prompt_len: usize,
}

impl Terminal {
    pub fn new() -> Self {
        let mut win = Window::new();
        win.set_title("Terminal");
        let scale = 2;
        let char_w = 8 * scale;
        let char_h = 8 * scale;
        let cols = 80;
        let rows = 25;
        let win_w = (cols * char_w) + 24;
        let win_h = (rows * char_h) + 50;
        win.w = if win_w < framebuf::width() { win_w } else { framebuf::width() - 40 };
        win.h = if win_h < framebuf::height() { win_h } else { framebuf::height() - 60 };
        win.center(framebuf::width(), framebuf::height());

        let mut t = Self {
            window: win,
            text_buf: [[0; 128]; SCROLLBACK_LINES],
            text_lens: [0; SCROLLBACK_LINES],
            text_count: 0,
            scroll_offset: 0,
            cursor_visible: true,
            cursor_tick: 0,
            input_buf: [0; INPUT_BUF_SIZE],
            input_len: 0,
            prompt: [0; 128],
            prompt_len: 0,
        };
        t.update_prompt();
        t
    }

    fn update_prompt(&mut self) {
        let mut cwd = [0u8; 128];
        let len = beast_syscall::getcwd(&mut cwd) as usize;
        let prefix = b"beast@beastos:";
        let suffix = b" $ ";
        let mut buf = [0u8; 128];
        let mut pos = 0;
        for &b in prefix { if pos < 127 { buf[pos] = b; pos += 1; } }
        if len > 0 {
            for i in 0..len.min(96) { if pos < 127 { buf[pos] = cwd[i]; pos += 1; } }
        } else {
            for &b in b"~" { if pos < 127 { buf[pos] = b; pos += 1; } }
        }
        for &b in suffix { if pos < 127 { buf[pos] = b; pos += 1; } }
        self.prompt[..pos].copy_from_slice(&buf[..pos]);
        self.prompt_len = pos;
    }

    pub fn writeln(&mut self, s: &str) {
        let idx = self.text_count % SCROLLBACK_LINES;
        let bytes = s.as_bytes();
        let len = bytes.len().min(127);
        self.text_buf[idx][..len].copy_from_slice(&bytes[..len]);
        self.text_lens[idx] = len;
        self.text_count += 1;
        self.scroll_offset = 0;
    }

    pub fn draw(&self) {
        self.window.draw();

        let cx = self.window.client_x() + 4;
        let cy = self.window.client_y() + 4;
        let cw = self.window.client_w().saturating_sub(8);
        let ch = self.window.client_h().saturating_sub(8);

        framebuf::fill_rect(cx - 2, cy - 2, cw + 4, ch + 4, colors::TERMINAL_BG);

        let scale = 2;
        let char_w = 8 * scale;
        let char_h = 8 * scale;
        let max_visible_lines = if char_h > 0 { (ch / char_h) as usize } else { 10 };

        let total = if self.text_count < SCROLLBACK_LINES { self.text_count } else { SCROLLBACK_LINES };
        let visible = max_visible_lines.min(total);

        for i in 0..visible {
            let idx = (self.text_count - visible + i) % SCROLLBACK_LINES;
            let line = core::str::from_utf8(&self.text_buf[idx][..self.text_lens[idx]]).unwrap_or("");
            framebuf::draw_str(cx, cy + (i as u32) * char_h, line, colors::TERMINAL_FG, colors::TERMINAL_BG, scale);
        }

        if self.cursor_visible {
            let cursor_y = cy + (visible as u32) * char_h;
            framebuf::fill_rect(cx, cursor_y + char_h - 3, char_w, 3, colors::TERMINAL_FG);
        }

        let prompt_str = core::str::from_utf8(&self.prompt[..self.prompt_len]).unwrap_or("$ ");
        framebuf::draw_str(cx, cy + (visible as u32) * char_h, prompt_str, colors::TERMINAL_FG, colors::TERMINAL_BG, scale);
        let prompt_width = prompt_str.len() as u32 * char_w;
        let input_str = core::str::from_utf8(&self.input_buf[..self.input_len]).unwrap_or("");
        let input_x = cx + prompt_width;
        framebuf::draw_str(input_x, cy + (visible as u32) * char_h, input_str, colors::TERMINAL_FG, colors::TERMINAL_BG, scale);

        if total > max_visible_lines {
            let sb_x = self.window.client_x() + self.window.client_w() - 10;
            let sb_h = self.window.client_h() - 8;
            framebuf::fill_rect(sb_x, cy, 8, sb_h, colors::SCROLLBAR_BG);
            let thumb_h = (sb_h as usize * max_visible_lines / total).max(8) as u32;
            let ratio = self.scroll_offset as u32 * (sb_h - thumb_h) / (total - max_visible_lines) as u32;
            framebuf::fill_rect(sb_x, cy + ratio, 8, thumb_h, colors::SCROLLBAR_FG);
        }
    }

    fn execute_input(&mut self) {
        if self.input_len == 0 { return; }
        {
            let mut line = [0u8; 256];
            let mut pos = 0;
            for i in 0..self.prompt_len.min(100) {
                if pos < 255 { line[pos] = self.prompt[i]; pos += 1; }
            }
            for i in 0..self.input_len.min(155) {
                if pos < 255 { line[pos] = self.input_buf[i]; pos += 1; }
            }
            let idx = self.text_count % SCROLLBACK_LINES;
            self.text_buf[idx][..pos].copy_from_slice(&line[..pos]);
            self.text_lens[idx] = pos;
            self.text_count += 1;
        }
        let input = core::str::from_utf8(&self.input_buf[..self.input_len]).unwrap_or("");
        // Parse command and args like the shell does
        let mut words = input.split_whitespace();
        let cmd_raw = words.next();
        if let Some(cmd) = cmd_raw {
            let mut args = [""; 8];
            let mut count = 0;
            for word in words {
                if count < 8 { args[count] = word; count += 1; }
            }
            commands::execute(cmd, &args[..count]);
        }
        self.input_len = 0;
        self.update_prompt();
        self.scroll_offset = 0;
    }

    pub fn handle_key(&mut self, key: u8) {
        match key {
            8 | 127 => {
                if self.input_len > 0 { self.input_len -= 1; }
            }
            10 | 13 => {
                self.execute_input();
            }
            3 => {
                self.input_len = 0;
            }
            32..=126 => {
                if self.input_len < INPUT_BUF_SIZE - 1 {
                    self.input_buf[self.input_len] = key;
                    self.input_len += 1;
                }
            }
            _ => {}
        }
    }

    pub fn tick(&mut self) {
        self.cursor_tick += 1;
        if self.cursor_tick >= 25 {
            self.cursor_tick = 0;
            self.cursor_visible = !self.cursor_visible;
        }
    }

    pub fn run(&mut self) -> ! {
        loop {
            self.draw();
            self.tick();
            let mut byte = [0u8; 1];
            let n = beast_syscall::read(0, &mut byte);
            if n > 0 {
                self.handle_key(byte[0]);
            } else {
                beast_syscall::yield_now();
            }
        }
    }
}
