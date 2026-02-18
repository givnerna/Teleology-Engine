//! Runtime game UI command buffer and prefab system.
//!
//! Scripts push immediate-mode UI commands during their tick callbacks.
//! The host (editor/runtime) reads the buffer each frame, renders via egui,
//! then clears it. Interaction results (button clicks) are stored for scripts
//! to poll on the next frame.
//!
//! **Prefabs** let devs record a sequence of UI commands as a reusable template.
//! Text fields containing `{0}`, `{1}`, … are substituted at instantiation time.
//! Prefabs can be saved to / loaded from JSON files.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One UI command from a script.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum UiCommand {
    // --- Containers ---
    BeginWindow {
        title: String,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    EndWindow,
    BeginHorizontal,
    EndHorizontal,
    BeginVertical,
    EndVertical,

    // --- Widgets ---
    Label {
        text: String,
        font_size: f32,
    },
    Button {
        id: u32,
        text: String,
    },
    ProgressBar {
        fraction: f32,
        text: String,
        w: f32,
    },
    Image {
        path: String,
        w: f32,
        h: f32,
    },
    Separator,
    Spacing {
        amount: f32,
    },

    // --- Styling (applies to next widget) ---
    SetColor {
        r: u8,
        g: u8,
        b: u8,
        a: u8,
    },
    SetFontSize {
        size: f32,
    },
}

impl UiCommand {
    /// Substitute `{0}`, `{1}`, … placeholders in text fields.
    pub fn substitute(&self, params: &[&str]) -> UiCommand {
        fn sub(s: &str, params: &[&str]) -> String {
            let mut out = s.to_string();
            for (i, val) in params.iter().enumerate() {
                let placeholder = format!("{{{}}}", i);
                out = out.replace(&placeholder, val);
            }
            out
        }
        match self {
            UiCommand::BeginWindow { title, x, y, w, h } => UiCommand::BeginWindow {
                title: sub(title, params),
                x: *x, y: *y, w: *w, h: *h,
            },
            UiCommand::Label { text, font_size } => UiCommand::Label {
                text: sub(text, params),
                font_size: *font_size,
            },
            UiCommand::Button { id, text } => UiCommand::Button {
                id: *id,
                text: sub(text, params),
            },
            UiCommand::ProgressBar { fraction, text, w } => UiCommand::ProgressBar {
                fraction: *fraction,
                text: sub(text, params),
                w: *w,
            },
            UiCommand::Image { path, w, h } => UiCommand::Image {
                path: sub(path, params),
                w: *w, h: *h,
            },
            other => other.clone(),
        }
    }
}

/// A reusable UI layout template.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiPrefab {
    /// Human-readable name (also the lookup key).
    pub name: String,
    /// The recorded sequence of UI commands (may contain `{0}`, `{1}`, … placeholders).
    pub commands: Vec<UiCommand>,
}

impl UiPrefab {
    /// Instantiate this prefab: substitute placeholders and return expanded commands.
    pub fn instantiate(&self, params: &[&str]) -> Vec<UiCommand> {
        self.commands.iter().map(|cmd| cmd.substitute(params)).collect()
    }
}

/// Registry of named UI prefabs.
#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct UiPrefabRegistry {
    pub prefabs: HashMap<String, UiPrefab>,
}

impl UiPrefabRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Store a prefab. Overwrites any existing prefab with the same name.
    pub fn insert(&mut self, prefab: UiPrefab) {
        self.prefabs.insert(prefab.name.clone(), prefab);
    }

    /// Look up a prefab by name.
    pub fn get(&self, name: &str) -> Option<&UiPrefab> {
        self.prefabs.get(name)
    }

    /// Remove a prefab by name.
    pub fn remove(&mut self, name: &str) -> Option<UiPrefab> {
        self.prefabs.remove(name)
    }

    /// Sorted list of prefab names (for UI display).
    pub fn names_sorted(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.prefabs.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    /// Save all prefabs to a JSON file.
    pub fn save_to_file(&self, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load prefabs from a JSON file, replacing current contents.
    pub fn load_from_file(path: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        let json = std::fs::read_to_string(path)?;
        let registry: Self = serde_json::from_str(&json)?;
        Ok(registry)
    }

    /// Save a single prefab to a JSON file.
    pub fn save_prefab(&self, name: &str, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let prefab = self.get(name).ok_or("prefab not found")?;
        let json = serde_json::to_string_pretty(prefab)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load a single prefab from a JSON file and insert it.
    pub fn load_prefab(&mut self, path: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
        let json = std::fs::read_to_string(path)?;
        let prefab: UiPrefab = serde_json::from_str(&json)?;
        let name = prefab.name.clone();
        self.insert(prefab);
        Ok(name)
    }
}

/// Frame command buffer + interaction results from previous frame.
#[derive(Resource, Clone, Default, Debug)]
pub struct UiCommandBuffer {
    /// Commands pushed by scripts this frame.
    pub commands: Vec<UiCommand>,
    /// Button IDs clicked last frame (scripts poll this).
    pub clicked_buttons: Vec<u32>,
    /// When non-None, we're recording commands into a prefab instead of the main buffer.
    pub recording_prefab: Option<String>,
    /// Temporary storage for commands during prefab recording.
    pub recording_commands: Vec<UiCommand>,
}

impl UiCommandBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a command. If recording a prefab, it goes to the recording buffer instead.
    pub fn push(&mut self, cmd: UiCommand) {
        if self.recording_prefab.is_some() {
            self.recording_commands.push(cmd);
        } else {
            self.commands.push(cmd);
        }
    }

    /// Check if a button was clicked last frame.
    pub fn was_clicked(&self, id: u32) -> bool {
        self.clicked_buttons.contains(&id)
    }

    /// Start recording commands into a named prefab.
    pub fn begin_recording(&mut self, name: &str) {
        self.recording_prefab = Some(name.to_string());
        self.recording_commands.clear();
    }

    /// Finish recording and return the prefab. Returns None if not recording.
    pub fn end_recording(&mut self) -> Option<UiPrefab> {
        let name = self.recording_prefab.take()?;
        let commands = std::mem::take(&mut self.recording_commands);
        Some(UiPrefab { name, commands })
    }

    /// Check if currently recording a prefab.
    pub fn is_recording(&self) -> bool {
        self.recording_prefab.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefab_substitute_placeholders() {
        let prefab = UiPrefab {
            name: "resource_bar".into(),
            commands: vec![
                UiCommand::Label { text: "{0}: {1}".into(), font_size: 14.0 },
                UiCommand::ProgressBar { fraction: 0.0, text: "{0}".into(), w: 200.0 },
            ],
        };
        let expanded = prefab.instantiate(&["Gold", "1234"]);
        match &expanded[0] {
            UiCommand::Label { text, .. } => assert_eq!(text, "Gold: 1234"),
            other => panic!("expected Label, got {:?}", other),
        }
        match &expanded[1] {
            UiCommand::ProgressBar { text, .. } => assert_eq!(text, "Gold"),
            other => panic!("expected ProgressBar, got {:?}", other),
        }
    }

    #[test]
    fn prefab_registry_insert_get() {
        let mut reg = UiPrefabRegistry::new();
        reg.insert(UiPrefab {
            name: "hud".into(),
            commands: vec![UiCommand::Separator],
        });
        assert!(reg.get("hud").is_some());
        assert!(reg.get("missing").is_none());
        assert_eq!(reg.names_sorted(), vec!["hud"]);
    }

    #[test]
    fn command_buffer_recording() {
        let mut buf = UiCommandBuffer::new();
        buf.push(UiCommand::Separator); // goes to main buffer
        assert_eq!(buf.commands.len(), 1);

        buf.begin_recording("my_prefab");
        buf.push(UiCommand::Label { text: "hello".into(), font_size: 14.0 });
        buf.push(UiCommand::Button { id: 1, text: "ok".into() });
        assert_eq!(buf.commands.len(), 1); // main buffer unchanged
        assert_eq!(buf.recording_commands.len(), 2);

        let prefab = buf.end_recording().unwrap();
        assert_eq!(prefab.name, "my_prefab");
        assert_eq!(prefab.commands.len(), 2);
        assert!(!buf.is_recording());
    }

    #[test]
    fn prefab_json_roundtrip() {
        let prefab = UiPrefab {
            name: "test".into(),
            commands: vec![
                UiCommand::BeginWindow { title: "{0}".into(), x: 10.0, y: 20.0, w: 300.0, h: 200.0 },
                UiCommand::Label { text: "Hello {1}".into(), font_size: 16.0 },
                UiCommand::EndWindow,
            ],
        };
        let json = serde_json::to_string(&prefab).unwrap();
        let back: UiPrefab = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "test");
        assert_eq!(back.commands.len(), 3);
    }
}
