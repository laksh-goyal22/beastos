#![no_std]
#![no_main]

extern crate beast_crt;

mod commands;
mod framebuf;
mod desktop;
mod ui;
mod widgets;
mod apps;

fn print_str(s: &str) {
    beast_syscall::write(1, s.as_bytes());
}

fn print_char(c: u8) {
    beast_syscall::write(1, &[c]);
}

#[no_mangle]
pub extern "C" fn beast_main(_argc: u64, _argv: u64) -> i64 {
    print_str("SERIAL: Shell started!\n");

    let mut shell = Shell::new();
    shell.run();
    0
}

pub struct Shell {
    input_buffer: [u8; 256],
    buffer_len: usize,
    history: [[u8; 256]; 10],
    history_lens: [usize; 10],
    history_count: usize,
    history_index: i32,
}

impl Shell {
    pub fn new() -> Self {
        Self {
            input_buffer: [0; 256],
            buffer_len: 0,
            history: [[0; 256]; 10],
            history_lens: [0; 10],
            history_count: 0,
            history_index: -1,
        }
    }

    pub fn run(&mut self) {
        beast_syscall::clear_screen();
        self.print_banner();

        loop {
            self.print_prompt();
            self.read_line();

            if self.buffer_len > 0 {
                let mut current_cmd = [0u8; 256];
                let len = self.buffer_len;
                current_cmd[..len].copy_from_slice(&self.input_buffer[..len]);

                if let Ok(input) = core::str::from_utf8(&current_cmd[..len]) {
                    self.add_to_history(input);
                    self.execute_command(input);
                }
            }
        }
    }

    fn add_to_history(&mut self, line: &str) {
        if line.is_empty() { return; }

        if self.history_count > 0 {
            let last_idx = (self.history_count - 1) % 10;
            if &self.history[last_idx][..self.history_lens[last_idx]] == line.as_bytes() {
                return;
            }
        }

        let idx = self.history_count % 10;
        let bytes = line.as_bytes();
        let len = bytes.len().min(255);
        self.history[idx][..len].copy_from_slice(&bytes[..len]);
        self.history_lens[idx] = len;
        self.history_count += 1;
    }

    fn print_banner(&self) {
        print_str("==========================================================\n");
        print_str("                 B E A S T   O S   v0.1.0                 \n");
        print_str("               \"Never move data twice\"                    \n");
        print_str("==========================================================\n");
        print_str("Type 'help' for available commands\n\n");
    }

    fn print_prompt(&self) {
        let mut buf = [0u8; 128];
        let len = beast_syscall::getcwd(&mut buf) as usize;
        if len > 0 {
            if let Ok(path) = core::str::from_utf8(&buf[..len]) {
                print_str("beast@beastos:");
                print_str(path);
                print_str("$ ");
                return;
            }
        }
        print_str("beast@beastos:~$ ");
    }

    fn read_line(&mut self) {
        self.buffer_len = 0;
        self.history_index = -1;

        loop {
            let ch = self.get_char();

            match ch {
                0x80 => {
                    if self.history_count > 0 {
                        let history_avail = self.history_count.min(10);
                        if self.history_index + 1 < history_avail as i32 {
                            self.history_index += 1;
                            self.load_history();
                        }
                    }
                }
                0x81 => {
                    if self.history_index > 0 {
                        self.history_index -= 1;
                        self.load_history();
                    } else if self.history_index == 0 {
                        self.history_index = -1;
                        self.clear_current_line();
                    }
                }
                9 => { self.handle_tab(); }
                8 | 127 => {
                    if self.buffer_len > 0 {
                        self.buffer_len -= 1;
                        print_char(8);
                    }
                }
                10 | 13 => {
                    print_char(b'\n');
                    break;
                }
                3 => {
                    print_str("^C\n");
                    self.buffer_len = 0;
                    return;
                }
                4 => {
                    print_char(b'\n');
                    beast_syscall::exit(0);
                }
                32..=126 => {
                    if self.buffer_len < 255 {
                        self.input_buffer[self.buffer_len] = ch;
                        self.buffer_len += 1;
                        print_char(ch);
                    }
                }
                _ => {}
            }
        }
    }

    fn load_history(&mut self) {
        if self.history_index < 0 { return; }
        self.clear_current_line();

        let idx = (self.history_count - 1 - self.history_index as usize) % 10;
        let len = self.history_lens[idx];
        self.input_buffer[..len].copy_from_slice(&self.history[idx][..len]);
        self.buffer_len = len;

        beast_syscall::write(1, &self.input_buffer[..self.buffer_len]);
    }

    fn clear_current_line(&mut self) {
        while self.buffer_len > 0 {
            print_char(8);
            self.buffer_len -= 1;
        }
    }

    fn handle_tab(&mut self) {
        let mut word_start = self.buffer_len;
        while word_start > 0 && self.input_buffer[word_start - 1] != b' ' {
            word_start -= 1;
        }

        if let Ok(prefix) = core::str::from_utf8(&self.input_buffer[word_start..self.buffer_len]) {
            if prefix.is_empty() { return; }

            let mut best_match: Option<&str> = None;
            let mut match_count = 0;

            for &cmd in commands::COMMANDS {
                if cmd.starts_with(prefix) {
                    best_match = Some(cmd);
                    match_count += 1;
                }
            }

            let mut buf = [0u8; 1024];
            let ret = beast_syscall::ls(".", &mut buf);
            if ret >= 0 {
                if let Ok(entries_str) = core::str::from_utf8(&buf[..ret as usize]) {
                    for entry in entries_str.split_whitespace() {
                        let entry_name = entry.trim_end_matches('/');
                        if entry_name.starts_with(prefix) {
                            best_match = Some(entry);
                            match_count += 1;
                        }
                    }
                }
            }

            if match_count == 1 {
                if let Some(m) = best_match {
                    let remaining = &m[prefix.len()..];
                    for &b in remaining.as_bytes() {
                        if self.buffer_len < 255 {
                            self.input_buffer[self.buffer_len] = b;
                            self.buffer_len += 1;
                            print_char(b);
                        }
                    }
                }
            }
        }
    }

    fn get_char(&self) -> u8 {
        let mut buf = [0u8; 1];
        let result = beast_syscall::read(0, &mut buf);
        if result == 0 {
            beast_syscall::yield_now();
            return self.get_char();
        }
        buf[0]
    }

    fn execute_command(&self, input: &str) {
        let mut words = input.split_whitespace();
        let cmd_raw = words.next();
        if cmd_raw.is_none() { return; }
        let cmd = cmd_raw.unwrap();

        let mut arg_bufs = [[0u8; 128]; 8];
        let mut args = [""; 8];
        let mut count = 0;

        for word in words {
            if count < 8 {
                let bytes = word.as_bytes();
                let len = bytes.len().min(127);
                arg_bufs[count][..len].copy_from_slice(&bytes[..len]);
                arg_bufs[count][len] = 0;
                count += 1;
            }
        }

        for i in 0..count {
            let mut len = 0;
            while arg_bufs[i][len] != 0 { len += 1; }
            if let Ok(s) = core::str::from_utf8(&arg_bufs[i][..len]) {
                args[i] = s;
            }
        }

        let mut cmd_buf = [0u8; 128];
        let cmd_len = cmd.len().min(127);
        cmd_buf[..cmd_len].copy_from_slice(cmd.as_bytes());
        cmd_buf[cmd_len] = 0;
        let cmd_norm = core::str::from_utf8(&cmd_buf[..cmd_len]).unwrap();

        commands::execute(cmd_norm, &args[..count]);
    }
}
