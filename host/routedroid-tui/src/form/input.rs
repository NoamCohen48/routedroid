//! A one-line text field with a cursor: what a terminal user expects from a
//! field (move, delete under the cursor, paste), counted in characters.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineInput {
    value: String,
    /// In characters, `0..=len`.
    cursor: usize,
}

impl LineInput {
    pub fn new(value: &str) -> Self {
        Self {
            value: value.to_string(),
            cursor: value.chars().count(),
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn insert(&mut self, text: &str) {
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        let at = self.byte(self.cursor);
        self.value.insert_str(at, &text);
        self.cursor += text.chars().count();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.value.remove(self.byte(self.cursor));
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.len() {
            self.value.remove(self.byte(self.cursor));
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.len());
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.len();
    }

    /// Ctrl-U: clear the field.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    fn len(&self) -> usize {
        self.value.chars().count()
    }

    fn byte(&self, chars: usize) -> usize {
        self.value
            .char_indices()
            .nth(chars)
            .map_or(self.value.len(), |(at, _)| at)
    }
}
