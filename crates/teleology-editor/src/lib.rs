//! Visual editor for the Teleology engine. Modes: Map Editor, World view, Settings.
//! Runs on Windows, Mac, Linux (native) and WebGL (browser).

use eframe::egui;
use std::collections::HashSet;
use std::num::NonZeroU32;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use teleology_core::{
    add_province_to_world, compute_adjacency, pull_next_event, queue_event, ArmyComposition,
    ArmyRegistry, CharacterGenConfig, EventBus, EventQueue, EventRegistry, GameDate, MapFile,
    MapKind, NationId, NationModifiers, NationStore, NationTags, ProgressState, ProgressTrees,
    ProvinceId, ProvinceModifiers, ProvinceStore, ProvinceTags, TagId, TagRegistry, TagTypeId,
    Army, spawn_army, ActiveEvent, WorldBounds,
};
use teleology_runtime::EngineContext;

/// Editor mode / feature.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorMode {
    /// Paint provinces, assign owners, load/save maps.
    #[default]
    MapEditor,
    /// Overview: date, province/nation counts, simulation controls.
    World,
    /// Editor settings (placeholder).
    Settings,
    /// Visual editor for pop-up events (connect choices to next events).
    Events,
    /// Visual editor for progress trees (connect prerequisite edges).
    ProgressTrees,
    /// Native audio/video test tools.
    Media,
}

/// Deferred context-menu action (run after panel so we don't hold world while ensuring).
#[derive(Clone, Copy)]
enum PendingContextAction {
    SetTagProvince(u32),
    SetTagNation(u32),
    AddModifierProvince(u32),
    AddModifierNation(u32),
    FireEventProvince(u32),
    SpawnArmyProvince(u32),
}

/// Map editor paint mode: paint by nation (ownership) or edit province layout.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum MapEditorPaintMode {
    /// Select a nation and paint on the map to set ownership. One step, intuitive.
    #[default]
    PaintOwnership,
    /// Edit which province id is in each tile; then assign province → nation.
    EditProvinces,
}

/// Non-province tile (empty / water / impassable). Dark blue so clearly distinct from land.
const TILE_EMPTY_COLOR: egui::Color32 = egui::Color32::from_rgb(0x0f, 0x1f, 0x2f);
/// Province tile with no nation owner yet. Light warm tan so clearly distinct from empty (blue) and owned (nation colors).
const TILE_UNOWNED_PROVINCE_COLOR: egui::Color32 = egui::Color32::from_rgb(0x9a, 0x85, 0x6b);
/// Stroke between provinces (same or different nation) so you can tell provinces apart.
fn province_border_stroke() -> egui::Stroke {
    egui::Stroke::new(1.0, egui::Color32::from_rgb(0x22, 0x22, 0x22))
}

const NATION_COLORS: [egui::Color32; 16] = [
    egui::Color32::from_rgb(0x8B, 0x45, 0x13),
    egui::Color32::from_rgb(0x41, 0x69, 0xE1),
    egui::Color32::from_rgb(0x22, 0x8B, 0x22),
    egui::Color32::from_rgb(0xDC, 0x14, 0x3C),
    egui::Color32::from_rgb(0xFF, 0xD7, 0x00),
    egui::Color32::from_rgb(0x99, 0x32, 0xCC),
    egui::Color32::from_rgb(0x00, 0xBF, 0xFF),
    egui::Color32::from_rgb(0xFF, 0x63, 0x47),
    egui::Color32::from_rgb(0x2E, 0x8B, 0x57),
    egui::Color32::from_rgb(0x93, 0x70, 0xDB),
    egui::Color32::from_rgb(0x87, 0xCE, 0xEB),
    egui::Color32::from_rgb(0xB8, 0x86, 0x0B),
    egui::Color32::from_rgb(0xCD, 0x5C, 0x5C),
    egui::Color32::from_rgb(0x20, 0xB2, 0xAA),
    egui::Color32::from_rgb(0x69, 0x69, 0x69),
    egui::Color32::from_rgb(0xFF, 0x69, 0xB4),
];

/// Map egui Key to script key code. Letters/digits/special keys use stable constants (ASCII / 256+).
/// Any other key (F1–F35, PageUp, punctuation, etc.) gets a unique code in 0x8000+ so OnKeyDown/OnKeyUp receive every key.
fn egui_key_to_code(key: egui::Key) -> u32 {
    use egui::Key;
    match key {
        Key::A => 65,
        Key::B => 66,
        Key::C => 67,
        Key::D => 68,
        Key::E => 69,
        Key::F => 70,
        Key::G => 71,
        Key::H => 72,
        Key::I => 73,
        Key::J => 74,
        Key::K => 75,
        Key::L => 76,
        Key::M => 77,
        Key::N => 78,
        Key::O => 79,
        Key::P => 80,
        Key::Q => 81,
        Key::R => 82,
        Key::S => 83,
        Key::T => 84,
        Key::U => 85,
        Key::V => 86,
        Key::W => 87,
        Key::X => 88,
        Key::Y => 89,
        Key::Z => 90,
        Key::Num0 => 48,
        Key::Num1 => 49,
        Key::Num2 => 50,
        Key::Num3 => 51,
        Key::Num4 => 52,
        Key::Num5 => 53,
        Key::Num6 => 54,
        Key::Num7 => 55,
        Key::Num8 => 56,
        Key::Num9 => 57,
        Key::Space => 32,
        Key::Escape => 256,
        Key::Enter => 257,
        Key::Tab => 258,
        Key::Backspace => 259,
        Key::Insert => 260,
        Key::Delete => 261,
        Key::ArrowRight => 262,
        Key::ArrowLeft => 263,
        Key::ArrowDown => 264,
        Key::ArrowUp => 265,
        Key::Home => 266,
        Key::End => 267,
        Key::PageUp => 268,
        Key::PageDown => 269,
        Key::F1 => 270,
        Key::F2 => 271,
        Key::F3 => 272,
        Key::F4 => 273,
        Key::F5 => 274,
        Key::F6 => 275,
        Key::F7 => 276,
        Key::F8 => 277,
        Key::F9 => 278,
        Key::F10 => 279,
        Key::F11 => 280,
        Key::F12 => 281,
        // Any other key (F13–F35, punctuation, etc.): unique code so OnKey(*) receives every key
        other => 0x8000_u32.wrapping_add((other as u8) as u32),
    }
}

fn nation_color(nation_raw: u32) -> egui::Color32 {
    if nation_raw == 0 {
        return egui::Color32::from_gray(80);
    }
    let i = ((nation_raw - 1) as usize) % NATION_COLORS.len();
    NATION_COLORS[i]
}

/// Unity/Unreal-style darker panel background.
fn panel_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(0x2d, 0x2d, 0x30))
        .inner_margin(egui::Margin::same(6.0))
}

/// Section header bar for Hierarchy / Scene / Inspector (Unity-style).
/// Uses Sense::focusable_noninteractive() so it never steals clicks from the map or other content below.
fn panel_header(ui: &mut egui::Ui, title: &str) {
    let bar_height = 22.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::Vec2::new(ui.available_rect_before_wrap().width(), bar_height),
        egui::Sense::focusable_noninteractive(),
    );
    ui.painter().rect_filled(
        rect,
        0.0,
        egui::Color32::from_rgb(0x3e, 0x3e, 0x42),
    );
    let text_pos = egui::Pos2::new(rect.min.x + 8.0, rect.center().y);
    ui.painter().text(
        text_pos,
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::default(),
        egui::Color32::from_gray(220),
    );
    ui.add_space(4.0);
}

pub struct EditorApp {
    pub engine: EngineContext,
    pub mode: EditorMode,
    /// Map editor: paint by nation (default) or edit province layout.
    pub map_paint_mode: MapEditorPaintMode,
    script_path_input: String,
    hot_reload: bool,
    selected_province: Option<u32>,
    selected_nation: Option<u32>,
    tick_accumulator: f32,
    running: bool,

    // --- Optional system UI state ---
    tag_type_name_input: String,
    tag_name_input: String,
    tag_type_raw_input: String,
    tag_raw_input: String,
    event_topic_input: String,
    event_payload_input: String,
    audio_path_input: String,
    video_path_input: String,

    // --- Graph editor state (events / progress trees) ---
    event_graph_pan: egui::Vec2,
    event_graph_pos: std::collections::HashMap<u32, egui::Pos2>, // event_raw -> pos in canvas space
    event_selected_raw: Option<u32>,
    event_link_from_choice: Option<(u32, usize)>, // (source_event_raw, choice_idx)

    progress_graph_pan: egui::Vec2,
    progress_graph_pos: std::collections::HashMap<(u32, u32), egui::Pos2>, // (tree_raw, node_raw) -> pos
    progress_selected_tree_raw: Option<u32>,
    progress_selected_node_raw: Option<u32>,
    progress_link_from_node: Option<u32>, // prerequisite from node_raw

    /// Lazy-init: create event definition on next frame after ensure (avoids holding world while ensuring).
    pending_create_event: bool,
    pending_create_tree: bool,

    /// Previous frame keys down (for script input delta).
    script_prev_keys: HashSet<u32>,

    /// Deferred from right-click context menu (avoids borrowing world in closure).
    pending_context_action: Option<PendingContextAction>,

    /// Undo/redo for map editor (map + province store + bounds).
    undo_history: Vec<(MapKind, ProvinceStore, WorldBounds)>,
    redo_history: Vec<(MapKind, ProvinceStore, WorldBounds)>,
    max_undo: usize,
    /// True while pointer is down so we push undo only once per paint stroke.
    stroke_undo_pushed: bool,
}

impl EditorApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let mut engine = EngineContext::new();
        engine.set_hot_reload(true);
        Self {
            engine,
            mode: EditorMode::default(),
            map_paint_mode: MapEditorPaintMode::default(),
            script_path_input: String::new(),
            hot_reload: true,
            selected_province: None,
            selected_nation: None,
            tick_accumulator: 0.0,
            running: false,

            tag_type_name_input: "religion".to_string(),
            tag_name_input: "catholic".to_string(),
            tag_type_raw_input: "1".to_string(),
            tag_raw_input: "1".to_string(),
            event_topic_input: "demo_event".to_string(),
            event_payload_input: "hello".to_string(),
            audio_path_input: String::new(),
            video_path_input: String::new(),

            event_graph_pan: egui::Vec2::new(20.0, 20.0),
            event_graph_pos: std::collections::HashMap::new(),
            event_selected_raw: None,
            event_link_from_choice: None,

            progress_graph_pan: egui::Vec2::new(20.0, 20.0),
            progress_graph_pos: std::collections::HashMap::new(),
            progress_selected_tree_raw: None,
            progress_selected_node_raw: None,
            progress_link_from_node: None,
            pending_create_event: false,
            pending_create_tree: false,
            script_prev_keys: HashSet::new(),
            pending_context_action: None,
            undo_history: Vec::new(),
            redo_history: Vec::new(),
            max_undo: 50,
            stroke_undo_pushed: false,
        }
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint();

        // Unity/Unreal-style dark theme
        ctx.set_visuals(egui::Visuals::dark());

        if !ctx.input(|i| i.pointer.primary_down()) {
            self.stroke_undo_pushed = false;
        }
        let undo_redo = ctx.input(|i| {
            if i.key_pressed(egui::Key::Z) && i.modifiers.command {
                if i.modifiers.shift {
                    Some(true) // redo
                } else {
                    Some(false) // undo
                }
            } else {
                None
            }
        });
        if let Some(do_redo) = undo_redo {
            if do_redo {
                self.redo();
            } else {
                self.undo();
            }
        }

        if self.running {
            self.tick_accumulator += ctx.input(|i| i.stable_dt);
            while self.tick_accumulator >= 1.0 / 60.0 {
                self.tick_accumulator -= 1.0 / 60.0;
                self.engine.tick();
            }
        }

        let _ = self.engine.try_reload_script();

        // --- Feed input to engine for script OnClick / OnKeyDown / OnKeyUp ---
        {
            let clicked = ctx.input(|i| i.pointer.primary_clicked());
            let pos = ctx.input(|i| i.pointer.interact_pos());
            if clicked && pos.is_some() {
                let p = pos.unwrap();
                self.engine.feed_click(p.x, p.y);
            }
            let keys_now: HashSet<u32> = ctx
                .input(|i| i.keys_down.iter().copied().map(egui_key_to_code).collect());
            for &code in keys_now.difference(&self.script_prev_keys) {
                self.engine.feed_key_down(code);
            }
            for &code in self.script_prev_keys.difference(&keys_now) {
                self.engine.feed_key_up(code);
            }
            self.script_prev_keys = keys_now;
            self.engine.deliver_input_events();
        }

        // --- Menu bar (Unity/Unreal style) ---
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        if ui.button("Open Map…").clicked() {
                            ui.close_menu();
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("Teleology map", &["tmap"])
                                .pick_file()
                            {
                                if let Ok(mut f) = std::fs::File::open(&path) {
                                    match MapFile::read(&mut f) {
                                        Ok(map_file) => map_file.apply_to_world(self.engine.world_mut()),
                                        Err(e) => eprintln!("Load map failed: {}", e),
                                    }
                                }
                            }
                        }
                        if ui.button("Save Map…").clicked() {
                            ui.close_menu();
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("Teleology map", &["tmap"])
                                .set_file_name("map.tmap")
                                .save_file()
                            {
                                let world = self.engine.world_mut();
                                if let Some(map_file) = MapFile::from_world(world) {
                                    if let Ok(mut f) = std::fs::File::create(&path) {
                                        let _ = map_file.write(&mut f);
                                    }
                                }
                            }
                        }
                        ui.separator();
                    }
                    if ui.button("Exit").clicked() {
                        ui.close_menu();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Map", |ui| {
                    #[cfg(not(target_arch = "wasm32"))]
                    if ui.button("Recompute Adjacency").clicked() {
                        ui.close_menu();
                        let world = self.engine.world_mut();
                        if let (Some(map_kind), Some(bounds)) = (
                            world.get_resource::<MapKind>(),
                            world.get_resource::<WorldBounds>(),
                        ) {
                            let adj = compute_adjacency(map_kind, bounds.province_count);
                            world.insert_resource(adj);
                        }
                    }
                });
                ui.menu_button("Window", |ui| {
                    if ui.button("Map Editor").clicked() { self.mode = EditorMode::MapEditor; ui.close_menu(); }
                    if ui.button("Events").clicked() { self.mode = EditorMode::Events; ui.close_menu(); }
                    if ui.button("Progress Trees").clicked() { self.mode = EditorMode::ProgressTrees; ui.close_menu(); }
                    if ui.button("World").clicked() { self.mode = EditorMode::World; ui.close_menu(); }
                    if ui.button("Settings").clicked() { self.mode = EditorMode::Settings; ui.close_menu(); }
                    if ui.button("Media").clicked() { self.mode = EditorMode::Media; ui.close_menu(); }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("About Teleology").clicked() { ui.close_menu(); }
                });
            });
        });

        // --- Toolbar (Play / Pause / Tick + Undo / Redo + window tabs) ---
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.style_mut().spacing.item_spacing.x = 8.0;
                if ui.button(if self.running { "⏸ Pause" } else { "▶ Play" }).clicked() { self.running = !self.running; }
                if ui.button("⏭ Tick").clicked() { self.engine.tick(); }
                ui.separator();
                let can_undo = !self.undo_history.is_empty();
                let can_redo = !self.redo_history.is_empty();
                let undo_btn = ui.add_enabled(can_undo, egui::Button::new("↶ Undo")).on_hover_text("Undo last map edit (⌘Z)");
                if undo_btn.clicked() { self.undo(); }
                let redo_btn = ui.add_enabled(can_redo, egui::Button::new("↷ Redo")).on_hover_text("Redo (⌘⇧Z)");
                if redo_btn.clicked() { self.redo(); }
                ui.separator();
                ui.add_space(4.0);
                ui.label(egui::RichText::new("Window:").small().color(ui.visuals().weak_text_color()));
                ui.selectable_value(&mut self.mode, EditorMode::MapEditor, "Map");
                ui.selectable_value(&mut self.mode, EditorMode::Events, "Events");
                ui.selectable_value(&mut self.mode, EditorMode::ProgressTrees, "Trees");
                ui.selectable_value(&mut self.mode, EditorMode::World, "World");
                ui.selectable_value(&mut self.mode, EditorMode::Settings, "Settings");
                ui.selectable_value(&mut self.mode, EditorMode::Media, "Media");
            });
        });

        // --- Body: mode-specific content (Hierarchy | Scene | Inspector) ---
        match self.mode {
            EditorMode::MapEditor => self.ui_map_editor(ctx),
            EditorMode::Events => self.ui_events_editor(ctx),
            EditorMode::ProgressTrees => self.ui_progress_trees_editor(ctx),
            EditorMode::Media => self.ui_media(ctx),
            EditorMode::World => self.ui_world(ctx),
            EditorMode::Settings => self.ui_settings(ctx),
        }

        // --- Bottom panel: Project / Console (Unity-style) ---
        egui::TopBottomPanel::bottom("project_console")
            .frame(panel_frame())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.style_mut().spacing.item_spacing.x = 8.0;
                    ui.strong("Project");
                ui.separator();
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.label("Script:");
                    ui.add(egui::TextEdit::singleline(&mut self.script_path_input).desired_width(280.0));
                    if ui.button("Load").clicked() {
                        let path = PathBuf::from(self.script_path_input.trim());
                        if path.exists() { let _ = self.engine.load_script(&path); }
                    }
                    ui.checkbox(&mut self.hot_reload, "Hot reload");
                    self.engine.set_hot_reload(self.hot_reload);
                }
                #[cfg(target_arch = "wasm32")]
                ui.label("WebGL │ script on desktop");
            });
        });
    }
}

impl EditorApp {
    /// Lazy-init: systems are disabled by default; these run on first use (right-click or API).
    fn ensure_tags(&mut self) {
        let world = self.engine.world_mut();
        if world.get_resource::<TagRegistry>().is_none() {
            world.insert_resource(TagRegistry::new());
            world.insert_resource(ProvinceTags::default());
            world.insert_resource(NationTags::default());
        }
    }
    fn ensure_modifiers(&mut self) {
        let world = self.engine.world_mut();
        if world.get_resource::<ProvinceModifiers>().is_none() {
            if let Some(b) = world.get_resource::<WorldBounds>().cloned() {
                world.insert_resource(ProvinceModifiers::new(b.province_count as usize));
                world.insert_resource(NationModifiers::new(b.nation_count as usize));
            }
        }
    }
    fn ensure_events(&mut self) {
        let world = self.engine.world_mut();
        if world.get_resource::<EventRegistry>().is_none() {
            world.insert_resource(EventRegistry::new());
            world.insert_resource(EventQueue::default());
            world.insert_resource(ActiveEvent::default());
        }
    }
    fn ensure_event_bus(&mut self) {
        let world = self.engine.world_mut();
        if world.get_resource::<EventBus>().is_none() {
            world.insert_resource(EventBus::new());
        }
    }
    fn ensure_progress_trees(&mut self) {
        let world = self.engine.world_mut();
        if world.get_resource::<ProgressTrees>().is_none() {
            if let Some(b) = world.get_resource::<WorldBounds>().cloned() {
                world.insert_resource(ProgressTrees::new());
                world.insert_resource(ProgressState::new(
                    b.nation_count as usize,
                    b.province_count as usize,
                ));
            }
        }
    }
    fn ensure_armies(&mut self) {
        let world = self.engine.world_mut();
        if world.get_resource::<ArmyRegistry>().is_none() {
            world.insert_resource(ArmyRegistry::new());
        }
    }
    fn ensure_character_gen(&mut self) {
        let world = self.engine.world_mut();
        if world.get_resource::<CharacterGenConfig>().is_none() {
            world.insert_resource(CharacterGenConfig::default());
        }
    }

    fn process_pending_context_action(&mut self) {
        let Some(action) = self.pending_context_action.take() else { return };
        match action {
            PendingContextAction::SetTagProvince(id) => {
                self.ensure_tags();
                self.selected_province = Some(id);
            }
            PendingContextAction::SetTagNation(id) => {
                self.ensure_tags();
                self.selected_nation = Some(id);
            }
            PendingContextAction::AddModifierProvince(id) => {
                self.ensure_modifiers();
                self.selected_province = Some(id);
            }
            PendingContextAction::AddModifierNation(id) => {
                self.ensure_modifiers();
                self.selected_nation = Some(id);
            }
            PendingContextAction::FireEventProvince(id) => {
                self.ensure_events();
                self.selected_province = Some(id);
            }
            PendingContextAction::SpawnArmyProvince(id) => {
                self.ensure_armies();
                self.selected_province = Some(id);
            }
        }
    }

    /// Snapshot current map/province/bounds for undo. Call before applying an edit.
    fn push_undo(&mut self) {
        let world = self.engine.world_mut();
        let Some(map_kind) = world.get_resource::<MapKind>().cloned() else { return };
        let Some(store) = world.get_resource::<ProvinceStore>().cloned() else { return };
        let Some(bounds) = world.get_resource::<WorldBounds>().cloned() else { return };
        self.redo_history.clear();
        self.undo_history.push((map_kind, store, bounds));
        if self.undo_history.len() > self.max_undo {
            self.undo_history.remove(0);
        }
    }

    fn undo(&mut self) -> bool {
        let Some((map_kind, store, bounds)) = self.undo_history.pop() else { return false };
        let world = self.engine.world_mut();
        let Some(current_map) = world.get_resource::<MapKind>().cloned() else { return false };
        let Some(current_store) = world.get_resource::<ProvinceStore>().cloned() else { return false };
        let Some(current_bounds) = world.get_resource::<WorldBounds>().cloned() else { return false };
        self.redo_history.push((current_map, current_store, current_bounds));
        world.insert_resource(map_kind);
        world.insert_resource(store);
        world.insert_resource(bounds);
        true
    }

    fn redo(&mut self) -> bool {
        let Some((map_kind, store, bounds)) = self.redo_history.pop() else { return false };
        let world = self.engine.world_mut();
        let Some(current_map) = world.get_resource::<MapKind>().cloned() else { return false };
        let Some(current_store) = world.get_resource::<ProvinceStore>().cloned() else { return false };
        let Some(current_bounds) = world.get_resource::<WorldBounds>().cloned() else { return false };
        self.undo_history.push((current_map, current_store, current_bounds));
        world.insert_resource(map_kind);
        world.insert_resource(store);
        world.insert_resource(bounds);
        true
    }

    fn ui_media(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(panel_frame())
            .show(ctx, |ui| {
                panel_header(ui, "Media");
                ui.heading("Media");
            ui.add_space(8.0);

            ui.group(|ui| {
                ui.strong("Audio (native)");
                ui.label(
                    egui::RichText::new("Basic playback via kira. Works on desktop; no-op on wasm.")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label("File:");
                    ui.text_edit_singleline(&mut self.audio_path_input);
                    #[cfg(not(target_arch = "wasm32"))]
                    if ui.button("Pick").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_file() {
                            self.audio_path_input = path.display().to_string();
                        }
                    }
                });

                ui.horizontal(|ui| {
                    if ui.button("Play once").clicked() {
                        let p = std::path::PathBuf::from(self.audio_path_input.trim());
                        let _ = self.engine.audio_play_file(&p, false, 0.8);
                    }
                    if ui.button("Loop").clicked() {
                        let p = std::path::PathBuf::from(self.audio_path_input.trim());
                        let _ = self.engine.audio_play_file(&p, true, 0.6);
                    }
                    if ui.button("Volume 100%").clicked() {
                        self.engine.audio_set_master_volume(1.0);
                    }
                    if ui.button("Volume 50%").clicked() {
                        self.engine.audio_set_master_volume(0.5);
                    }
                });
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(12.0);

            ui.group(|ui| {
                ui.strong("Video (cutscene player)");
                ui.label(
                    egui::RichText::new("MP4/H.264 decoding is feature-gated behind `teleology-runtime` feature `video_ffmpeg`.")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("File:");
                    ui.text_edit_singleline(&mut self.video_path_input);
                    #[cfg(not(target_arch = "wasm32"))]
                    if ui.button("Pick").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Video", &["mp4", "m4v", "mov"])
                            .pick_file()
                        {
                            self.video_path_input = path.display().to_string();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Open").clicked() {
                        let p = std::path::PathBuf::from(self.video_path_input.trim());
                        let ok = self.engine.video_open(&p);
                        if !ok {
                            ui.label(
                                egui::RichText::new("Open failed (likely missing `video_ffmpeg` feature or FFmpeg).")
                                    .color(ui.visuals().error_fg_color),
                            );
                        }
                    }
                    if ui.button("Poll frame").clicked() {
                        let _ = self.engine.video_poll_frame();
                    }
                });
                ui.label("Frame rendering to texture will appear once decoding is implemented (FFmpeg path is currently a stub).");
            });
        });
    }

    fn ui_events_editor(&mut self, ctx: &egui::Context) {
        if self.pending_create_event {
            self.ensure_events();
        }
        let world = self.engine.world_mut();

        egui::SidePanel::left("hierarchy")
            .default_width(260.0)
            .resizable(true)
            .frame(panel_frame())
            .show(ctx, |ui| {
                panel_header(ui, "Hierarchy");
                ui.strong("Events");
                ui.add_space(6.0);

                if world.get_resource::<EventRegistry>().is_none() {
                    ui.label("Events initialize on first use: right-click province/nation → Fire event, or use script API.");
                    ui.label("Click New event below to add an event definition.");
                    ui.add_space(6.0);
                }

                ui.horizontal(|ui| {
                    if ui.button("New event").clicked() {
                        self.pending_create_event = true;
                    }
                    if ui.button("Clear link").clicked() {
                        self.event_link_from_choice = None;
                    }
                });

                if self.pending_create_event {
                    if let Some(mut reg) = world.get_resource_mut::<EventRegistry>() {
                        let id = reg.insert(teleology_core::EventDefinition {
                            id: teleology_core::EventId(NonZeroU32::new(1).unwrap()),
                            title: "New Event".to_string(),
                            body: "Edit me.".to_string(),
                            choices: vec![teleology_core::EventChoice {
                                text: "OK".to_string(),
                                next_event: None,
                                effects_payload: Vec::new(),
                            }],
                        });
                        self.event_selected_raw = Some(id.raw());
                    }
                    self.pending_create_event = false;
                }

                // List events.
                let ids: Vec<u32> = world
                    .get_resource::<EventRegistry>()
                    .map(|r| r.events.keys().copied().collect())
                    .unwrap_or_default();
                let mut ids = ids;
                ids.sort_unstable();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for raw in ids {
                        let selected = self.event_selected_raw == Some(raw);
                        if ui.selectable_label(selected, format!("Event {}", raw)).clicked() {
                            self.event_selected_raw = Some(raw);
                        }
                    }
                });
            });

        egui::SidePanel::right("inspector")
            .default_width(320.0)
            .resizable(true)
            .frame(panel_frame())
            .show(ctx, |ui| {
                panel_header(ui, "Inspector");
                ui.strong("Details");
                ui.add_space(6.0);

                let Some(selected_raw) = self.event_selected_raw else {
                    ui.label("Select an event.");
                    return;
                };
                let Some(mut reg) = world.get_resource_mut::<EventRegistry>() else {
                    ui.label("Event registry missing.");
                    return;
                };
                let Some(def) = reg.events.get_mut(&selected_raw) else {
                    ui.label("Event not found.");
                    return;
                };

                ui.horizontal(|ui| {
                    ui.label("Title:");
                    ui.text_edit_singleline(&mut def.title);
                });
                ui.label("Body:");
                ui.text_edit_multiline(&mut def.body);
                ui.add_space(8.0);
                ui.separator();
                ui.strong("Choices");

                let mut add_choice = false;
                if ui.button("Add choice").clicked() {
                    add_choice = true;
                }
                if add_choice {
                    def.choices.push(teleology_core::EventChoice {
                        text: "New choice".to_string(),
                        next_event: None,
                        effects_payload: Vec::new(),
                    });
                }

                for (i, ch) in def.choices.iter_mut().enumerate() {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(format!("#{}", i));
                        ui.text_edit_singleline(&mut ch.text);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Next:");
                        let next_raw = ch.next_event.map(|e| e.raw()).unwrap_or(0);
                        ui.label(if next_raw == 0 {
                            "—".to_string()
                        } else {
                            format!("Event {}", next_raw)
                        });
                        if ui.button("Clear").clicked() {
                            ch.next_event = None;
                        }
                    });
                    ui.label("Tip: in the canvas, click this choice's link handle then click a target event to connect.");
                }
            });

        // Canvas (Scene)
        egui::CentralPanel::default()
            .frame(panel_frame())
            .show(ctx, |ui| {
                panel_header(ui, "Scene");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.strong("Event graph");
                if let Some((from_raw, choice_idx)) = self.event_link_from_choice {
                    ui.add_space(8.0);
                    ui.label(format!("Linking from Event {} choice #{}", from_raw, choice_idx));
                }
            });
            ui.add_space(6.0);

            let reg_snapshot = world.get_resource::<EventRegistry>().cloned();
            let Some(reg_snapshot) = reg_snapshot else {
                ui.label("Events initialize on first use. Click New event (left panel) to add an event, then use the graph.");
                return;
            };

            let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::drag());
            let rect = response.rect;

            // Pan by dragging background.
            if response.dragged() {
                // Use per-frame pointer delta (drag_delta is total).
                let d = ui.input(|i| i.pointer.delta());
                self.event_graph_pan += d;
            }

            let node_size = egui::Vec2::new(200.0, 110.0);
            let mut ids: Vec<u32> = reg_snapshot.events.keys().copied().collect();
            ids.sort_unstable();

            // Ensure default positions
            for (i, raw) in ids.iter().copied().enumerate() {
                self.event_graph_pos.entry(raw).or_insert_with(|| {
                    let col = (i % 4) as f32;
                    let row = (i / 4) as f32;
                    egui::Pos2::new(col * 240.0, row * 150.0)
                });
            }

            // Draw connections first
            for raw in &ids {
                let Some(def) = reg_snapshot.events.get(raw) else { continue };
                let from_pos = *self.event_graph_pos.get(raw).unwrap();
                for (choice_idx, ch) in def.choices.iter().enumerate() {
                    let Some(next) = ch.next_event else { continue };
                    let to_raw = next.raw();
                    let Some(to_pos) = self.event_graph_pos.get(&to_raw).copied() else { continue };

                    let from = rect.min + (from_pos.to_vec2() + self.event_graph_pan)
                        + egui::Vec2::new(node_size.x, 30.0 + choice_idx as f32 * 18.0);
                    let to = rect.min + (to_pos.to_vec2() + self.event_graph_pan)
                        + egui::Vec2::new(0.0, node_size.y * 0.5);
                    painter.line_segment(
                        [from, to],
                        egui::Stroke::new(2.0, ui.visuals().widgets.active.bg_fill),
                    );
                }
            }

            // Draw nodes + interactions
            for raw in ids {
                let Some(def) = reg_snapshot.events.get(&raw) else { continue };
                let pos = *self.event_graph_pos.get(&raw).unwrap();
                let top_left = rect.min + (pos.to_vec2() + self.event_graph_pan);
                let node_rect = egui::Rect::from_min_size(top_left, node_size);

                let id = egui::Id::new(("event_node", raw));
                let resp = ui.interact(node_rect, id, egui::Sense::click_and_drag());

                if resp.dragged() {
                    let d = ui.input(|i| i.pointer.delta());
                    if let Some(p) = self.event_graph_pos.get_mut(&raw) {
                        *p = *p + d;
                    }
                }
                if resp.clicked() {
                    // If linking from a choice, connect it to this node.
                    if let Some((from_raw, choice_idx)) = self.event_link_from_choice.take() {
                        if let Some(mut reg) = world.get_resource_mut::<EventRegistry>() {
                            if let Some(from_def) = reg.events.get_mut(&from_raw) {
                                if let Some(ch) = from_def.choices.get_mut(choice_idx) {
                                    ch.next_event = NonZeroU32::new(raw).map(teleology_core::EventId);
                                }
                            }
                        }
                    } else {
                        self.event_selected_raw = Some(raw);
                    }
                }

                let selected = self.event_selected_raw == Some(raw);
                let fill = if selected {
                    ui.visuals().selection.bg_fill
                } else {
                    ui.visuals().widgets.inactive.bg_fill
                };
                painter.rect_filled(node_rect, 6.0, fill);
                painter.rect_stroke(
                    node_rect,
                    6.0,
                    egui::Stroke::new(1.0, ui.visuals().widgets.inactive.fg_stroke.color),
                );

                painter.text(
                    node_rect.min + egui::Vec2::new(8.0, 6.0),
                    egui::Align2::LEFT_TOP,
                    format!("Event {}", raw),
                    egui::FontId::proportional(14.0),
                    ui.visuals().text_color(),
                );
                painter.text(
                    node_rect.min + egui::Vec2::new(8.0, 26.0),
                    egui::Align2::LEFT_TOP,
                    def.title.clone(),
                    egui::FontId::proportional(13.0),
                    ui.visuals().weak_text_color(),
                );

                // Choice link handles
                for (i, ch) in def.choices.iter().enumerate() {
                    let y = node_rect.min.y + 48.0 + i as f32 * 18.0;
                    painter.text(
                        egui::Pos2::new(node_rect.min.x + 10.0, y),
                        egui::Align2::LEFT_TOP,
                        ch.text.clone(),
                        egui::FontId::proportional(12.0),
                        ui.visuals().text_color(),
                    );
                    let handle_rect = egui::Rect::from_min_size(
                        egui::Pos2::new(node_rect.max.x - 18.0, y + 2.0),
                        egui::Vec2::new(12.0, 12.0),
                    );
                    let hid = egui::Id::new(("event_choice_link", raw, i));
                    let hresp = ui.interact(handle_rect, hid, egui::Sense::click());
                    painter.rect_filled(handle_rect, 3.0, ui.visuals().widgets.active.bg_fill);
                    if hresp.clicked() {
                        self.event_link_from_choice = Some((raw, i));
                    }
                }
            }
        });
    }

    fn ui_progress_trees_editor(&mut self, ctx: &egui::Context) {
        if self.pending_create_tree {
            self.ensure_progress_trees();
        }
        let world = self.engine.world_mut();

        egui::SidePanel::left("hierarchy")
            .default_width(260.0)
            .resizable(true)
            .frame(panel_frame())
            .show(ctx, |ui| {
                panel_header(ui, "Hierarchy");
                ui.strong("Progress Trees");
                ui.add_space(6.0);

                if world.get_resource::<ProgressTrees>().is_none() {
                    ui.label("Progress trees initialize on first use: right-click or use script API.");
                    ui.label("Click New tree below to add a tree.");
                    ui.add_space(6.0);
                }

                ui.horizontal(|ui| {
                    if ui.button("New tree").clicked() {
                        self.pending_create_tree = true;
                    }
                    if ui.button("Clear link").clicked() {
                        self.progress_link_from_node = None;
                    }
                });
                if self.pending_create_tree {
                    if let Some(mut trees) = world.get_resource_mut::<ProgressTrees>() {
                        let id = trees.add_tree("New Tree");
                        self.progress_selected_tree_raw = Some(id.raw());
                    }
                    self.pending_create_tree = false;
                }

                // Tree list
                let tree_list: Vec<(u32, String)> = world
                    .get_resource::<ProgressTrees>()
                    .map(|t| t.trees.iter().map(|tr| (tr.id.raw(), tr.name.clone())).collect())
                    .unwrap_or_default();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (raw, name) in tree_list {
                        let selected = self.progress_selected_tree_raw == Some(raw);
                        if ui.selectable_label(selected, format!("{} ({})", name, raw)).clicked() {
                            self.progress_selected_tree_raw = Some(raw);
                            self.progress_selected_node_raw = None;
                        }
                    }
                });
            });

        egui::SidePanel::right("inspector")
            .default_width(320.0)
            .resizable(true)
            .frame(panel_frame())
            .show(ctx, |ui| {
                panel_header(ui, "Inspector");
                ui.strong("Details");
                ui.add_space(6.0);

                let Some(tree_raw) = self.progress_selected_tree_raw else {
                    ui.label("Select a tree.");
                    return;
                };
                let Some(mut trees) = world.get_resource_mut::<ProgressTrees>() else {
                    ui.label("ProgressTrees missing.");
                    return;
                };
                // Find the tree index first so we can avoid overlapping mutable borrows.
                let Some(tree_idx) = trees.trees.iter().position(|t| t.id.raw() == tree_raw) else {
                    ui.label("Tree not found.");
                    return;
                };

                ui.horizontal(|ui| {
                    ui.label("Tree name:");
                    ui.text_edit_singleline(&mut trees.trees[tree_idx].name);
                });
                ui.add_space(8.0);

                if ui.button("Add node").clicked() {
                    let tree_id = NonZeroU32::new(tree_raw).map(teleology_core::TreeId).unwrap();
                    let node = trees.add_node(
                        tree_id,
                        "New Node",
                        100.0,
                        Vec::new(),
                        Vec::new(),
                    );
                    self.progress_selected_node_raw = Some(node.raw());
                }

                let Some(node_raw) = self.progress_selected_node_raw else {
                    ui.label("Select a node in the graph.");
                    return;
                };

                let Some(node) = trees.trees[tree_idx]
                    .nodes
                    .iter_mut()
                    .find(|n| n.id.raw() == node_raw) else {
                    ui.label("Node not found.");
                    return;
                };

                ui.horizontal(|ui| {
                    ui.label("Node name:");
                    ui.text_edit_singleline(&mut node.name);
                });
                ui.horizontal(|ui| {
                    ui.label("Cost:");
                    let mut cost = node.cost;
                    ui.add(egui::DragValue::new(&mut cost).speed(1.0));
                    node.cost = cost.max(0.0);
                });
                ui.add_space(6.0);
                ui.strong("Prerequisites");
                for prereq in &node.prerequisites {
                    ui.label(format!("Node {}", prereq.raw()));
                }
                ui.label("Tip: on the canvas, click a node's link handle, then click another node to add it as a prerequisite.");
            });

        egui::CentralPanel::default()
            .frame(panel_frame())
            .show(ctx, |ui| {
                panel_header(ui, "Scene");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.strong("Tree graph");
                if let Some(from) = self.progress_link_from_node {
                    ui.add_space(8.0);
                    ui.label(format!("Linking prerequisite from Node {}", from));
                }
            });
            ui.add_space(6.0);

            let Some(tree_raw) = self.progress_selected_tree_raw else {
                ui.label("Select a tree on the left.");
                return;
            };
            let trees_snapshot = world.get_resource::<ProgressTrees>().cloned();
            let Some(trees_snapshot) = trees_snapshot else { return };
            let Some(tree) = trees_snapshot.trees.iter().find(|t| t.id.raw() == tree_raw) else {
                ui.label("Tree missing.");
                return;
            };

            let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::drag());
            let rect = response.rect;
            if response.dragged() {
                let d = ui.input(|i| i.pointer.delta());
                self.progress_graph_pan += d;
            }

            let node_size = egui::Vec2::new(210.0, 80.0);
            // Ensure default positions
            for (i, n) in tree.nodes.iter().enumerate() {
                let key = (tree_raw, n.id.raw());
                self.progress_graph_pos.entry(key).or_insert_with(|| {
                    let col = (i % 4) as f32;
                    let row = (i / 4) as f32;
                    egui::Pos2::new(col * 260.0, row * 130.0)
                });
            }

            // Draw edges: prereq -> node
            for n in &tree.nodes {
                let to_key = (tree_raw, n.id.raw());
                let Some(to_pos) = self.progress_graph_pos.get(&to_key).copied() else { continue };
                for prereq in &n.prerequisites {
                    let from_key = (tree_raw, prereq.raw());
                    let Some(from_pos) = self.progress_graph_pos.get(&from_key).copied() else { continue };
                    let from = rect.min + (from_pos.to_vec2() + self.progress_graph_pan) + egui::Vec2::new(node_size.x, node_size.y * 0.5);
                    let to = rect.min + (to_pos.to_vec2() + self.progress_graph_pan) + egui::Vec2::new(0.0, node_size.y * 0.5);
                    painter.line_segment([from, to], egui::Stroke::new(2.0, ui.visuals().widgets.active.bg_fill));
                }
            }

            // Nodes
            for n in &tree.nodes {
                let raw = n.id.raw();
                let key = (tree_raw, raw);
                let pos = *self.progress_graph_pos.get(&key).unwrap();
                let top_left = rect.min + (pos.to_vec2() + self.progress_graph_pan);
                let node_rect = egui::Rect::from_min_size(top_left, node_size);

                let id = egui::Id::new(("tree_node", tree_raw, raw));
                let resp = ui.interact(node_rect, id, egui::Sense::click_and_drag());

                if resp.dragged() {
                    let d = ui.input(|i| i.pointer.delta());
                    if let Some(p) = self.progress_graph_pos.get_mut(&key) {
                        *p = *p + d;
                    }
                }

                if resp.clicked() {
                    if let Some(from_raw) = self.progress_link_from_node.take() {
                        if from_raw != raw {
                            // Add prerequisite: from_raw -> raw
                            if let Some(mut trees) = world.get_resource_mut::<ProgressTrees>() {
                                if let Some(t) = trees.trees.iter_mut().find(|t| t.id.raw() == tree_raw) {
                                    if let Some(target) = t.nodes.iter_mut().find(|nn| nn.id.raw() == raw) {
                                        let from_id = NonZeroU32::new(from_raw).map(teleology_core::NodeId).unwrap();
                                        if !target.prerequisites.iter().any(|p| p.raw() == from_raw) {
                                            target.prerequisites.push(from_id);
                                        }
                                    }
                                }
                                trees.rebuild_index();
                            }
                        }
                    } else {
                        self.progress_selected_node_raw = Some(raw);
                    }
                }

                let selected = self.progress_selected_node_raw == Some(raw);
                let fill = if selected { ui.visuals().selection.bg_fill } else { ui.visuals().widgets.inactive.bg_fill };
                painter.rect_filled(node_rect, 6.0, fill);
                painter.rect_stroke(
                    node_rect,
                    6.0,
                    egui::Stroke::new(1.0, ui.visuals().widgets.inactive.fg_stroke.color),
                );
                painter.text(
                    node_rect.min + egui::Vec2::new(8.0, 6.0),
                    egui::Align2::LEFT_TOP,
                    format!("Node {}", raw),
                    egui::FontId::proportional(14.0),
                    ui.visuals().text_color(),
                );
                painter.text(
                    node_rect.min + egui::Vec2::new(8.0, 26.0),
                    egui::Align2::LEFT_TOP,
                    n.name.clone(),
                    egui::FontId::proportional(13.0),
                    ui.visuals().weak_text_color(),
                );
                painter.text(
                    node_rect.min + egui::Vec2::new(8.0, 46.0),
                    egui::Align2::LEFT_TOP,
                    format!("Cost: {}", n.cost),
                    egui::FontId::proportional(12.0),
                    ui.visuals().weak_text_color(),
                );

                // Link handle (prereq source)
                let handle_rect = egui::Rect::from_min_size(
                    egui::Pos2::new(node_rect.max.x - 18.0, node_rect.center().y - 6.0),
                    egui::Vec2::new(12.0, 12.0),
                );
                let hid = egui::Id::new(("tree_node_link", tree_raw, raw));
                let hresp = ui.interact(handle_rect, hid, egui::Sense::click());
                painter.rect_filled(handle_rect, 3.0, ui.visuals().widgets.active.bg_fill);
                if hresp.clicked() {
                    self.progress_link_from_node = Some(raw);
                }
            }
        });
    }

    fn ui_map_editor(&mut self, ctx: &egui::Context) {
        let paint_ownership = self.map_paint_mode == MapEditorPaintMode::PaintOwnership;
        let is_irregular = self
            .engine
            .world()
            .get_resource::<MapKind>()
            .map(|mk| matches!(mk, MapKind::Irregular(_)))
            .unwrap_or(false);

        // Left panel: nations when painting ownership (grid), provinces when editing or irregular
        if paint_ownership && !is_irregular {
            self.ui_map_editor_nations_panel(ctx);
        } else {
            self.ui_map_editor_provinces_panel(ctx);
        }

        // Right panel: nations for Edit provinces or Irregular (Assign)
        if !paint_ownership || is_irregular {
            self.ui_map_editor_nations_panel_right(ctx);
        }

        egui::CentralPanel::default()
            .frame(panel_frame())
            .show(ctx, |ui| {
                panel_header(ui, "Scene");
                ui.add_space(6.0);
                let date = self.engine.world().get_resource::<GameDate>().copied().unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.strong("Date:");
                ui.add_space(4.0);
                ui.label(format!("{}-{:02}-{:02}", date.year, date.month, date.day));
            });
            ui.add_space(4.0);

            // Paint mode toggle
            ui.horizontal(|ui| {
                ui.label("Map tool:");
                ui.radio_value(
                    &mut self.map_paint_mode,
                    MapEditorPaintMode::PaintOwnership,
                    "Paint by nation (ownership)",
                );
                ui.radio_value(
                    &mut self.map_paint_mode,
                    MapEditorPaintMode::EditProvinces,
                    "Edit provinces (borders)",
                );
            });
            ui.add_space(4.0);

            if paint_ownership {
                ui.label(
                    egui::RichText::new("Select a nation (left), then click or drag on the map to paint ownership.")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            } else {
                ui.label(
                    egui::RichText::new("Select a province (left), paint on the map, then select a nation (right) and click Assign.")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            }
            ui.add_space(8.0);

            let world = self.engine.world();
            let map_kind = world.get_resource::<MapKind>();
            let store = world.get_resource::<ProvinceStore>();

            #[derive(Clone, Copy)]
            enum TileHit {
                Square(u32, u32),
                Hex(u32, u32),
            }

            let (_map_bounds, tile_hit) = {
                if let (Some(mk), Some(st)) = (map_kind, store) {
                    match mk {
                        MapKind::Square(map) => {
                            let cell_size = 14.0;
                            let (response, painter) = ui.allocate_painter(
                                egui::Vec2::new(
                                    map.width as f32 * cell_size,
                                    map.height as f32 * cell_size,
                                ),
                                egui::Sense::click_and_drag(),
                            );
                            let rect = response.rect;
                            for y in 0..map.height {
                                for x in 0..map.width {
                                    let raw = map.get(x, y);
                                    let color = if raw == 0 {
                                        TILE_EMPTY_COLOR
                                    } else {
                                        let owner = st
                                            .get(ProvinceId(NonZeroU32::new(raw).unwrap()))
                                            .and_then(|p| p.owner)
                                            .map(|n| n.0.get())
                                            .unwrap_or(0);
                                        if owner == 0 {
                                            TILE_UNOWNED_PROVINCE_COLOR
                                        } else {
                                            nation_color(owner)
                                        }
                                    };
                                    let xf = rect.min.x + x as f32 * cell_size;
                                    let yf = rect.min.y + y as f32 * cell_size;
                                    painter.rect_filled(
                                        egui::Rect::from_min_size(
                                            egui::Pos2::new(xf, yf),
                                            egui::Vec2::new(cell_size - 1.0, cell_size - 1.0),
                                        ),
                                        0.0,
                                        color,
                                    );
                                }
                            }
                            // Province borders so same-nation provinces are distinguishable
                            for y in 0..map.height {
                                for x in 0..map.width {
                                    let raw = map.get(x, y);
                                    let xf = rect.min.x + x as f32 * cell_size;
                                    let yf = rect.min.y + y as f32 * cell_size;
                                    if x + 1 < map.width && map.get(x + 1, y) != raw {
                                        painter.line_segment(
                                            [
                                                egui::Pos2::new(xf + cell_size, yf),
                                                egui::Pos2::new(xf + cell_size, yf + cell_size),
                                            ],
                                            province_border_stroke(),
                                        );
                                    }
                                    if y + 1 < map.height && map.get(x, y + 1) != raw {
                                        painter.line_segment(
                                            [
                                                egui::Pos2::new(xf, yf + cell_size),
                                                egui::Pos2::new(xf + cell_size, yf + cell_size),
                                            ],
                                            province_border_stroke(),
                                        );
                                    }
                                }
                            }
                            let to_tile = |pos: egui::Pos2| {
                                let rx = ((pos.x - rect.min.x) / cell_size) as u32;
                                let ry = ((pos.y - rect.min.y) / cell_size) as u32;
                                (rx < map.width && ry < map.height).then(|| TileHit::Square(rx, ry))
                            };
                            let hit = response.clicked()
                                .then(|| response.interact_pointer_pos())
                                .flatten()
                                .and_then(to_tile)
                                .or_else(|| {
                                    response.dragged()
                                        .then(|| response.interact_pointer_pos())
                                        .flatten()
                                        .and_then(to_tile)
                                });
                            (Some((map.width, map.height)), hit)
                        }
                        MapKind::Hex(hex) => {
                            let cell_size = 14.0;
                            let w = hex.width;
                            let h = hex.height;
                            let hex_w = cell_size * 1.732;
                            let hex_h = cell_size * 2.0;
                            let total_w = w as f32 * hex_w * 0.75 + hex_w * 0.25;
                            let total_h = h as f32 * hex_h * 0.5 + hex_h * 0.5;
                            let (response, painter) = ui.allocate_painter(
                                egui::Vec2::new(total_w, total_h),
                                egui::Sense::click_and_drag(),
                            );
                            let rect = response.rect;
                            for r in 0..h {
                                for q in 0..w {
                                    let raw = hex.get(q, r);
                                    let cx = rect.min.x + (q as f32 + 0.5 * (r % 2) as f32) * hex_w;
                                    let cy = rect.min.y + (r as f32 + 0.5) * hex_h * 0.5;
                                    let color = if raw == 0 {
                                        TILE_EMPTY_COLOR
                                    } else {
                                        let owner = st
                                            .get(ProvinceId(NonZeroU32::new(raw).unwrap()))
                                            .and_then(|p| p.owner)
                                            .map(|n| n.0.get())
                                            .unwrap_or(0);
                                        if owner == 0 {
                                            TILE_UNOWNED_PROVINCE_COLOR
                                        } else {
                                            nation_color(owner)
                                        }
                                    };
                                    let mut points = Vec::with_capacity(6);
                                    for i in 0..6 {
                                        let a = std::f32::consts::FRAC_PI_3 * (i as f32);
                                        points.push(egui::Pos2::new(
                                            cx + cell_size * a.cos(),
                                            cy + cell_size * a.sin(),
                                        ));
                                    }
                                    painter.add(egui::Shape::convex_polygon(
                                        points,
                                        color,
                                        province_border_stroke(),
                                    ));
                                }
                            }
                            let to_hex = |pos: egui::Pos2| {
                                let px = (pos.x - rect.min.x) / hex_w;
                                let py = (pos.y - rect.min.y) / (hex_h * 0.5);
                                let r = (py - 0.5).floor() as u32;
                                let q = (px - 0.5 * (r % 2) as f32).floor() as u32;
                                (q < w && r < h).then(|| TileHit::Hex(q, r))
                            };
                            let hit = response.clicked()
                                .then(|| response.interact_pointer_pos())
                                .flatten()
                                .and_then(to_hex)
                                .or_else(|| {
                                    response.dragged()
                                        .then(|| response.interact_pointer_pos())
                                        .flatten()
                                        .and_then(to_hex)
                                });
                            (Some((w, h)), hit)
                        }
                        MapKind::Irregular(vec) => {
                            ui.label(format!(
                                "Irregular map: {} provinces. Use Assign (below) to set owners.",
                                vec.polygons.len()
                            ));
                            (None, None)
                        }
                    }
                } else {
                    ui.label("No map. Create world with map_size(), map_hex(), or load a map.");
                    (None, None)
                }
            };

            if let Some(hit) = tile_hit {
                if !self.stroke_undo_pushed {
                    self.push_undo();
                    self.stroke_undo_pushed = true;
                }
                match hit {
                    TileHit::Square(rx, ry) => {
                        if paint_ownership {
                            let raw = self
                                .engine
                                .world()
                                .get_resource::<MapKind>()
                                .and_then(|mk| match mk { MapKind::Square(m) => Some(m.get(rx, ry)), _ => None })
                                .unwrap_or(0);
                            if raw != 0 {
                                if let Some(nid) = self.selected_nation {
                                    let world_mut = self.engine.world_mut();
                                    let mut store = world_mut.get_resource_mut::<ProvinceStore>().unwrap();
                                    let pid = ProvinceId(NonZeroU32::new(raw).unwrap());
                                    let nation_id = NationId(NonZeroU32::new(nid).unwrap());
                                    if let Some(p) = store.get_mut(pid) {
                                        p.owner = Some(nation_id);
                                    }
                                }
                            }
                        } else {
                            let selected_prov = self.selected_province.unwrap_or(1);
                            if let Some(mut mk) = self.engine.world_mut().get_resource_mut::<MapKind>() {
                                if let MapKind::Square(ref mut m) = *mk {
                                    m.set(rx, ry, selected_prov);
                                }
                            }
                        }
                    }
                    TileHit::Hex(q, r) => {
                        if paint_ownership {
                            let raw = self
                                .engine
                                .world()
                                .get_resource::<MapKind>()
                                .and_then(|mk| match mk { MapKind::Hex(m) => Some(m.get(q, r)), _ => None })
                                .unwrap_or(0);
                            if raw != 0 {
                                if let Some(nid) = self.selected_nation {
                                    let world_mut = self.engine.world_mut();
                                    let mut store = world_mut.get_resource_mut::<ProvinceStore>().unwrap();
                                    let pid = ProvinceId(NonZeroU32::new(raw).unwrap());
                                    let nation_id = NationId(NonZeroU32::new(nid).unwrap());
                                    if let Some(p) = store.get_mut(pid) {
                                        p.owner = Some(nation_id);
                                    }
                                }
                            }
                        } else {
                            let selected_prov = self.selected_province.unwrap_or(1);
                            if let Some(mut mk) = self.engine.world_mut().get_resource_mut::<MapKind>() {
                                if let MapKind::Hex(ref mut m) = *mk {
                                    m.set(q, r, selected_prov);
                                }
                            }
                        }
                    }
                }
            }
            if !paint_ownership || is_irregular {
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
                if let (Some(pid), Some(nid)) = (self.selected_province, self.selected_nation) {
                    let assign_clicked = ui
                        .button(egui::RichText::new("Assign province → nation").color(egui::Color32::WHITE))
                        .on_hover_text("Set the selected province's owner to the selected nation")
                        .clicked();
                    if assign_clicked {
                        self.push_undo();
                        let world = self.engine.world_mut();
                        let Some(mut store) = world.get_resource_mut::<ProvinceStore>() else { return };
                        let prov_id = ProvinceId(NonZeroU32::new(pid).unwrap());
                        let nation_id = NationId(NonZeroU32::new(nid).unwrap());
                        if let Some(p) = store.get_mut(prov_id) {
                            p.owner = Some(nation_id);
                        }
                    }
                } else {
                    ui.label(
                        egui::RichText::new("Select a province (left) and a nation (right) to enable Assign.")
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    );
                }
            }
            ui.add_space(8.0);

            // Pop-up events (optional).
            {
                let world = self.engine.world_mut();
                pull_next_event(world);
                let inst = world
                    .get_resource::<ActiveEvent>()
                    .and_then(|a| a.current.clone());
                let def = inst
                    .as_ref()
                    .and_then(|i| world.get_resource::<EventRegistry>().and_then(|r| r.get(i.event_id)).cloned());

                if let (Some(inst), Some(def)) = (inst, def) {
                    let mut chosen_next: Option<teleology_core::EventId> = None;
                    let mut close = false;
                    egui::Window::new(def.title.clone())
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.label(&def.body);
                            ui.add_space(8.0);
                            for ch in &def.choices {
                                if ui.button(&ch.text).clicked() {
                                    close = true;
                                    chosen_next = ch.next_event;
                                }
                            }
                        });

                    if close {
                        if let Some(mut active) = world.get_resource_mut::<ActiveEvent>() {
                            active.current = None;
                        }
                        if let Some(next) = chosen_next {
                            queue_event(world, next, inst.scope, inst.payload.clone());
                        }
                    }
                }
            }
        });
    }

    fn ui_map_editor_nations_panel(&mut self, ctx: &egui::Context) {
        self.process_pending_context_action();
        egui::SidePanel::left("hierarchy")
            .default_width(260.0)
            .resizable(true)
            .frame(panel_frame())
            .show(ctx, |ui| {
                panel_header(ui, "Hierarchy");
                ui.add_space(4.0);
                ui.strong("Nations");
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("Click one, then paint on the map to set ownership.")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);
                let world = self.engine.world();
                let (bounds, nations) = (
                    world.get_resource::<WorldBounds>(),
                    world.get_resource::<NationStore>(),
                );
                if let (Some(bounds), Some(nations)) = (bounds, nations) {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            for (i, n) in nations.nations.iter().enumerate().take(bounds.nation_count as usize) {
                                let id = (i + 1) as u32;
                                let selected = self.selected_nation == Some(id);
                                let label_response = ui.horizontal(|ui| {
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::Vec2::new(12.0, 12.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().rect_filled(rect, 2.0, nation_color(id));
                                    ui.add_space(6.0);
                                    ui.selectable_label(selected, format!("Nation {}", id))
                                        .on_hover_text(format!(
                                            "Prestige: {}  Stability: {}  Treasury: {}",
                                            n.prestige, n.stability, n.treasury,
                                        ))
                                }).inner;
                                if label_response.clicked() {
                                    self.selected_nation = Some(id);
                                }
                                label_response.context_menu(|ui| {
                                    ui.label("Nation actions (init on first use):");
                                    ui.separator();
                                    if ui.button("Set tag…").clicked() {
                                        self.pending_context_action = Some(PendingContextAction::SetTagNation(id));
                                        ui.close_menu();
                                    }
                                    if ui.button("Add modifier…").clicked() {
                                        self.pending_context_action = Some(PendingContextAction::AddModifierNation(id));
                                        ui.close_menu();
                                    }
                                });
                                ui.add_space(2.0);
                            }
                        });
                } else {
                    ui.label("No world loaded.");
                }
                ui.add_space(8.0);
            });
    }

    fn ui_map_editor_provinces_panel(&mut self, ctx: &egui::Context) {
        self.process_pending_context_action();
        egui::SidePanel::left("hierarchy")
            .default_width(260.0)
            .resizable(true)
            .frame(panel_frame())
            .show(ctx, |ui| {
                panel_header(ui, "Hierarchy");
                ui.add_space(4.0);
                ui.strong("Provinces");
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("Click one to select it for painting on the map. Add province to create new ones.")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
                ui.add_space(6.0);
                if ui.button("Add province").clicked() {
                    self.push_undo();
                    if let Some(new_id) = add_province_to_world(self.engine.world_mut()) {
                        self.selected_province = Some(new_id);
                    }
                }
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);
                let world = self.engine.world();
                let (bounds, provinces) = (
                    world.get_resource::<WorldBounds>(),
                    world.get_resource::<ProvinceStore>(),
                );
                if let (Some(bounds), Some(provinces)) = (bounds, provinces) {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            for (i, p) in provinces.provinces.iter().enumerate().take(bounds.province_count as usize) {
                                let id = (i + 1) as u32;
                                let selected = self.selected_province == Some(id);
                                let owner_raw = p.owner.map(|n| n.0.get()).unwrap_or(0);
                                let label_response = ui.horizontal(|ui| {
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::Vec2::new(12.0, 12.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().rect_filled(
                                        rect,
                                        2.0,
                                        nation_color(owner_raw),
                                    );
                                    ui.add_space(6.0);
                                    ui.selectable_label(
                                        selected,
                                        format!("Province {}", id),
                                    )
                                    .on_hover_text(format!(
                                        "Owner: {}  Dev: {}/{}/{}",
                                        if owner_raw == 0 { "—" } else { &owner_raw.to_string() },
                                        p.development[0],
                                        p.development[1],
                                        p.development[2],
                                    ))
                                }).inner;
                                if label_response.clicked() {
                                    self.selected_province = Some(id);
                                }
                                label_response.context_menu(|ui| {
                                    ui.label("Province actions (init on first use):");
                                    ui.separator();
                                    if ui.button("Set tag…").clicked() {
                                        self.pending_context_action = Some(PendingContextAction::SetTagProvince(id));
                                        ui.close_menu();
                                    }
                                    if ui.button("Add modifier…").clicked() {
                                        self.pending_context_action = Some(PendingContextAction::AddModifierProvince(id));
                                        ui.close_menu();
                                    }
                                    if ui.button("Fire event…").clicked() {
                                        self.pending_context_action = Some(PendingContextAction::FireEventProvince(id));
                                        ui.close_menu();
                                    }
                                    if ui.button("Spawn army here").clicked() {
                                        self.pending_context_action = Some(PendingContextAction::SpawnArmyProvince(id));
                                        ui.close_menu();
                                    }
                                });
                                ui.add_space(2.0);
                            }
                        });
                } else {
                    ui.label("No world loaded.");
                }
                ui.add_space(8.0);
            });
    }

    fn ui_map_editor_nations_panel_right(&mut self, ctx: &egui::Context) {
        self.process_pending_context_action();
        egui::SidePanel::right("inspector")
            .default_width(220.0)
            .resizable(true)
            .frame(panel_frame())
            .show(ctx, |ui| {
                panel_header(ui, "Inspector");
                ui.add_space(4.0);
                ui.strong("Nations");
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("Select one, then use Assign (center) to set province owner.")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);
                let world = self.engine.world();
                let (bounds, nations) = (
                    world.get_resource::<WorldBounds>(),
                    world.get_resource::<NationStore>(),
                );
                if let (Some(bounds), Some(nations)) = (bounds, nations) {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            for (i, n) in nations.nations.iter().enumerate().take(bounds.nation_count as usize) {
                                let id = (i + 1) as u32;
                                let selected = self.selected_nation == Some(id);
                                let label_response = ui.horizontal(|ui| {
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::Vec2::new(12.0, 12.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().rect_filled(rect, 2.0, nation_color(id));
                                    ui.add_space(6.0);
                                    ui.selectable_label(selected, format!("Nation {}", id))
                                        .on_hover_text(format!(
                                            "Prestige: {}  Stability: {}  Treasury: {}",
                                            n.prestige, n.stability, n.treasury,
                                        ))
                                }).inner;
                                if label_response.clicked() {
                                    self.selected_nation = Some(id);
                                }
                                label_response.context_menu(|ui| {
                                    ui.label("Nation actions (init on first use):");
                                    ui.separator();
                                    if ui.button("Set tag…").clicked() {
                                        self.pending_context_action = Some(PendingContextAction::SetTagNation(id));
                                        ui.close_menu();
                                    }
                                    if ui.button("Add modifier…").clicked() {
                                        self.pending_context_action = Some(PendingContextAction::AddModifierNation(id));
                                        ui.close_menu();
                                    }
                                });
                                ui.add_space(2.0);
                            }
                        });
                } else {
                    ui.label("No world loaded.");
                }
                ui.add_space(8.0);
            });
    }

    fn ui_world(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(panel_frame())
            .show(ctx, |ui| {
                panel_header(ui, "Scene");
                ui.heading("World");
            let world = self.engine.world();
            let date = world.get_resource::<GameDate>().copied().unwrap_or_default();
            ui.label(format!("Date: {}-{:02}-{:02}", date.year, date.month, date.day));
            if let Some(bounds) = world.get_resource::<WorldBounds>() {
                ui.label(format!("Provinces: {}", bounds.province_count));
                ui.label(format!("Nations: {}", bounds.nation_count));
            }
            if let Some(mk) = world.get_resource::<MapKind>() {
                match mk {
                    MapKind::Square(m) => {
                        ui.label(format!("Map: Square {}×{}", m.width, m.height));
                    }
                    MapKind::Hex(m) => {
                        ui.label(format!("Map: Hex {}×{}", m.width, m.height));
                    }
                    MapKind::Irregular(v) => {
                        ui.label(format!("Map: Irregular ({} polygons)", v.polygons.len()));
                    }
                };
            }
            ui.add_space(16.0);
            ui.label("Use Map Editor mode to paint provinces and load/save maps.");
            ui.label("Use Run / Pause / Tick to advance the simulation.");
        });
    }

    fn ui_settings(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(panel_frame())
            .show(ctx, |ui| {
                panel_header(ui, "Settings");
                ui.heading("Settings");
            ui.add_space(8.0);
            ui.label("Gameplay modules initialize on first use: right-click province/nation for submenus (Set tag, Add modifier, Fire event, Spawn army), or use script API.");
            ui.add_space(12.0);

            let world = self.engine.world_mut();
            let _bounds = world.get_resource::<WorldBounds>().cloned();

            ui.collapsing("Tags", |ui| {
                if world.get_resource::<TagRegistry>().is_none() {
                    ui.label("Tags initialize on first use (right-click province/nation → Set tag, or script API).");
                    return;
                }

                ui.horizontal(|ui| {
                    ui.label("New tag type:");
                    ui.text_edit_singleline(&mut self.tag_type_name_input);
                    if ui.button("Register type").clicked() {
                        if let Some(mut reg) = world.get_resource_mut::<TagRegistry>() {
                            reg.register_type(self.tag_type_name_input.trim().to_string());
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Tag type raw:");
                    ui.text_edit_singleline(&mut self.tag_type_raw_input);
                    ui.label("New tag:");
                    ui.text_edit_singleline(&mut self.tag_name_input);
                    if ui.button("Register tag").clicked() {
                        let ty_raw = self.tag_type_raw_input.trim().parse::<u32>().unwrap_or(0);
                        if let (Some(ty), Some(mut reg)) =
                            (NonZeroU32::new(ty_raw).map(TagTypeId), world.get_resource_mut::<TagRegistry>())
                        {
                            reg.register_tag(ty, self.tag_name_input.trim().to_string());
                        }
                    }
                });

                // Assign to selected province/nation (debug workflow).
                ui.add_space(8.0);
                ui.label("Assign (debug): enter type_raw + tag_raw, then apply to selected province/nation.");
                ui.horizontal(|ui| {
                    ui.label("type_raw:");
                    ui.text_edit_singleline(&mut self.tag_type_raw_input);
                    ui.label("tag_raw:");
                    ui.text_edit_singleline(&mut self.tag_raw_input);
                });
                let ty_raw = self.tag_type_raw_input.trim().parse::<u32>().unwrap_or(0);
                let tag_raw = self.tag_raw_input.trim().parse::<u32>().unwrap_or(0);
                let ty = NonZeroU32::new(ty_raw).map(TagTypeId);
                let tag = NonZeroU32::new(tag_raw).map(TagId);

                if let (Some(pid_raw), Some(ty), Some(tag)) = (self.selected_province, ty, tag) {
                    if ui.button("Set tag on selected province").clicked() {
                        if let Some(mut pt) = world.get_resource_mut::<ProvinceTags>() {
                            pt.set(ProvinceId(NonZeroU32::new(pid_raw).unwrap()), ty, tag);
                        }
                    }
                }
                if let (Some(nid_raw), Some(ty), Some(tag)) = (self.selected_nation, ty, tag) {
                    if ui.button("Set tag on selected nation").clicked() {
                        if let Some(mut nt) = world.get_resource_mut::<NationTags>() {
                            nt.set(NationId(NonZeroU32::new(nid_raw).unwrap()), ty, tag);
                        }
                    }
                }
            });

            ui.separator();
            ui.collapsing("Modifiers", |ui| {
                if world.get_resource::<ProvinceModifiers>().is_none() {
                    ui.label("Modifiers initialize on first use (right-click province/nation → Add modifier, or script API).");
                } else {
                    ui.label("Modifiers in use.");
                }
            });

            ui.separator();
            ui.collapsing("Pop-up Events", |ui| {
                if world.get_resource::<EventRegistry>().is_none() {
                    ui.label("Events initialize on first use (right-click → Fire event, Events mode New event, or script API).");
                    return;
                }
                if ui.button("Add + queue demo event").clicked() {
                    let mut reg = world.get_resource_mut::<EventRegistry>().unwrap();
                    let id = reg.insert(teleology_core::EventDefinition {
                        id: teleology_core::EventId(NonZeroU32::new(1).unwrap()),
                        title: "Demo Event".to_string(),
                        body: "Choose an option.".to_string(),
                        choices: vec![
                            teleology_core::EventChoice {
                                text: "OK".to_string(),
                                next_event: None,
                                effects_payload: Vec::new(),
                            },
                            teleology_core::EventChoice {
                                text: "Chain".to_string(),
                                next_event: None,
                                effects_payload: Vec::new(),
                            },
                        ],
                    });
                    queue_event(world, id, teleology_core::EventScope::Global, Vec::new());
                }
            });

            ui.separator();
            ui.collapsing("EventBus", |ui| {
                if world.get_resource::<EventBus>().is_none() {
                    ui.label("EventBus initializes on first use (script API publish/poll).");
                    return;
                }
                ui.horizontal(|ui| {
                    ui.label("Topic:");
                    ui.text_edit_singleline(&mut self.event_topic_input);
                    ui.label("Payload (utf8):");
                    ui.text_edit_singleline(&mut self.event_payload_input);
                    if ui.button("Publish").clicked() {
                        teleology_core::publish_event(
                            world,
                            self.event_topic_input.trim(),
                            teleology_core::EventScopeRef::Global,
                            1,
                            self.event_payload_input.as_bytes().to_vec(),
                            0,
                        );
                    }
                });
                let queued = world.get_resource::<EventBus>().map(|b| b.queue.len()).unwrap_or(0);
                ui.label(format!("Queued messages: {}", queued));
            });

            ui.separator();
            ui.collapsing("Progress Trees", |ui| {
                if world.get_resource::<ProgressTrees>().is_none() {
                    ui.label("Progress trees initialize on first use (Progress Trees mode New tree, or script API).");
                    return;
                }
                let tree_count = world.get_resource::<ProgressTrees>().map(|t| t.trees.len()).unwrap_or(0);
                ui.label(format!("Trees: {}", tree_count));
            });

            ui.separator();
            ui.collapsing("Armies", |ui| {
                if world.get_resource::<ArmyRegistry>().is_none() {
                    ui.label("Armies initialize on first use (right-click province → Spawn army, or script API).");
                    return;
                }
                if let (Some(nid_raw), Some(pid_raw)) = (self.selected_nation, self.selected_province) {
                    if ui.button("Spawn army at selected province").clicked() {
                        let owner = NationId(NonZeroU32::new(nid_raw).unwrap());
                        let loc = ProvinceId(NonZeroU32::new(pid_raw).unwrap());
                        spawn_army(world, owner, loc, ArmyComposition::default());
                    }
                } else {
                    ui.label("Select a nation and province to spawn an army.");
                }
                let mut q = world.query::<&Army>();
                let count = q.iter(world).count();
                ui.label(format!("Armies in world: {}", count));
            });

            ui.separator();
            ui.collapsing("Character Generator", |ui| {
                if world.get_resource::<CharacterGenConfig>().is_none() {
                    ui.label("Character generator initializes on first use (script API).");
                } else {
                    ui.label("Character generator in use.");
                }
            });
        });
    }
}

// --- WebGL entry point: called when the wasm module loads in the browser ---
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn run() {
    console_error_panic_hook::init_once();
    spawn_local(async {
        let web_options = eframe::WebOptions::default();
        let _ = eframe::WebRunner::new()
            .start(
                "teleology_canvas",
                web_options,
                Box::new(|cc| Ok(Box::new(EditorApp::new(cc)))),
            )
            .await;
    });
}
