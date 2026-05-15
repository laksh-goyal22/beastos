#![no_std]
#![no_main]

extern crate beast_crt;

#[no_mangle]
pub extern "C" fn beast_main(argc: u64, argv: u64) -> i64 {
    let mut editor = Editor::new();

    if argc > 1 {
        let argv = argv as *const *const u8;
        unsafe {
            let arg_ptr = *argv.add(1);
            let mut len = 0;
            while *arg_ptr.add(len) != 0 { len += 1; }
            if let Ok(name) = core::str::from_utf8(core::slice::from_raw_parts(arg_ptr, len)) {
                editor.load_file(name);
            }
        }
    }

    editor.run();
    0
}

const MAX_LINES: usize = 1000;
const MAX_LINE_LEN: usize = 256;
const ROWS: usize = 25;
const COLS: usize = 80;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Pos {
    y: usize,
    x: usize,
}

pub struct Editor {
    buffer: [[u8; MAX_LINE_LEN]; MAX_LINES],
    line_lens: [usize; MAX_LINES],
    line_count: usize,
    cursor: Pos,
    scroll_y: usize,
    filename: [u8; 64],
    filename_len: usize,
    dirty: bool,
    selection_start: Option<Pos>,
    is_selecting: bool,
}

impl Editor {
    pub fn new() -> Self {
        Self {
            buffer: [[0; MAX_LINE_LEN]; MAX_LINES],
            line_lens: [0; MAX_LINES],
            line_count: 1,
            cursor: Pos { y: 0, x: 0 },
            scroll_y: 0,
            filename: [0; 64],
            filename_len: 0,
            dirty: false,
            selection_start: None,
            is_selecting: false,
        }
    }

    pub fn load_file(&mut self, name: &str) {
        let len = name.len().min(63);
        self.filename[..len].copy_from_slice(name.as_bytes());
        self.filename_len = len;

        let fd = beast_syscall::open(name);
        if fd >= 0 {
            self.line_count = 0;
            let mut line_pos = 0;
            loop {
                let mut byte = [0u8; 1];
                let n = beast_syscall::read(fd as u64, &mut byte);
                if n == 0 { break; }

                if byte[0] == b'\n' {
                    if self.line_count < MAX_LINES {
                        self.line_lens[self.line_count] = line_pos;
                        self.line_count += 1;
                    }
                    line_pos = 0;
                    if self.line_count >= MAX_LINES { break; }
                } else if line_pos < MAX_LINE_LEN {
                    self.buffer[self.line_count][line_pos] = byte[0];
                    line_pos += 1;
                }
            }
            if line_pos > 0 && self.line_count < MAX_LINES {
                self.line_lens[self.line_count] = line_pos;
                self.line_count += 1;
            }
            if self.line_count == 0 { self.line_count = 1; }
            beast_syscall::close(fd as u64);
        }
    }

    pub fn run(&mut self) {
        beast_syscall::clear_screen();
        self.draw();

        loop {
            let mut mouse_buf = [0i32; 5];
            if beast_syscall::get_mouse(&mut mouse_buf) == 0 {
                let mx = mouse_buf[0];
                let my = mouse_buf[1];
                let ml = mouse_buf[2] != 0;

                if ml {
                    let char_x = (mx / 16) as usize;
                    let char_y = ((my - 32) / 16) as usize + self.scroll_y;

                    if char_y < self.line_count {
                        let new_pos = Pos {
                            y: char_y,
                            x: char_x.min(self.line_lens[char_y])
                        };
                        if !self.is_selecting {
                            self.selection_start = Some(self.cursor);
                            self.is_selecting = true;
                        }
                        self.cursor = new_pos;
                    }
                } else if self.is_selecting {
                    self.is_selecting = false;
                }
            }

            let ch = self.try_get_char();
            if let Some(ch) = ch {
                match ch {
                    1 | 2 | 5..=7 | 9 | 11..=12 | 14..=16 | 18 | 20..=21 | 23 | 25..=26 => {}
                    3 => self.copy(),
                    24 => self.cut(),
                    22 => self.paste(),
                    19 => self.save(),
                    17 => return,
                    0x80 => { self.selection_start = None; self.move_up(); }
                    0x81 => { self.selection_start = None; self.move_down(); }
                    0x82 => { self.selection_start = None; self.move_left(); }
                    0x83 => { self.selection_start = None; self.move_right(); }
                    0x84 => {
                        if self.selection_start.is_none() { self.selection_start = Some(self.cursor); }
                        self.move_up();
                    }
                    0x85 => {
                        if self.selection_start.is_none() { self.selection_start = Some(self.cursor); }
                        self.move_down();
                    }
                    0x86 => {
                        if self.selection_start.is_none() { self.selection_start = Some(self.cursor); }
                        self.move_left();
                    }
                    0x87 => {
                        if self.selection_start.is_none() { self.selection_start = Some(self.cursor); }
                        self.move_right();
                    }
                    8 | 127 => { self.handle_backspace(); self.dirty = true; self.selection_start = None; }
                    10 | 13 => { self.handle_enter(); self.dirty = true; self.selection_start = None; }
                    32..=126 => { self.insert_char(ch); self.dirty = true; self.selection_start = None; }
                    _ => {}
                }
            }
            self.draw();
            beast_syscall::yield_now();
        }
    }

    fn move_up(&mut self) {
        if self.cursor.y > 0 {
            self.cursor.y -= 1;
            if self.cursor.y < self.scroll_y { self.scroll_y -= 1; }
            self.cursor.x = self.cursor.x.min(self.line_lens[self.cursor.y]);
        }
    }

    fn move_down(&mut self) {
        if self.cursor.y < self.line_count - 1 {
            self.cursor.y += 1;
            if self.cursor.y >= self.scroll_y + ROWS - 4 { self.scroll_y += 1; }
            self.cursor.x = self.cursor.x.min(self.line_lens[self.cursor.y]);
        }
    }

    fn move_left(&mut self) {
        if self.cursor.x > 0 {
            self.cursor.x -= 1;
        } else if self.cursor.y > 0 {
            self.cursor.y -= 1;
            self.cursor.x = self.line_lens[self.cursor.y];
        }
    }

    fn move_right(&mut self) {
        if self.cursor.x < self.line_lens[self.cursor.y] {
            self.cursor.x += 1;
        } else if self.cursor.y < self.line_count - 1 {
            self.cursor.y += 1;
            self.cursor.x = 0;
        }
    }

    fn try_get_char(&self) -> Option<u8> {
        let mut buf = [0u8; 1];
        let res = beast_syscall::read(0, &mut buf);
        if res > 0 { Some(buf[0]) } else { None }
    }

    fn draw(&self) {
        beast_syscall::clear_screen();

        self.print_at(0, 0, " BEAST EDITOR v0.1.0 | Shift + Arrow / Mouse Select | Ctrl+C/V: Copy/Paste ");
        self.print_at(0, 1, "--------------------------------------------------------------------------------");

        for i in 0..(ROWS - 4) {
            let line_idx = self.scroll_y + i;
            if line_idx < self.line_count {
                let len = self.line_lens[line_idx];
                let line_str = core::str::from_utf8(&self.buffer[line_idx][..len]).unwrap_or("?");
                self.print_at(0, (i + 2) as u32, line_str);
            }
        }

        let footer_y = (ROWS - 1) as u32;
        let mut status = [0u8; 128];
        let mut pos = 0;

        let s1 = b" Line: ";
        status[pos..pos+s1.len()].copy_from_slice(s1);
        pos += s1.len();
        pos += self.write_num_to_buf((self.cursor.y + 1) as u64, &mut status[pos..]);

        let s2 = b" Col: ";
        status[pos..pos+s2.len()].copy_from_slice(s2);
        pos += s2.len();
        pos += self.write_num_to_buf((self.cursor.x + 1) as u64, &mut status[pos..]);

        if let Some(_) = self.selection_start {
            let s_sel = b" [Selecting]";
            status[pos..pos+s_sel.len()].copy_from_slice(s_sel);
            pos += s_sel.len();
        }

        if self.dirty {
            let s3 = b" [Modified]";
            status[pos..pos+s3.len()].copy_from_slice(s3);
            pos += s3.len();
        }

        self.print_at(0, footer_y, core::str::from_utf8(&status[..pos]).unwrap_or(""));
    }

    fn print_at(&self, _x: u32, _y: u32, s: &str) {
        beast_syscall::write(1, s.as_bytes());
        beast_syscall::write(1, b"\n");
    }

    fn write_num_to_buf(&self, n: u64, buf: &mut [u8]) -> usize {
        let mut digits = [0u8; 10];
        let mut i = 0;
        let mut num = n;
        while num > 0 {
            digits[i] = b'0' + (num % 10) as u8;
            num /= 10;
            i += 1;
        }
        if i == 0 { digits[0] = b'0'; i = 1; }
        for j in 0..i { buf[j] = digits[i - 1 - j]; }
        i
    }

    fn insert_char(&mut self, ch: u8) {
        let len = self.line_lens[self.cursor.y];
        if len < MAX_LINE_LEN - 1 {
            for i in (self.cursor.x..len).rev() {
                self.buffer[self.cursor.y][i + 1] = self.buffer[self.cursor.y][i];
            }
            self.buffer[self.cursor.y][self.cursor.x] = ch;
            self.line_lens[self.cursor.y] += 1;
            self.cursor.x += 1;
        }
    }

    fn handle_backspace(&mut self) {
        if self.cursor.x > 0 {
            let len = self.line_lens[self.cursor.y];
            for i in self.cursor.x..len {
                self.buffer[self.cursor.y][i - 1] = self.buffer[self.cursor.y][i];
            }
            self.line_lens[self.cursor.y] -= 1;
            self.cursor.x -= 1;
        } else if self.cursor.y > 0 {
            let prev_line = self.cursor.y - 1;
            let prev_len = self.line_lens[prev_line];
            let curr_len = self.line_lens[self.cursor.y];

            if prev_len + curr_len < MAX_LINE_LEN {
                for i in 0..curr_len {
                    self.buffer[prev_line][prev_len + i] = self.buffer[self.cursor.y][i];
                }
                self.line_lens[prev_line] += curr_len;
                self.cursor.x = prev_len;
                for i in self.cursor.y..(self.line_count - 1) {
                    self.buffer[i] = self.buffer[i + 1];
                    self.line_lens[i] = self.line_lens[i + 1];
                }
                self.line_count -= 1;
                self.cursor.y -= 1;
            }
        }
    }

    fn handle_enter(&mut self) {
        if self.line_count < MAX_LINES - 1 {
            let len = self.line_lens[self.cursor.y];
            let split_at = self.cursor.x;
            let remain_len = len - split_at;
            for i in (self.cursor.y + 2..self.line_count + 1).rev() {
                self.buffer[i] = self.buffer[i - 1];
                self.line_lens[i] = self.line_lens[i - 1];
            }
            for i in 0..remain_len {
                self.buffer[self.cursor.y + 1][i] = self.buffer[self.cursor.y][split_at + i];
            }
            self.line_lens[self.cursor.y + 1] = remain_len;
            self.line_lens[self.cursor.y] = split_at;
            self.line_count += 1;
            self.cursor.y += 1;
            self.cursor.x = 0;
        }
    }

    fn save(&mut self) {
        let name = if self.filename_len > 0 {
            core::str::from_utf8(&self.filename[..self.filename_len]).unwrap_or("error.txt")
        } else {
            "new_file.txt"
        };
        let _ = beast_syscall::create(name);
        let fd = beast_syscall::open(name);
        if fd >= 0 {
            for i in 0..self.line_count {
                beast_syscall::write(fd as u64, &self.buffer[i][..self.line_lens[i]]);
                beast_syscall::write(fd as u64, b"\n");
            }
            beast_syscall::close(fd as u64);
            self.dirty = false;
        }
    }

    fn copy(&self) {
        if let Some(start) = self.selection_start {
            let (p1, p2) = if start < self.cursor { (start, self.cursor) } else { (self.cursor, start) };

            let mut clip_data = [0u8; 1024];
            let mut pos = 0;

            for y in p1.y..=p2.y {
                let start_x = if y == p1.y { p1.x } else { 0 };
                let end_x = if y == p2.y { p2.x } else { self.line_lens[y] };

                let line_data = &self.buffer[y][start_x..end_x];
                let copy_len = line_data.len().min(1024 - pos);
                clip_data[pos..pos+copy_len].copy_from_slice(&line_data[..copy_len]);
                pos += copy_len;

                if y < p2.y && pos < 1024 {
                    clip_data[pos] = b'\n';
                    pos += 1;
                }
            }
            beast_syscall::clipboard_copy(&clip_data[..pos]);
        } else {
            let len = self.line_lens[self.cursor.y];
            beast_syscall::clipboard_copy(&self.buffer[self.cursor.y][..len]);
        }
    }

    fn cut(&mut self) {
        self.copy();
        self.line_lens[self.cursor.y] = 0;
        self.cursor.x = 0;
        self.dirty = true;
    }

    fn paste(&mut self) {
        let mut buf = [0u8; 1024];
        let len = beast_syscall::clipboard_paste(&mut buf);
        if len > 0 {
            for i in 0..(len as usize).min(1024) {
                if buf[i] == b'\n' {
                    self.handle_enter();
                } else {
                    self.insert_char(buf[i]);
                }
            }
        }
    }
}
