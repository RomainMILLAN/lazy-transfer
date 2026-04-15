use crossterm::event::{KeyCode, KeyEvent};

/// Choice represents a single option in a ChoiceDialog.
#[derive(Debug, Clone)]
pub struct Choice {
    pub key: char,
    pub label: String,
}

/// ChoiceDialog shows a dialog with multiple keyed options.
pub struct ChoiceDialog {
    message: String,
    choices: Vec<Choice>,
    visible: bool,
    selected_key: Option<char>,
}

impl Default for ChoiceDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl ChoiceDialog {
    pub fn new() -> Self {
        ChoiceDialog {
            message: String::new(),
            choices: vec![],
            visible: false,
            selected_key: None,
        }
    }

    pub fn show(&mut self, message: &str, choices: Vec<Choice>) {
        self.message = message.to_string();
        self.choices = choices;
        self.visible = true;
        self.selected_key = None;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.choices.clear();
        self.selected_key = None;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn selected_key(&self) -> Option<char> {
        self.selected_key
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<char> {
        if !self.visible {
            return None;
        }

        match key.code {
            KeyCode::Esc => {
                self.visible = false;
                self.selected_key = None;
                Some('\x1b')
            }
            KeyCode::Char(c) => {
                for ch in &self.choices {
                    if c == ch.key {
                        self.visible = false;
                        self.selected_key = Some(c);
                        return Some(c);
                    }
                }
                None
            }
            _ => None,
        }
    }

    pub fn view(&self) -> String {
        if !self.visible {
            return String::new();
        }

        let mut b = String::new();
        b.push_str(&self.message);
        b.push('\n');

        for ch in &self.choices {
            b.push_str(&format!("\n[{}] {}", ch.key, ch.label));
        }

        b.push_str("\n\n[esc] cancel");
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn select_choice() {
        let mut d = ChoiceDialog::new();
        d.show(
            "Pick one",
            vec![
                Choice {
                    key: 'a',
                    label: "Alpha".to_string(),
                },
                Choice {
                    key: 'b',
                    label: "Beta".to_string(),
                },
            ],
        );
        let result = d.handle_key(key(KeyCode::Char('a')));
        assert_eq!(result, Some('a'));
        assert!(!d.is_visible());
    }

    #[test]
    fn cancel_with_esc() {
        let mut d = ChoiceDialog::new();
        d.show("Pick", vec![]);
        let result = d.handle_key(key(KeyCode::Esc));
        assert_eq!(result, Some('\x1b'));
    }
}
