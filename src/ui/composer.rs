use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Debug, Default, Clone)]
pub struct Composer {
    text: String,
    cursor: usize,
}

impl Composer {
    pub fn input(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn remove_range(&mut self, start: usize, end: usize) {
        if start >= end {
            return;
        }

        let start = start.min(self.input_len());
        let end = end.min(self.input_len());
        if start >= end {
            return;
        }

        let byte_start = char_to_byte_idx(&self.text, start);
        let byte_end = char_to_byte_idx(&self.text, end);
        self.text.replace_range(byte_start..byte_end, "");

        let removed_len = end - start;
        self.cursor = if self.cursor <= start {
            self.cursor
        } else if self.cursor >= end {
            self.cursor - removed_len
        } else {
            start
        };
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn insert_paste(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        self.insert_str(&text.replace(['\r', '\n', '\t'], " "));
        true
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return false;
        }

        if key_pressed(key, KeyCode::Char('a'), KeyModifiers::CONTROL) {
            self.move_to_start();
            return true;
        }
        if key_pressed(key, KeyCode::Char('e'), KeyModifiers::CONTROL) {
            self.move_to_end();
            return true;
        }

        match key.code {
            KeyCode::Backspace => {
                self.delete_backward_char();
                true
            }
            KeyCode::Delete => {
                self.delete_forward_char();
                true
            }
            KeyCode::Left => {
                self.move_left();
                true
            }
            KeyCode::Right => {
                self.move_right();
                true
            }
            KeyCode::Home => {
                self.move_to_start();
                true
            }
            KeyCode::End => {
                self.move_to_end();
                true
            }
            KeyCode::Char(ch) if is_plain_text_modifier(key.modifiers) => {
                self.insert_char(ch);
                true
            }
            _ => false,
        }
    }

    fn input_len(&self) -> usize {
        self.text.chars().count()
    }

    fn insert_char(&mut self, ch: char) {
        let byte_idx = char_to_byte_idx(&self.text, self.cursor);
        self.text.insert(byte_idx, ch);
        self.cursor += 1;
    }

    fn insert_str(&mut self, value: &str) {
        for ch in value.chars() {
            self.insert_char(ch);
        }
    }

    fn delete_backward_char(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = char_to_byte_idx(&self.text, self.cursor - 1);
        let end = char_to_byte_idx(&self.text, self.cursor);
        self.text.replace_range(start..end, "");
        self.cursor -= 1;
    }

    fn delete_forward_char(&mut self) {
        if self.cursor >= self.input_len() {
            return;
        }
        let start = char_to_byte_idx(&self.text, self.cursor);
        let end = char_to_byte_idx(&self.text, self.cursor + 1);
        self.text.replace_range(start..end, "");
    }

    fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.input_len());
    }

    fn move_to_start(&mut self) {
        self.cursor = 0;
    }

    fn move_to_end(&mut self) {
        self.cursor = self.input_len();
    }
}

fn key_pressed(key: KeyEvent, code: KeyCode, modifiers: KeyModifiers) -> bool {
    key.code == code && key.modifiers.contains(modifiers)
}

fn is_plain_text_modifier(modifiers: KeyModifiers) -> bool {
    !modifiers.intersects(
        KeyModifiers::CONTROL
            | KeyModifiers::ALT
            | KeyModifiers::SUPER
            | KeyModifiers::HYPER
            | KeyModifiers::META,
    )
}

fn char_to_byte_idx(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}
