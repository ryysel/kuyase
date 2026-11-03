use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::ChildStdin;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SerialSnapshot {
    pub lines: Vec<String>,
    pub active_line: String,
    pub cursor_col: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum AnsiParseState {
    #[default]
    Ground,
    Escape,
    Csi(String),
    Osc,
}

#[derive(Debug, Default)]
pub struct SerialScreenBuffer {
    completed_lines: VecDeque<String>,
    active_line: Vec<char>,
    cursor_col: usize,
    parse_state: AnsiParseState,
    saw_cursor_home: bool,
    utf8_tail: Vec<u8>,
}

impl SerialScreenBuffer {
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            completed_lines: VecDeque::with_capacity(cap),
            ..Self::default()
        }
    }

    pub fn push_completed_line(&mut self, line: String) {
        if self.completed_lines.len() >= 2000 {
            self.completed_lines.pop_front();
        }
        self.completed_lines.push_back(line);
    }

    pub fn clear_scrollback(&mut self) {
        self.completed_lines.clear();
    }

    pub fn snapshot(&self) -> SerialSnapshot {
        SerialSnapshot {
            lines: self.completed_lines.iter().cloned().collect(),
            active_line: self.active_line.iter().collect(),
            cursor_col: self.cursor_col,
        }
    }

    pub fn all_lines(&self) -> Vec<String> {
        let mut out: Vec<String> = self.completed_lines.iter().cloned().collect();
        if !self.active_line.is_empty() {
            out.push(self.active_line.iter().collect());
        }
        out
    }

    fn put_char(&mut self, ch: char) {
        while self.active_line.len() < self.cursor_col {
            self.active_line.push(' ');
        }
        if self.cursor_col < self.active_line.len() {
            self.active_line[self.cursor_col] = ch;
        } else {
            self.active_line.push(ch);
        }
        self.cursor_col += 1;
    }

    fn trim_erased_tail(&mut self) {
        while self.active_line.len() > self.cursor_col && self.active_line.last() == Some(&' ') {
            self.active_line.pop();
        }
    }

    fn handle_csi(&mut self, params: &str, final_byte: char, dsr_col: &mut Option<usize>) {
        let first_num = params
            .trim_start_matches('?')
            .split(';')
            .next()
            .and_then(|s| s.parse::<usize>().ok());

        match final_byte {
            'K' => match first_num.unwrap_or(0) {
                0 => {
                    if self.cursor_col < self.active_line.len() {
                        self.active_line.truncate(self.cursor_col);
                    }
                }
                1 => {
                    let end = self.cursor_col.min(self.active_line.len().saturating_sub(1));
                    for idx in 0..=end {
                        if idx < self.active_line.len() {
                            self.active_line[idx] = ' ';
                        }
                    }
                }
                2 => {
                    self.active_line.clear();
                    self.cursor_col = 0;
                }
                _ => {}
            },
            'J' => {
                let mode = first_num.unwrap_or(0);
                if mode == 2 || mode == 3 || self.saw_cursor_home {
                    self.completed_lines.clear();
                    self.active_line.clear();
                    self.cursor_col = 0;
                } else if self.cursor_col < self.active_line.len() {
                    self.active_line.truncate(self.cursor_col);
                }
                self.saw_cursor_home = false;
            }
            'H' | 'f' => {
                if params.is_empty() || params == "1;1" || params == ";" {
                    self.saw_cursor_home = true;
                }
            }
            'D' => {
                let step = first_num.unwrap_or(1).max(1);
                self.cursor_col = self.cursor_col.saturating_sub(step);
                self.trim_erased_tail();
            }
            'C' => {
                let step = first_num.unwrap_or(1).max(1);
                self.cursor_col += step;
                while self.active_line.len() < self.cursor_col {
                    self.active_line.push(' ');
                }
            }
            'G' => {
                let col1 = first_num.unwrap_or(1).max(1);
                self.cursor_col = col1 - 1;
                while self.active_line.len() < self.cursor_col {
                    self.active_line.push(' ');
                }
            }
            'P' => {
                let count = first_num.unwrap_or(1).max(1);
                for _ in 0..count {
                    if self.cursor_col < self.active_line.len() {
                        self.active_line.remove(self.cursor_col);
                    }
                }
            }
            '@' => {
                let count = first_num.unwrap_or(1).max(1);
                for _ in 0..count {
                    if self.cursor_col <= self.active_line.len() {
                        self.active_line.insert(self.cursor_col, ' ');
                    }
                }
            }
            'n' => {
                if params == "6" {
                    *dsr_col = Some(self.cursor_col + 1);
                }
            }
            _ => {}
        }
    }

    fn feed_char(&mut self, ch: char, completed: &mut Vec<String>, dsr_col: &mut Option<usize>) {
        match &mut self.parse_state {
            AnsiParseState::Escape => {
                match ch {
                    '[' => self.parse_state = AnsiParseState::Csi(String::new()),
                    ']' => self.parse_state = AnsiParseState::Osc,
                    'c' => {
                        self.active_line.clear();
                        self.cursor_col = 0;
                        self.parse_state = AnsiParseState::Ground;
                    }
                    _ => self.parse_state = AnsiParseState::Ground,
                }
                return;
            }
            AnsiParseState::Osc => {
                if ch == '\x07' || ch == '\n' {
                    self.parse_state = AnsiParseState::Ground;
                }
                return;
            }
            AnsiParseState::Csi(params) => {
                if ('@'..='~').contains(&ch) {
                    let params_owned = std::mem::take(params);
                    self.parse_state = AnsiParseState::Ground;
                    self.handle_csi(&params_owned, ch, dsr_col);
                } else if params.len() < 32 {
                    params.push(ch);
                } else {
                    self.parse_state = AnsiParseState::Ground;
                }
                return;
            }
            AnsiParseState::Ground => {}
        }

        if ch != '\x1b' {
            self.saw_cursor_home = false;
        }

        match ch {
            '\x1b' => {
                self.parse_state = AnsiParseState::Escape;
            }
            '\r' => {
                self.cursor_col = 0;
            }
            '\n' => {
                let line: String = self.active_line.iter().collect();
                self.push_completed_line(line.clone());
                completed.push(line);
                self.active_line.clear();
                self.cursor_col = 0;
            }
            '\x08' => {
                self.cursor_col = self.cursor_col.saturating_sub(1);
                self.trim_erased_tail();
            }
            '\t' => {
                let next_stop = ((self.cursor_col / 8) + 1) * 8;
                while self.cursor_col < next_stop {
                    self.put_char(' ');
                }
            }
            c if c >= ' ' => {
                self.put_char(c);
            }
            _ => {}
        }
    }

    pub fn feed_bytes(&mut self, bytes: &[u8]) -> (Vec<String>, Option<usize>) {
        self.utf8_tail.extend_from_slice(bytes);
        let mut completed = Vec::new();
        let mut dsr_col = None;

        loop {
            match std::str::from_utf8(&self.utf8_tail) {
                Ok(valid) => {
                    let owned = valid.to_string();
                    self.utf8_tail.clear();
                    for ch in owned.chars() {
                        self.feed_char(ch, &mut completed, &mut dsr_col);
                    }
                    break;
                }
                Err(err) => {
                    let valid_up_to = err.valid_up_to();
                    if valid_up_to > 0 {
                        let valid_part = std::str::from_utf8(&self.utf8_tail[..valid_up_to])
                            .unwrap()
                            .to_string();
                        for ch in valid_part.chars() {
                            self.feed_char(ch, &mut completed, &mut dsr_col);
                        }
                        self.utf8_tail.drain(..valid_up_to);
                    }
                    match err.error_len() {
                        Some(invalid_len) => {
                            self.utf8_tail.drain(..invalid_len);
                        }
                        None => break,
                    }
                }
            }
        }

        (completed, dsr_col)
    }
}

pub fn strip_ansi_and_control_chars(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if c == '\x07' {
                            break;
                        }
                    }
                }
                Some(_) => {
                    chars.next();
                }
                None => {}
            }
            continue;
        }
        if ch == '\t' || ch >= ' ' {
            out.push(ch);
        }
    }
    out
}

pub(crate) fn spawn_stdout_serial_reader<R: std::io::Read + Send + 'static>(
    mut stream: R,
    screen_ref: Arc<Mutex<SerialScreenBuffer>>,
    stdin_ref: Arc<Mutex<Option<ChildStdin>>>,
    log_path: PathBuf,
) {
    thread::spawn(move || {
        let mut log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .ok();
        let mut buf = [0u8; 1024];
        loop {
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let (completed, dsr_col) = {
                        let mut guard = screen_ref.lock().unwrap();
                        guard.feed_bytes(&buf[..n])
                    };
                    if let Some(col) = dsr_col {
                        if let Some(ref mut stdin) = *stdin_ref.lock().unwrap() {
                            let reply = format!("\x1b[1;{}R", col);
                            let _ = stdin.write_all(reply.as_bytes());
                            let _ = stdin.flush();
                        }
                    }
                    if let Some(ref mut f) = log_file {
                        for line in completed {
                            let _ = writeln!(f, "{}", line);
                        }
                    }
                }
            }
        }
    });
}

pub(crate) fn spawn_stderr_serial_reader<R: std::io::Read + Send + 'static>(
    stream: R,
    screen_ref: Arc<Mutex<SerialScreenBuffer>>,
    log_path: PathBuf,
) {
    thread::spawn(move || {
        let reader = BufReader::new(stream);
        let mut log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .ok();
        for line in reader.lines().map_while(Result::ok) {
            let clean = strip_ansi_and_control_chars(line.trim_end_matches('\r'));
            if let Some(ref mut f) = log_file {
                let _ = writeln!(f, "[stderr] {}", clean);
            }
            let mut guard = screen_ref.lock().unwrap();
            guard.push_completed_line(format!("[qemu] {}", clean));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serial_screen_buffer_native_sh_prompt_tab_and_editing() {
        let mut screen = SerialScreenBuffer::with_capacity(64);

        // 1. BusyBox ash prints prompt without trailing newline and queries DSR \x1b[6n
        let (done, dsr) = screen.feed_bytes(b"root@qemu-orion:/# \x1b[6n");
        assert!(done.is_empty());
        assert_eq!(dsr, Some(20));
        let snap = screen.snapshot();
        assert_eq!(snap.active_line, "root@qemu-orion:/# ");
        assert_eq!(snap.cursor_col, 19);

        // 2. Typing "cd /sys/cl" and BusyBox ash completing "ass/" on Tab
        screen.feed_bytes(b"cd /sys/cl");
        screen.feed_bytes(b"ass/");
        assert_eq!(
            screen.snapshot().active_line,
            "root@qemu-orion:/# cd /sys/class/"
        );

        // 3. Backspace (\x08 \x08) erasing '/'
        screen.feed_bytes(b"\x08 \x08");
        assert_eq!(
            screen.snapshot().active_line,
            "root@qemu-orion:/# cd /sys/class"
        );

        // 4. Multi-candidate Tab output (\r\n + candidates + \r\n + reprinted prompt)
        let (done2, _) = screen.feed_bytes(
            b"\r\nbacklight/  graphics/  leds/\r\nroot@qemu-orion:/# cd /sys/class/",
        );
        assert_eq!(
            done2,
            vec![
                "root@qemu-orion:/# cd /sys/class".to_string(),
                "backlight/  graphics/  leds/".to_string()
            ]
        );
        assert_eq!(
            screen.snapshot().active_line,
            "root@qemu-orion:/# cd /sys/class/"
        );

        // 5. History recall (ArrowUp) replacing long command with shorter command via \r + \x1b[K
        screen.feed_bytes(b"\rroot@qemu-orion:/# ps\x1b[K");
        assert_eq!(screen.snapshot().active_line, "root@qemu-orion:/# ps");
        assert_eq!(screen.snapshot().cursor_col, 21);
    }
}
