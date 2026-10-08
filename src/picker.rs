//! Picker state is independent of the webview and host clipboard.
use crate::core::Engine;
use serde::Serialize;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub history_entries: Vec<crate::content::EntryView>,
    pub current_entry: Option<crate::content::EntryView>,
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
    pub current_entry: Option<crate::content::EntryView>,
    pub hotkeys: crate::settings::Hotkeys,
    pub engine: Engine,
    pub current_clipboard: Option<String>,
    pub status: String,
    pub copy: bool,
    pub visible: bool,
    pub target: usize,
    pub pinned: bool,
    pub selection: Option<usize>,
    navigation: Vec<Target>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Text(String),
    Entry(u64),
}
impl Picker {
    pub fn new(target: usize) -> Self {
        Self {
            current_entry: None,
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
            history_entries: self
                .engine
                .entries()
                .iter()
                .map(|entry| entry.view.clone())
                .collect(),
            current_entry: self.current_entry.clone(),
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
            .map(Target::Text)
            .chain(self.engine.entries().iter().map(|entry| {
                if entry.content.is_some() {
                    Target::Entry(entry.view.id)
                } else {
                    Target::Text(entry.view.text.clone().unwrap_or_default())
                }
            }))
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
        match self.dismiss_target(commit) {
            Some(Target::Text(text)) => Some(text),
            _ => None,
        }
    }
    pub fn dismiss_target(&mut self, commit: bool) -> Option<Target> {
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
    fn mixed_keyboard_selection_restores_entry_id_and_eviction_cancels_selection() {
        use crate::content::{Format, describe};
        let mut picker = Picker::new(0);
        let content = describe(vec![Format {
            id: 49152,
            name: "Binary".into(),
            bytes: vec![1, 2, 3],
        }]);
        picker.engine.observe_content(content.clone());
        let id = picker.engine.entries()[0].view.id;
        picker.show(false, 0);
        picker.navigate(false);
        let mut engine = picker.engine.clone();
        engine.observe("new text");
        picker.refresh(engine);
        assert_eq!(picker.selection, Some(1));
        assert_eq!(picker.dismiss_target(true), Some(Target::Entry(id)));
        picker.show(false, 0);
        picker.navigate(true);
        let mut engine = picker.engine.clone();
        engine.clear_history();
        picker.refresh(engine);
        assert_eq!(picker.selection, None);
        assert_eq!(picker.dismiss_target(true), None);
    }
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
