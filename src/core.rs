//! Text registers and a bounded clipboard ring, independent of the host OS.

pub const REGISTER_COUNT: usize = 26;
pub const DEFAULT_HISTORY_LIMIT: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    InvalidRegister(char),
    EmptyRegister(char),
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRegister(key) => write!(f, "Invalid register '{key}'; use a–z"),
            Self::EmptyRegister(key) => write!(f, "Register '{key}' is empty"),
        }
    }
}

impl std::error::Error for RegisterError {}

#[derive(Debug, Clone)]
pub struct Engine {
    registers: [Option<String>; REGISTER_COUNT],
    register_names: [String; REGISTER_COUNT],
    history: Vec<String>,
    history_limit: usize,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(DEFAULT_HISTORY_LIMIT)
    }
}

impl Engine {
    pub fn new(history_limit: usize) -> Self {
        Self {
            registers: std::array::from_fn(|_| None),
            register_names: std::array::from_fn(|_| String::new()),
            history: Vec::new(),
            history_limit,
        }
    }

    pub fn save_register(&mut self, key: char, text: &str) -> Result<(), RegisterError> {
        let index = register_index(key)?;
        self.registers[index] = Some(text.to_owned());
        self.observe(text);
        Ok(())
    }

    pub fn load_register(&mut self, key: char) -> Result<String, RegisterError> {
        let index = register_index(key)?;
        let text = self.registers[index]
            .clone()
            .ok_or(RegisterError::EmptyRegister((b'a' + index as u8) as char))?;
        self.observe(&text);
        Ok(text)
    }

    /// Edit a register's label and exact text together. Invalid keys change nothing.
    pub fn edit_register(
        &mut self,
        key: char,
        name: &str,
        text: &str,
    ) -> Result<(), RegisterError> {
        let index = register_index(key)?;
        self.register_names[index] = name.to_owned();
        self.registers[index] = Some(text.to_owned());
        self.observe(text);
        Ok(())
    }

    /// Remove one register and its label without changing the clipboard ring.
    pub fn clear_register(&mut self, key: char) -> Result<(), RegisterError> {
        let index = register_index(key)?;
        self.register_names[index].clear();
        self.registers[index] = None;
        Ok(())
    }

    /// Record a nonempty text value without changing any named register.
    pub fn observe(&mut self, text: &str) {
        if text.is_empty() || self.history_limit == 0 {
            return;
        }
        if self.history.first().is_some_and(|first| first == text) {
            return;
        }
        self.history.retain(|entry| entry != text);
        self.history.insert(0, text.to_owned());
        self.history.truncate(self.history_limit);
    }

    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Index zero is register a; index 25 is register z.
    pub fn registers(&self) -> &[Option<String>; REGISTER_COUNT] {
        &self.registers
    }

    pub fn register_names(&self) -> &[String; REGISTER_COUNT] {
        &self.register_names
    }

    /// Clear recent copies while retaining register contents and labels.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Clear all register contents and labels while retaining recent copies.
    pub fn clear_registers(&mut self) {
        self.registers.iter_mut().for_each(|entry| *entry = None);
        self.register_names.iter_mut().for_each(String::clear);
    }

    pub fn clear(&mut self) {
        self.clear_registers();
        self.clear_history();
    }
}

pub fn register_index(key: char) -> Result<usize, RegisterError> {
    if key.is_ascii_alphabetic() {
        Ok((key.to_ascii_lowercase() as u8 - b'a') as usize)
    } else {
        Err(RegisterError::InvalidRegister(key))
    }
}

/// A Unicode-safe two-line summary. Stored content is never modified.
pub fn preview(text: &str) -> String {
    const MAX_LINE_CHARS: usize = 100;
    let mut lines = text.lines();
    let mut result = Vec::new();
    for line in lines.by_ref().take(2) {
        let mut chars = line.chars();
        let mut short: String = chars.by_ref().take(MAX_LINE_CHARS).collect();
        if chars.next().is_some() {
            short.push('…');
        }
        result.push(short);
    }
    if lines.next().is_some()
        && let Some(last) = result.last_mut()
    {
        last.push('…');
    }
    result.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearing_registers_preserves_history_and_allows_reuse() {
        let mut engine = Engine::default();
        engine.edit_register('a', "First", "one").unwrap();
        engine.edit_register('z', "Last", "two").unwrap();
        let history = engine.history().to_vec();
        engine.clear_registers();
        engine.clear_registers();
        assert!(engine.registers().iter().all(Option::is_none));
        assert!(engine.register_names().iter().all(String::is_empty));
        assert_eq!(engine.history(), history);
        engine.edit_register('a', "New", "new text").unwrap();
        assert_eq!(engine.registers()[0].as_deref(), Some("new text"));
    }

    #[test]
    fn clearing_history_preserves_registers_and_accepts_new_copies() {
        let mut engine = Engine::default();
        engine.edit_register('a', "Keep name", "Keep text").unwrap();
        engine.observe("Recent copy");
        engine.clear_history();
        engine.clear_history();
        assert!(engine.history().is_empty());
        assert_eq!(engine.registers()[0].as_deref(), Some("Keep text"));
        assert_eq!(engine.register_names()[0], "Keep name");
        engine.observe("Next copy");
        assert_eq!(engine.history(), &["Next copy"]);
    }

    #[test]
    fn only_ascii_letters_are_registers_and_case_is_shared() {
        let mut engine = Engine::default();
        engine.save_register('X', "hello 🌍\n世界").unwrap();
        assert_eq!(engine.load_register('x').unwrap(), "hello 🌍\n世界");
        engine.save_register('x', "replacement").unwrap();
        assert_eq!(engine.load_register('X').unwrap(), "replacement");
        for key in ['0', 'é', ' ', '['] {
            assert_eq!(
                engine.save_register(key, "bad"),
                Err(RegisterError::InvalidRegister(key))
            );
        }
        assert_eq!(
            engine.load_register('A'),
            Err(RegisterError::EmptyRegister('a'))
        );
    }

    #[test]
    fn history_is_bounded_unique_and_loading_promotes_without_overwriting_registers() {
        let mut engine = Engine::new(3);
        engine.save_register('a', "saved").unwrap();
        for text in ["one", "two", "three", "two"] {
            engine.observe(text);
        }
        assert_eq!(engine.history(), &["two", "three", "one"]);
        assert_eq!(engine.load_register('a').unwrap(), "saved");
        assert_eq!(engine.history(), &["saved", "two", "three"]);
        engine.observe("saved");
        assert_eq!(engine.history().len(), 3);
        assert_eq!(engine.registers()[0].as_deref(), Some("saved"));
    }

    #[test]
    fn clear_removes_everything_and_empty_observations_do_not_create_history() {
        let mut engine = Engine::default();
        engine.observe("");
        assert!(engine.history().is_empty());
        engine.save_register('z', "text").unwrap();
        engine.clear();
        assert!(engine.registers().iter().all(Option::is_none));
        assert!(engine.history().is_empty());
        let mut disabled = Engine::new(0);
        disabled.save_register('a', "still saved").unwrap();
        assert!(disabled.history().is_empty());
        assert_eq!(disabled.load_register('a').unwrap(), "still saved");
    }

    #[test]
    fn preview_bounds_lines_and_handles_unicode_without_changing_source() {
        assert_eq!(preview("first\r\nsecond\nthird"), "first\nsecond…");
        assert_eq!(preview(&"🌍".repeat(101)), format!("{}…", "🌍".repeat(100)));
        assert_eq!(preview(""), "");
    }

    #[test]
    fn editing_register_preserves_exact_unicode_metadata_and_quick_copy_keeps_name() {
        let mut engine = Engine::default();
        let name = "VM 世界 🌍 ";
        let text = "first\r\n第二行\nthird\t  ";
        engine.edit_register('X', name, text).unwrap();
        assert_eq!(engine.register_names()[23], name);
        assert_eq!(engine.load_register('x').unwrap(), text);
        engine.save_register('x', "quick copy replacement").unwrap();
        assert_eq!(engine.register_names()[23], name);
        assert_eq!(engine.load_register('X').unwrap(), "quick copy replacement");
        engine.edit_register('x', "", "").unwrap();
        assert_eq!(engine.register_names()[23], "");
        assert_eq!(engine.registers()[23].as_deref(), Some(""));
    }

    #[test]
    fn clear_register_removes_only_its_text_and_name_without_modifying_history() {
        let mut engine = Engine::default();
        engine.edit_register('a', "first", "one").unwrap();
        engine.edit_register('b', "second", "two").unwrap();
        let history = engine.history().to_vec();
        engine.clear_register('A').unwrap();
        assert_eq!(engine.registers()[0], None);
        assert_eq!(engine.register_names()[0], "");
        assert_eq!(engine.registers()[1].as_deref(), Some("two"));
        assert_eq!(engine.register_names()[1], "second");
        assert_eq!(engine.history(), history);
        engine.clear_register('a').unwrap();
        assert_eq!(engine.history(), history);
        engine.clear();
        assert!(engine.register_names().iter().all(String::is_empty));
        assert!(engine.registers().iter().all(Option::is_none));
        assert!(engine.history().is_empty());
    }

    #[test]
    fn invalid_edit_and_clear_are_atomic_for_metadata_text_and_history() {
        let mut engine = Engine::default();
        engine
            .edit_register('z', "keep label", "keep contents")
            .unwrap();
        let before = engine.clone();
        for key in ['0', 'é', '['] {
            assert_eq!(
                engine.edit_register(key, "bad", "bad"),
                Err(RegisterError::InvalidRegister(key))
            );
            assert_eq!(
                engine.clear_register(key),
                Err(RegisterError::InvalidRegister(key))
            );
        }
        assert_eq!(engine.register_names(), before.register_names());
        assert_eq!(engine.registers(), before.registers());
        assert_eq!(engine.history(), before.history());
    }
}
