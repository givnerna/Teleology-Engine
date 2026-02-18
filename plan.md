# Runtime Game UI Toolkit — Implementation Plan

## Problem

Game devs using the Teleology Engine C++ scripting API have **zero tools for building in-game UI**. The only UI scripts can produce is the fixed-format pop-up event system (title + body + choice buttons). There's no way to:

- Show a HUD (gold, date, nation name, military strength)
- Create menus (main menu, pause, settings, diplomacy)
- Display tooltips, resource bars, stat panels
- Put arbitrary text or images on screen
- Build any custom game interface

## Architecture

**Immediate-mode command buffer**: Scripts call `teleology_ui_*` functions during their tick callbacks. These push commands into a `UiCommandBuffer` resource. After the tick, the editor/runtime reads and renders them via egui. Buffer is cleared each frame.

This matches egui's immediate-mode philosophy — scripts "declare" their UI each frame, just like egui code does.

```
Script (C++)                Engine (Rust)              Renderer (egui)
─────────────              ──────────────             ────────────────
on_daily_tick() {          UiCommandBuffer {          for cmd in buffer {
  teleology_ui_window()      commands: Vec<UiCmd>       match cmd {
  teleology_ui_label()     }                            BeginWindow => Window::new()
  teleology_ui_button()                                 Label => ui.label()
  teleology_ui_end()                                    Button => ui.button()
}                                                     }
```

---

## 1. `UiCommandBuffer` Resource (teleology-core)

**File:** `crates/teleology-core/src/game_ui.rs` (new)

```rust
/// One UI command from a script.
#[derive(Clone)]
pub enum UiCommand {
    // --- Containers ---
    BeginWindow { title: String, x: f32, y: f32, w: f32, h: f32 },
    EndWindow,
    BeginHorizontal,
    EndHorizontal,
    BeginVertical,
    EndVertical,

    // --- Widgets ---
    Label { text: String, font_size: f32 },
    Button { id: u32, text: String },
    ProgressBar { fraction: f32, text: String, w: f32 },
    Image { path: String, w: f32, h: f32 },
    Separator,
    Spacing { amount: f32 },

    // --- Styling (applies to next widget) ---
    SetColor { r: u8, g: u8, b: u8, a: u8 },
    SetFontSize { size: f32 },
}

/// Frame command buffer + interaction results from previous frame.
#[derive(Resource, Clone, Default)]
pub struct UiCommandBuffer {
    pub commands: Vec<UiCommand>,
    /// Button IDs clicked last frame (scripts poll this).
    pub clicked_buttons: Vec<u32>,
}
```

The `clicked_buttons` field is key: when the renderer processes a `Button` command and egui reports it was clicked, the button's `id` is added to `clicked_buttons`. Next frame, scripts poll to check if their button was clicked.

---

## 2. C API Functions (teleology-runtime + teleology-script-api)

New `#[no_mangle] extern "C"` functions in `teleology-runtime/src/lib.rs`:

| C Function | Purpose |
|---|---|
| `teleology_ui_begin_window(engine, title, x, y, w, h)` | Open a positioned window |
| `teleology_ui_end_window(engine)` | Close window |
| `teleology_ui_begin_horizontal(engine)` | Start horizontal layout |
| `teleology_ui_end_horizontal(engine)` | End horizontal layout |
| `teleology_ui_begin_vertical(engine)` | Start vertical layout |
| `teleology_ui_end_vertical(engine)` | End vertical layout |
| `teleology_ui_label(engine, text)` | Text label |
| `teleology_ui_label_sized(engine, text, font_size)` | Text label with custom size |
| `teleology_ui_button(engine, id, text)` | Clickable button |
| `teleology_ui_button_was_clicked(engine, id) -> bool` | Poll if button was clicked last frame |
| `teleology_ui_progress_bar(engine, fraction, text, width)` | Progress/resource bar |
| `teleology_ui_image(engine, path, w, h)` | Display an image |
| `teleology_ui_separator(engine)` | Horizontal separator line |
| `teleology_ui_spacing(engine, amount)` | Vertical spacing |
| `teleology_ui_set_color(engine, r, g, b, a)` | Set color for next widget |
| `teleology_ui_set_font_size(engine, size)` | Set font size for next widget |

---

## 3. Renderer Integration (teleology-editor)

In `EditorApp::update()`, after the tick and after rendering the map, read `UiCommandBuffer` and render via egui:

```rust
fn render_game_ui(&self, ctx: &egui::Context) {
    let world = self.engine.world();
    let Some(buffer) = world.get_resource::<UiCommandBuffer>() else { return };
    let mut clicked = Vec::new();
    // Walk commands, maintaining a stack of open containers
    // BeginWindow -> egui::Window::new().fixed_pos().fixed_size().show()
    // Label -> ui.label()
    // Button { id, text } -> if ui.button(text).clicked() { clicked.push(id) }
    // etc.
    // After rendering, write clicked_buttons back
    drop(buffer);
    if let Some(mut buf) = self.engine.world_mut().get_resource_mut::<UiCommandBuffer>() {
        buf.clicked_buttons = clicked;
        buf.commands.clear();
    }
}
```

---

## 4. C++ Header Update

Add all new functions to `cpp/include/teleology.h`:

```c
/* --- Game UI (immediate-mode; call during tick; rendered by engine each frame) --- */
void teleology_ui_begin_window(TeleologyEngine* engine, const char* title, float x, float y, float w, float h);
void teleology_ui_end_window(TeleologyEngine* engine);
void teleology_ui_begin_horizontal(TeleologyEngine* engine);
void teleology_ui_end_horizontal(TeleologyEngine* engine);
void teleology_ui_begin_vertical(TeleologyEngine* engine);
void teleology_ui_end_vertical(TeleologyEngine* engine);
void teleology_ui_label(TeleologyEngine* engine, const char* text);
void teleology_ui_label_sized(TeleologyEngine* engine, const char* text, float font_size);
void teleology_ui_button(TeleologyEngine* engine, uint32_t id, const char* text);
uint8_t teleology_ui_button_was_clicked(TeleologyEngine* engine, uint32_t id);
void teleology_ui_progress_bar(TeleologyEngine* engine, float fraction, const char* text, float width);
void teleology_ui_image(TeleologyEngine* engine, const char* path, float w, float h);
void teleology_ui_separator(TeleologyEngine* engine);
void teleology_ui_spacing(TeleologyEngine* engine, float amount);
void teleology_ui_set_color(TeleologyEngine* engine, uint8_t r, uint8_t g, uint8_t b, uint8_t a);
void teleology_ui_set_font_size(TeleologyEngine* engine, float size);
```

---

## 5. What a Game Dev's Code Looks Like

```cpp
void on_daily_tick(TeleologyEngine* engine) {
    CGameDate date = teleology_get_date(engine);

    // Top HUD bar
    teleology_ui_begin_window(engine, "HUD", 0, 0, 800, 40);
      teleology_ui_begin_horizontal(engine);
        teleology_ui_set_font_size(engine, 18.0f);
        teleology_ui_label(engine, "Gold: 1,234");
        teleology_ui_spacing(engine, 20);
        teleology_ui_label(engine, "Army: 15,000");
        teleology_ui_spacing(engine, 20);
        char datebuf[32];
        snprintf(datebuf, sizeof(datebuf), "%d-%02d-%02d", date.year, date.month, date.day);
        teleology_ui_label(engine, datebuf);
      teleology_ui_end_horizontal(engine);
    teleology_ui_end_window(engine);

    // Diplomacy button
    teleology_ui_begin_window(engine, "Menu", 700, 50, 100, 40);
      teleology_ui_button(engine, 1, "Diplomacy");
    teleology_ui_end_window(engine);

    if (teleology_ui_button_was_clicked(engine, 1)) {
        show_diplomacy_panel = true;
    }

    if (show_diplomacy_panel) {
        teleology_ui_begin_window(engine, "Diplomacy", 200, 100, 400, 300);
          teleology_ui_label_sized(engine, "Relations", 20.0f);
          teleology_ui_separator(engine);
          teleology_ui_label(engine, "France: +50");
          teleology_ui_label(engine, "England: -30");
          teleology_ui_progress_bar(engine, 0.75f, "Trust: 75%", 300.0f);
          teleology_ui_spacing(engine, 10);
          teleology_ui_button(engine, 2, "Close");
        teleology_ui_end_window(engine);

        if (teleology_ui_button_was_clicked(engine, 2)) {
            show_diplomacy_panel = false;
        }
    }
}
```

---

## 6. Implementation Order

| Step | What | Files |
|------|------|-------|
| 1 | Create `game_ui.rs` with `UiCommand` enum + `UiCommandBuffer` resource | `crates/teleology-core/src/game_ui.rs` (new), edit `lib.rs` |
| 2 | Add `UiCommandBuffer` initialization in runtime `EngineContext::new()` | Edit `crates/teleology-runtime/src/lib.rs` |
| 3 | Implement all `teleology_ui_*` C API functions in runtime | Edit `crates/teleology-runtime/src/lib.rs` |
| 4 | Add `EngineApi` trait methods for UI (with default no-ops) | Edit `crates/teleology-script-api/src/lib.rs` |
| 5 | Add renderer in editor that reads `UiCommandBuffer` and renders via egui | Edit `crates/teleology-editor/src/lib.rs` |
| 6 | Update C header with all new function declarations | Edit `cpp/include/teleology.h` |
| 7 | Add tests for command buffer + round-trip | Edit `crates/teleology-runtime/src/lib.rs` (tests) |

---

## Files Changed/Created

| File | Action |
|------|--------|
| `crates/teleology-core/src/game_ui.rs` | **Create** — UiCommand, UiCommandBuffer |
| `crates/teleology-core/src/lib.rs` | **Edit** — add `pub mod game_ui` + re-exports |
| `crates/teleology-runtime/src/lib.rs` | **Edit** — init buffer, implement C API functions |
| `crates/teleology-script-api/src/lib.rs` | **Edit** — add UI trait methods (optional) |
| `crates/teleology-editor/src/lib.rs` | **Edit** — add `render_game_ui()` method |
| `cpp/include/teleology.h` | **Edit** — declare new C functions |
