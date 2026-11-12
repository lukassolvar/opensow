//! A one-line text field (the map editor's map name, docs/map-editor-ingame.md): characters arrive as the window's composed text
//! (dead keys and the Czech / Slovak layouts give `á`, `č` ... directly; an input method commits whole strings), the caret moves
//! with the arrows, Home and End, Backspace and Delete remove.

/// A one-line text field: text, caret and focus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextField {
    /// The text.
    pub text: String,
    /// Caret position in characters.
    pub caret: usize,
    /// The field takes the keyboard.
    pub focused: bool,
    /// Longest text kept, in characters.
    pub max: usize,
}

/// An editing key of the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
}

impl TextField {
    /// A field holding `text`, cut to `max` characters.
    pub fn new(text: &str, max: usize) -> TextField {
        let text: String = text.chars().take(max).collect();
        TextField { caret: text.chars().count(), text, focused: false, max }
    }

    fn byte(&self, char_index: usize) -> usize {
        self.text.char_indices().nth(char_index).map_or(self.text.len(), |(b, _)| b)
    }

    /// Inserts typed or committed text at the caret (control characters are dropped, the length is capped); `true` when it changed.
    pub fn insert(&mut self, s: &str) -> bool {
        let room = self.max.saturating_sub(self.text.chars().count());
        let add: String = s.chars().filter(|c| !c.is_control()).take(room).collect();
        if add.is_empty() {
            return false;
        }
        let at = self.byte(self.caret);
        self.text.insert_str(at, &add);
        self.caret += add.chars().count();
        true
    }

    /// Applies an editing key; `true` when the text or the caret changed.
    pub fn edit(&mut self, e: Edit) -> bool {
        let n = self.text.chars().count();
        let before = (self.text.clone(), self.caret);
        match e {
            Edit::Backspace if self.caret > 0 => {
                let (a, b) = (self.byte(self.caret - 1), self.byte(self.caret));
                self.text.replace_range(a..b, "");
                self.caret -= 1;
            }
            Edit::Delete if self.caret < n => {
                let (a, b) = (self.byte(self.caret), self.byte(self.caret + 1));
                self.text.replace_range(a..b, "");
            }
            Edit::Left => self.caret = self.caret.saturating_sub(1),
            Edit::Right => self.caret = (self.caret + 1).min(n),
            Edit::Home => self.caret = 0,
            Edit::End => self.caret = n,
            _ => {}
        }
        (self.text.clone(), self.caret) != before
    }

    /// The text before the caret (to place the caret when drawing).
    pub fn before_caret(&self) -> &str {
        &self.text[..self.byte(self.caret)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_moves_and_deletes_by_characters() {
        let mut f = TextField::new("", 10);
        assert!(f.insert("Žltá"));
        assert!(f.insert(" púšť\n"));
        assert_eq!((f.text.as_str(), f.caret), ("Žltá púšť", 9), "the newline is dropped");
        assert!(f.edit(Edit::Home) && f.edit(Edit::Right));
        assert!(f.edit(Edit::Delete));
        assert_eq!(f.text, "Žtá púšť");
        assert!(f.edit(Edit::Backspace));
        assert_eq!((f.text.as_str(), f.caret), ("tá púšť", 0));
        assert!(!f.edit(Edit::Backspace) && !f.edit(Edit::Left));
        assert!(f.edit(Edit::End));
        assert_eq!(f.before_caret(), "tá púšť");
        assert!(f.insert("1234567"));
        assert_eq!(f.text.chars().count(), 10, "capped at the maximum");
        assert!(!f.insert("x"));
        assert!(TextField::new("双河地图", 3).text == "双河地");
    }
}
