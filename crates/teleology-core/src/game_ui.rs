//! Runtime game UI command buffer.
//!
//! Scripts push immediate-mode UI commands during their tick callbacks.
//! The host (editor/runtime) reads the buffer each frame, renders via egui,
//! then clears it. Interaction results (button clicks) are stored for scripts
//! to poll on the next frame.

use bevy_ecs::prelude::*;

/// One UI command from a script.
#[derive(Clone, Debug)]
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

/// Frame command buffer + interaction results from previous frame.
#[derive(Resource, Clone, Default, Debug)]
pub struct UiCommandBuffer {
    /// Commands pushed by scripts this frame.
    pub commands: Vec<UiCommand>,
    /// Button IDs clicked last frame (scripts poll this).
    pub clicked_buttons: Vec<u32>,
}

impl UiCommandBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a command into the buffer.
    pub fn push(&mut self, cmd: UiCommand) {
        self.commands.push(cmd);
    }

    /// Check if a button was clicked last frame.
    pub fn was_clicked(&self, id: u32) -> bool {
        self.clicked_buttons.contains(&id)
    }
}
