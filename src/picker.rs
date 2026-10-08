//! Picker state is independent of the webview and host clipboard.
use crate::core::Engine;
use serde::Serialize;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub hotkeys: crate::settings::Hotkeys,
    pub registers: Vec<Option<String>>,
    pub register_names: Vec<String>,
    pub current_clipboard: Option<String>,
    pub history: Vec<String>,
    pub status: String,
    pub copy: bool,
    pub visible: bool,
    pub selection: Option<usize>,
}

pub struct Picker {
    pub hotkeys: crate::settings::Hotkeys,
    pub engine: Engine,
    pub current_clipboard: Option<String>,
    pub status: String,
    pub copy: bool,
    pub visible: bool,
    pub target: usize,
    pub pinned: bool,
    pub selection: Option<usize>,
    navigation: Vec<String>,
}
impl Picker {
    pub fn new(target: usize) -> Self {
        Self {
            hotkeys: crate::settings::Hotkeys::default(),
            engine: Engine::default(),
            current_clipboard: None,
            status: String::new(),
            copy: false,
            visible: false,
            target,
            pinned: false,
            selection: None,
            navigation: Vec::new(),
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            hotkeys: self.hotkeys.clone(),
            registers: self.engine.registers().to_vec(),
            register_names: self.engine.register_names().to_vec(),
            current_clipboard: self.current_clipboard.clone(),
            history: self.engine.history().to_vec(),
            status: self.status.clone(),
            copy: self.copy,
            visible: self.visible,
            selection: self.selection,
        }
    }
    pub fn refresh(&mut self, engine: Engine) {
        let selected = self.selection.and_then(|i| self.navigation.get(i)).cloned();
        self.engine = engine;
        self.refresh_navigation();
        if let Some(text) = selected {
            self.selection = self.navigation.iter().position(|item| item == &text);
        }
    }
    fn refresh_navigation(&mut self) {
        self.navigation = self
            .engine
            .registers()
            .iter()
            .filter_map(Clone::clone)
            .chain(self.engine.history().iter().cloned())
            .collect();
    }
    pub fn pin_editor(&mut self) {
        self.pinned = true;
        self.clear_selection();
    }
    pub fn end_editor(&mut self) {
        self.pinned = false;
    }
    pub fn shortcut_can_dismiss(&self, target: usize) -> bool {
        self.visible && self.target == target && !self.pinned
    }
    pub fn show(&mut self, copy: bool, target: usize) {
        self.pinned = false;
        self.copy = copy;
        self.target = target;
        self.visible = true;
        self.selection = None;
        self.refresh_navigation();
    }
    pub fn navigate(&mut self, backwards: bool) {
        let count = self.navigation.len();
        if count == 0 {
            return;
        }
        self.selection = Some(match self.selection {
            None if backwards => count - 1,
            None => 0,
            Some(i) if backwards => (i + count - 1) % count,
            Some(i) => (i + 1) % count,
        });
    }
    pub fn dismiss(&mut self, commit: bool) -> Option<String> {
        let text = if commit {
            self.selection.and_then(|i| self.navigation.get(i)).cloned()
        } else {
            None
        };
        self.pinned = false;
        self.visible = false;
        self.copy = false;
        self.selection = None;
        text
    }
    pub fn clear_selection(&mut self) {
        self.selection = None;
        self.navigation.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editor_survives_modifier_release_and_never_commits_a_prior_selection() {
        let mut p = Picker::new(42);
        p.engine.observe("previously selected");
        p.show(false, 42);
        p.navigate(false);
        p.pin_editor();
        assert!(!p.shortcut_can_dismiss(42));
        assert!(p.visible);
        assert_eq!(p.selection, None);
        p.end_editor();
        assert!(p.shortcut_can_dismiss(42));
        assert!(!p.shortcut_can_dismiss(43));
        assert_eq!(p.dismiss(true), None);
        assert!(!p.pinned);
    }

    #[test]
    fn release_loads_selected_text_and_escape_cancels() {
        let mut p = Picker::new(42);
        p.engine.save_register('x', "saved text").unwrap();
        p.show(false, 42);
        assert_eq!(p.dismiss(true), None);
        p.show(false, 42);
        p.navigate(false);
        assert_eq!(p.dismiss(true).as_deref(), Some("saved text"));
        assert!(!p.visible);
        p.show(false, 42);
        p.navigate(false);
        assert_eq!(p.dismiss(false), None);
    }
    #[test]
    fn refresh_keeps_selected_content_and_clear_cannot_resurrect_it() {
        let mut p = Picker::new(42);
        p.engine.observe("chosen");
        p.show(false, 42);
        p.navigate(false);
        let mut updated = p.engine.clone();
        updated.observe("new clipboard");
        p.refresh(updated);
        assert_eq!(p.dismiss(true).as_deref(), Some("chosen"));
        p.show(false, 42);
        p.navigate(false);
        p.clear_selection();
        assert_eq!(p.dismiss(true), None);
    }

    #[test]
    fn history_refresh_remaps_highlight_to_the_text_that_will_be_loaded() {
        let mut p = Picker::new(42);
        p.engine.observe("chosen 🌍\nexact text");
        p.show(false, 42);
        p.navigate(false);
        assert_eq!(p.snapshot().selection, Some(0));
        let mut updated = p.engine.clone();
        updated.observe("new clipboard");
        p.refresh(updated);
        let snapshot = p.snapshot();
        assert_eq!(snapshot.selection, Some(1));
        assert_eq!(
            snapshot.history[snapshot.selection.unwrap()],
            "chosen 🌍\nexact text"
        );
        assert_eq!(p.dismiss(true).as_deref(), Some("chosen 🌍\nexact text"));
    }

    #[test]
    fn evicted_history_selection_cannot_load_an_unhighlighted_old_entry() {
        let mut p = Picker::new(42);
        p.engine = Engine::new(1);
        p.engine.observe("chosen");
        p.show(false, 42);
        p.navigate(false);
        let mut updated = p.engine.clone();
        updated.observe("replacement");
        p.refresh(updated);
        assert_eq!(p.snapshot().history, vec!["replacement"]);
        assert_eq!(p.snapshot().selection, None);
        assert_eq!(p.dismiss(true), None);
    }

    #[test]
    fn register_insertion_remaps_history_selection_past_populated_registers() {
        let mut p = Picker::new(42);
        p.engine.observe("chosen");
        p.show(false, 42);
        p.navigate(false);
        let mut updated = p.engine.clone();
        updated.save_register('a', "new register").unwrap();
        p.refresh(updated);
        let snapshot = p.snapshot();
        assert_eq!(snapshot.selection, Some(2));
        let displayed: Vec<_> = snapshot
            .registers
            .iter()
            .filter_map(|text| text.as_deref())
            .chain(snapshot.history.iter().map(String::as_str))
            .collect();
        assert_eq!(displayed[snapshot.selection.unwrap()], "chosen");
        assert_eq!(p.dismiss(true).as_deref(), Some("chosen"));
    }
}
