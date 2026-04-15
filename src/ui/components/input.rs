use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, Widget};

use crate::ui::messages::Action;
use crate::ui::style::theme;

/// InputBox is a text input overlay with cursor navigation.
pub struct InputBox {
    value: Vec<char>,
    cursor: usize,
    label: String,
    placeholder: String,
    visible: bool,
    scroll_x: usize,
    password_mode: bool,
}

impl Default for InputBox {
    fn default() -> Self {
        Self::new()
    }
}

impl InputBox {
    pub fn new() -> Self {
        InputBox {
            value: Vec::new(),
            cursor: 0,
            label: String::new(),
            placeholder: String::new(),
            visible: false,
            scroll_x: 0,
            password_mode: false,
        }
    }

    pub fn show(&mut self, label: &str, placeholder: &str) {
        self.label = label.to_string();
        self.placeholder = placeholder.to_string();
        self.value.clear();
        self.cursor = 0;
        self.scroll_x = 0;
        self.visible = true;
        self.password_mode = false;
    }

    pub fn show_with_value(&mut self, label: &str, placeholder: &str, initial: &str) {
        self.label = label.to_string();
        self.placeholder = placeholder.to_string();
        self.value = initial.chars().collect();
        self.cursor = self.value.len();
        self.scroll_x = 0;
        self.visible = true;
        self.password_mode = false;
    }

    pub fn show_password(&mut self, label: &str, placeholder: &str) {
        self.show(label, placeholder);
        self.password_mode = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn value(&self) -> String {
        self.value.iter().collect()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        if !self.visible {
            return None;
        }

        match key.code {
            KeyCode::Enter => {
                let value = self.value();
                self.hide();
                Some(Action::InputSubmit(value))
            }
            KeyCode::Esc => {
                self.hide();
                Some(Action::InputCancel)
            }
            KeyCode::Char('h' | 'w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let target = self.prev_word_boundary();
                self.value.drain(target..self.cursor);
                self.cursor = target;
                None
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.value.insert(self.cursor, c);
                self.cursor += 1;
                None
            }
            KeyCode::Backspace => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    let target = self.prev_word_boundary();
                    self.value.drain(target..self.cursor);
                    self.cursor = target;
                } else if self.cursor > 0 {
                    self.cursor -= 1;
                    self.value.remove(self.cursor);
                }
                None
            }
            KeyCode::Delete => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    let target = self.next_word_boundary();
                    self.value.drain(self.cursor..target);
                } else if self.cursor < self.value.len() {
                    self.value.remove(self.cursor);
                }
                None
            }
            KeyCode::Left => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.cursor = self.prev_word_boundary();
                } else if self.cursor > 0 {
                    self.cursor -= 1;
                }
                None
            }
            KeyCode::Right => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.cursor = self.next_word_boundary();
                } else if self.cursor < self.value.len() {
                    self.cursor += 1;
                }
                None
            }
            KeyCode::Home => {
                self.cursor = 0;
                None
            }
            KeyCode::End => {
                self.cursor = self.value.len();
                None
            }
            _ => None,
        }
    }

    fn prev_word_boundary(&self) -> usize {
        if self.cursor == 0 {
            return 0;
        }
        let mut pos = self.cursor - 1;
        while pos > 0 && self.value[pos] == ' ' {
            pos -= 1;
        }
        while pos > 0 && self.value[pos - 1] != ' ' {
            pos -= 1;
        }
        pos
    }

    fn next_word_boundary(&self) -> usize {
        let len = self.value.len();
        if self.cursor >= len {
            return len;
        }
        let mut pos = self.cursor;
        while pos < len && self.value[pos] != ' ' {
            pos += 1;
        }
        while pos < len && self.value[pos] == ' ' {
            pos += 1;
        }
        pos
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if !self.visible {
            return;
        }

        Clear.render(area, buf);

        let block = Block::default()
            .title(format!(" {} ", self.label))
            .borders(Borders::ALL)
            .border_style(
                Style::default()
                    .fg(theme::color_primary())
                    .add_modifier(Modifier::BOLD),
            );
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height == 0 || inner.width < 4 {
            return;
        }

        let field_w = inner.width.saturating_sub(2) as usize;
        let y_label = inner.y;
        let y_field = inner.y + 2;
        let y_hint = inner.y + inner.height.saturating_sub(1);

        let hint_style = Style::default().fg(theme::color_muted());
        buf.set_string(
            inner.x + 1,
            y_hint,
            "Enter: submit  Esc: cancel",
            hint_style,
        );

        if self.value.is_empty() {
            let placeholder_style = Style::default().fg(theme::color_muted());
            buf.set_string(inner.x + 1, y_field, &self.placeholder, placeholder_style);
            let cursor_style = Style::default()
                .fg(theme::color_bright())
                .add_modifier(Modifier::REVERSED);
            buf.set_string(inner.x + 1, y_field, " ", cursor_style);
            return;
        }

        let scroll = if self.cursor < self.scroll_x {
            self.cursor
        } else if self.cursor >= self.scroll_x + field_w {
            self.cursor - field_w + 1
        } else {
            self.scroll_x
        };

        let text_style = Style::default().fg(theme::color_bright());
        let cursor_style = Style::default()
            .fg(theme::color_background())
            .bg(theme::color_primary());

        let visible_chars: Vec<char> = self
            .value
            .iter()
            .skip(scroll)
            .take(field_w)
            .copied()
            .collect();

        let field_bg = Style::default()
            .fg(theme::color_text())
            .bg(theme::color_secondary());
        let bg_fill: String = " ".repeat(field_w);
        buf.set_string(inner.x + 1, y_field, &bg_fill, field_bg);

        for (i, &ch) in visible_chars.iter().enumerate() {
            let abs_pos = i + scroll;
            let x = inner.x + 1 + i as u16;
            let display_ch = if self.password_mode { '*' } else { ch };
            if abs_pos == self.cursor {
                buf.set_string(x, y_field, display_ch.to_string(), cursor_style);
            } else {
                buf.set_string(
                    x,
                    y_field,
                    display_ch.to_string(),
                    text_style.bg(theme::color_secondary()),
                );
            }
        }

        if self.cursor >= scroll + visible_chars.len() && self.cursor == self.value.len() {
            let x = inner.x + 1 + (self.cursor - scroll).min(field_w.saturating_sub(1)) as u16;
            buf.set_string(x, y_field, " ", cursor_style);
        }

        let pos_text = format!(" {}/{} ", self.cursor, self.value.len());
        let pos_style = Style::default().fg(theme::color_muted());
        let pos_x = inner.x + inner.width.saturating_sub(pos_text.len() as u16 + 1);
        buf.set_string(pos_x, y_label, &pos_text, pos_style);
    }

    pub fn view(&self) -> String {
        if !self.visible {
            return String::new();
        }
        format!("{}\n\n{}", self.label, self.value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn submit_returns_value() {
        let mut b = InputBox::new();
        b.show("Filter", "type here");
        b.handle_key(key(KeyCode::Char('h')));
        b.handle_key(key(KeyCode::Char('i')));
        let result = b.handle_key(key(KeyCode::Enter));
        match result {
            Some(Action::InputSubmit(v)) => assert_eq!(v, "hi"),
            other => panic!("expected InputSubmit, got {other:?}"),
        }
    }

    #[test]
    fn cancel_returns_action() {
        let mut b = InputBox::new();
        b.show("Filter", "");
        let result = b.handle_key(key(KeyCode::Esc));
        match result {
            Some(Action::InputCancel) => {}
            other => panic!("expected InputCancel, got {other:?}"),
        }
    }

    #[test]
    fn cursor_navigation() {
        let mut b = InputBox::new();
        b.show_with_value("Test", "", "hello world");
        assert_eq!(b.cursor, 11);

        b.handle_key(key(KeyCode::Home));
        assert_eq!(b.cursor, 0);

        b.handle_key(key(KeyCode::End));
        assert_eq!(b.cursor, 11);
    }
}
