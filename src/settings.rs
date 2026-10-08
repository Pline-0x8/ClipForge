//! Only shortcuts are persisted; clipboard contents never reach disk.
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const ALT: u32 = 1;
pub const CTRL: u32 = 2;
pub const SHIFT: u32 = 4;
pub const SUPER: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub modifiers: u32,
    pub key: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Hotkeys {
    pub menu: String,
    pub copy: String,
    pub paste: String,
}
impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            menu: "Ctrl+Alt+Space".into(),
            copy: "Ctrl+Alt+C".into(),
            paste: "Ctrl+Alt+V".into(),
        }
    }
}
impl Hotkeys {
    pub fn parsed(&self) -> Result<[Shortcut; 3], String> {
        let values = [&self.menu, &self.copy, &self.paste];
        let labels = ["Menu", "Register copy", "Register paste"];
        let mut result = [Shortcut {
            modifiers: 0,
            key: 0,
        }; 3];
        for i in 0..3 {
            result[i] = parse(values[i]).map_err(|e| format!("{}: {e}", labels[i]))?;
            if result[..i].contains(&result[i]) {
                return Err("Each action must use a different shortcut".into());
            }
        }
        Ok(result)
    }
    pub fn normalized(&self) -> Result<Self, String> {
        let [menu, copy, paste] = self.parsed()?.map(|s| s.label());
        Ok(Self { menu, copy, paste })
    }
}
impl Shortcut {
    pub fn label(self) -> String {
        let mut parts = Vec::new();
        for (flag, name) in [
            (CTRL, "Ctrl"),
            (ALT, "Alt"),
            (SHIFT, "Shift"),
            (SUPER, "Super"),
        ] {
            if self.modifiers & flag != 0 {
                parts.push(name.to_owned());
            }
        }
        parts.push(match self.key {
            0x20 => "Space".into(),
            0x70..=0x87 => format!("F{}", self.key - 0x70 + 1),
            key => char::from_u32(u32::from(key)).unwrap().to_string(),
        });
        parts.join("+")
    }
}
pub fn parse(value: &str) -> Result<Shortcut, String> {
    let mut modifiers = 0;
    let mut key = None;
    for part in value.split('+').map(str::trim) {
        let lower = part.to_ascii_lowercase();
        let flag = match lower.as_str() {
            "ctrl" | "control" => CTRL,
            "alt" => ALT,
            "shift" => SHIFT,
            "super" | "win" | "command" | "cmd" => SUPER,
            _ => 0,
        };
        if flag != 0 {
            if modifiers & flag != 0 {
                return Err("Duplicate modifier".into());
            }
            modifiers |= flag;
            continue;
        }
        let code = if lower == "space" {
            0x20
        } else if lower.len() == 1 && lower.as_bytes()[0].is_ascii_alphanumeric() {
            u16::from(lower.as_bytes()[0].to_ascii_uppercase())
        } else if let Some(number) = lower
            .strip_prefix('f')
            .and_then(|n| n.parse::<u16>().ok())
            .filter(|n| (1..=24).contains(n))
        {
            0x70 + number - 1
        } else {
            return Err(
                "Use Ctrl, Alt, Shift or Super plus a letter, digit, Space or F1–F24".into(),
            );
        };
        if key.replace(code).is_some() {
            return Err("Use exactly one activation key".into());
        }
    }
    if modifiers & (CTRL | ALT | SUPER) == 0 {
        return Err("Include Ctrl, Alt or Super".into());
    }
    let key = key.ok_or("Choose an activation key")?;
    if modifiers == CTRL && matches!(key, 0x41 | 0x43 | 0x56 | 0x58 | 0x59 | 0x5a)
        || modifiers == ALT && key == 0x73
        || modifiers & SUPER != 0 && matches!(key, 0x4c | 0x55)
    {
        return Err("This shortcut is reserved for standard editing or system actions".into());
    }
    Ok(Shortcut { modifiers, key })
}
pub fn load(path: &Path) -> Result<Hotkeys, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<Hotkeys>(&bytes)
            .map_err(|e| format!("Invalid hotkey settings: {e}"))?
            .normalized(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Hotkeys::default()),
        Err(e) => Err(format!("Could not read hotkey settings: {e}")),
    }
}
pub fn save(path: &Path, hotkeys: &Hotkeys) -> Result<(), String> {
    let save = || -> Result<(), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(path.parent().ok_or("Missing settings directory")?)?;
        let temp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(&hotkeys.normalized()?)?;
        use std::io::Write;
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            };
            let from: Vec<_> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
            let to: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            if unsafe {
                MoveFileExW(
                    from.as_ptr(),
                    to.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        #[cfg(not(windows))]
        std::fs::rename(temp, path)?;
        Ok(())
    };
    save().map_err(|e| format!("Could not save hotkey settings: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validation_and_normalization() {
        assert_eq!(parse(" alt + CTRL + f12 ").unwrap().label(), "Ctrl+Alt+F12");
        for value in [
            "A",
            "Shift+A",
            "Ctrl+C",
            "Ctrl+Ctrl+A",
            "Alt+F4",
            "Win+L",
            "Ctrl+A+B",
            "Ctrl+F25",
            "Ctrl+",
            "Ctrl+Escape",
        ] {
            assert!(parse(value).is_err(), "{value}");
        }
        let keys = Hotkeys {
            copy: "ALT+CONTROL+SPACE".into(),
            ..Hotkeys::default()
        };
        assert!(keys.parsed().is_err());
    }
    #[test]
    fn persistence_replaces_previous_settings_and_reports_invalid_files() {
        let root = std::env::temp_dir().join(format!("clipforge-settings-{}", std::process::id()));
        let path = root.join("hotkeys.json");
        assert_eq!(load(&path).unwrap(), Hotkeys::default());
        save(&path, &Hotkeys::default()).unwrap();
        let keys = Hotkeys {
            menu: "ctrl+shift+f12".into(),
            ..Hotkeys::default()
        };
        save(&path, &keys).unwrap();
        assert_eq!(load(&path).unwrap(), keys.normalized().unwrap());
        std::fs::write(&path, b"broken").unwrap();
        assert!(load(&path).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
