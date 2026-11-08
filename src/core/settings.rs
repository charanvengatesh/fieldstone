//! `settings.ron` next to the executable: key bindings, default zoom, volume. Written with
//! defaults on first run so players can see what to edit.

use std::fs;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub const SETTINGS_PATH: &str = "settings.ron";

#[derive(Resource, Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub zoom: f32,
    /// 0.0 .. 1.0
    pub volume: f32,
    pub keys: BindingNames,
}

/// Key names as written in the file; parsed into [`Keys`] at startup.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BindingNames {
    pub up: Vec<String>,
    pub down: Vec<String>,
    pub left: Vec<String>,
    pub right: Vec<String>,
    pub sprint: Vec<String>,
    pub use_item: Vec<String>,
    pub inventory: Vec<String>,
    pub craft: Vec<String>,
    pub save: Vec<String>,
    pub debug: Vec<String>,
    pub pause: Vec<String>,
    pub zoom_in: Vec<String>,
    pub zoom_out: Vec<String>,
    pub hotbar: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect();
        Self {
            zoom: 0.6,
            volume: 0.8,
            keys: BindingNames {
                up: v(&["W", "Up"]),
                down: v(&["S", "Down"]),
                left: v(&["A", "Left"]),
                right: v(&["D", "Right"]),
                sprint: v(&["ShiftLeft", "ShiftRight"]),
                use_item: v(&["F"]),
                inventory: v(&["E"]),
                craft: v(&["C"]),
                save: v(&["F5"]),
                debug: v(&["F3"]),
                pause: v(&["Escape"]),
                zoom_in: v(&["Equal"]),
                zoom_out: v(&["Minus"]),
                hotbar: v(&["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"]),
            },
        }
    }
}

/// Parsed bindings. Each action may have several keys.
#[derive(Resource, Clone, Debug)]
pub struct Keys {
    pub up: Vec<KeyCode>,
    pub down: Vec<KeyCode>,
    pub left: Vec<KeyCode>,
    pub right: Vec<KeyCode>,
    pub sprint: Vec<KeyCode>,
    pub use_item: Vec<KeyCode>,
    pub inventory: Vec<KeyCode>,
    pub craft: Vec<KeyCode>,
    pub save: Vec<KeyCode>,
    pub debug: Vec<KeyCode>,
    pub pause: Vec<KeyCode>,
    pub zoom_in: Vec<KeyCode>,
    pub zoom_out: Vec<KeyCode>,
    pub hotbar: Vec<KeyCode>,
}

impl Keys {
    pub fn from_names(n: &BindingNames) -> Self {
        let p = |xs: &[String]| xs.iter().filter_map(|s| parse_key(s)).collect::<Vec<_>>();
        Self {
            up: p(&n.up),
            down: p(&n.down),
            left: p(&n.left),
            right: p(&n.right),
            sprint: p(&n.sprint),
            use_item: p(&n.use_item),
            inventory: p(&n.inventory),
            craft: p(&n.craft),
            save: p(&n.save),
            debug: p(&n.debug),
            pause: p(&n.pause),
            zoom_in: p(&n.zoom_in),
            zoom_out: p(&n.zoom_out),
            hotbar: p(&n.hotbar),
        }
    }
    pub fn pressed(&self, input: &ButtonInput<KeyCode>, keys: &[KeyCode]) -> bool {
        keys.iter().any(|k| input.pressed(*k))
    }
    pub fn just(&self, input: &ButtonInput<KeyCode>, keys: &[KeyCode]) -> bool {
        keys.iter().any(|k| input.just_pressed(*k))
    }
}

/// Accepts single letters, digits, and the common named keys. Unknown names are ignored with
/// a warning so a typo never locks the player out.
pub fn parse_key(name: &str) -> Option<KeyCode> {
    use KeyCode::*;
    let k = match name {
        "A" => KeyA,
        "B" => KeyB,
        "C" => KeyC,
        "D" => KeyD,
        "E" => KeyE,
        "F" => KeyF,
        "G" => KeyG,
        "H" => KeyH,
        "I" => KeyI,
        "J" => KeyJ,
        "K" => KeyK,
        "L" => KeyL,
        "M" => KeyM,
        "N" => KeyN,
        "O" => KeyO,
        "P" => KeyP,
        "Q" => KeyQ,
        "R" => KeyR,
        "S" => KeyS,
        "T" => KeyT,
        "U" => KeyU,
        "V" => KeyV,
        "W" => KeyW,
        "X" => KeyX,
        "Y" => KeyY,
        "Z" => KeyZ,
        "0" => Digit0,
        "1" => Digit1,
        "2" => Digit2,
        "3" => Digit3,
        "4" => Digit4,
        "5" => Digit5,
        "6" => Digit6,
        "7" => Digit7,
        "8" => Digit8,
        "9" => Digit9,
        "Up" => ArrowUp,
        "Down" => ArrowDown,
        "Left" => ArrowLeft,
        "Right" => ArrowRight,
        "Space" => Space,
        "Tab" => Tab,
        "Escape" | "Esc" => Escape,
        "Enter" => Enter,
        "ShiftLeft" | "Shift" => ShiftLeft,
        "ShiftRight" => ShiftRight,
        "ControlLeft" | "Ctrl" => ControlLeft,
        "ControlRight" => ControlRight,
        "AltLeft" | "Alt" => AltLeft,
        "AltRight" => AltRight,
        "Minus" | "-" => Minus,
        "Equal" | "=" => Equal,
        "Backquote" | "`" => Backquote,
        "BracketLeft" | "[" => BracketLeft,
        "BracketRight" | "]" => BracketRight,
        "F1" => F1,
        "F2" => F2,
        "F3" => F3,
        "F4" => F4,
        "F5" => F5,
        "F6" => F6,
        "F7" => F7,
        "F8" => F8,
        "F9" => F9,
        "F10" => F10,
        "F11" => F11,
        "F12" => F12,
        other => {
            warn!("settings: unknown key name '{other}'");
            return None;
        }
    };
    Some(k)
}

pub fn load() -> Settings {
    match fs::read_to_string(SETTINGS_PATH) {
        Ok(text) => match ron::from_str::<Settings>(&text) {
            Ok(s) => s,
            Err(e) => {
                error!("{SETTINGS_PATH}: {e}; using defaults");
                Settings::default()
            }
        },
        Err(_) => {
            let s = Settings::default();
            save(&s);
            s
        }
    }
}

pub fn save(settings: &Settings) {
    match ron::ser::to_string_pretty(settings, ron::ser::PrettyConfig::default()) {
        Ok(text) => {
            if let Err(e) = fs::write(SETTINGS_PATH, text) {
                error!("could not write {SETTINGS_PATH}: {e}");
            }
        }
        Err(e) => error!("settings: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bindings_parse() {
        let k = Keys::from_names(&Settings::default().keys);
        assert_eq!(k.up, vec![KeyCode::KeyW, KeyCode::ArrowUp]);
        assert_eq!(k.hotbar.len(), 10);
        assert_eq!(parse_key("nope"), None);
    }
}
