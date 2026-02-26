//! Visual editor for the Teleology engine. Modes: Map Editor, World view, Settings.
//! Runs on Windows, Mac, Linux (native) and WebGL (browser).

mod utils;
mod map_editor;
mod events_editor;
mod progress_trees_editor;
mod world;
mod media;

use eframe::egui;
use std::collections::HashSet;
use std::num::NonZeroU32;
use teleology_core::{
    compute_adjacency,
    ArmyRegistry, CharacterGenConfig, EventBus, EventQueue, EventRegistry,
    MapFile, MapKind, NationModifiers, NationTags, ProgressState,
    ProgressTrees, ProvinceModifiers, ProvinceStore, ProvinceTags, TagRegistry,
    ActiveEvent, WorldBounds, Viewport,
};
use teleology_runtime::EngineContext;
use utils::*;

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

/// Province or Nation — used in unified context actions.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    Province,
    Nation,
}

/// Deferred context-menu action (run after panel so we don't hold world while ensuring).
#[derive(Clone, Copy)]
enum PendingContextAction {
    SetTag(ScopeKind, u32),
    AddModifier(ScopeKind, u32),
    FireEventProvince(u32),
    SpawnArmyProvince(u32),
    // Event editor actions
    DeleteEvent(u32),
    DuplicateEvent(u32),
    ClearEventConnections(u32),
    // Progress tree editor actions
    DeleteTree(u32),
    DeleteNode(u32, u32),         // (tree_raw, node_raw)
    ClearNodePrereqs(u32, u32),   // (tree_raw, node_raw)
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

    // --- Map canvas: zoom/pan ---
    map_zoom: f32,
    map_pan: egui::Vec2,
    /// Brush radius: 0 = single tile (1x1), 1 = 3x3, 2 = 5x5.
    brush_radius: u32,
    /// Show province/nation name labels on the map.
    show_map_names: bool,

    // --- Media browser state ---
    /// Cached list of audio files in resources/audio/.
    audio_files: Vec<std::path::PathBuf>,
    /// Cached list of image files in resources/assets/.
    image_files: Vec<std::path::PathBuf>,
    /// Selected audio file index.
    media_selected_audio: Option<usize>,
    /// Selected image file index.
    media_selected_image: Option<usize>,
    /// Cached image textures (path string → texture handle).
    media_textures: std::collections::HashMap<std::path::PathBuf, egui::TextureHandle>,
    /// Cached list of font files in resources/fonts/.
    font_files: Vec<std::path::PathBuf>,
    /// Selected font file index.
    media_selected_font: Option<usize>,
    /// Names of loaded custom font families (one per loaded file).
    loaded_font_families: Vec<String>,
    /// Preview text for font preview.
    font_preview_text: String,

    // --- Resource browser (Unity-style Project panel) ---
    /// Currently browsed directory (relative to project root).
    project_dir: std::path::PathBuf,
    /// Cached entries in the current directory.
    project_entries: Vec<ResourceEntry>,
    /// Selected entry index.
    project_selected: Option<usize>,
    /// Grid tile size (px).
    project_tile_size: f32,
    /// Whether the resource browser has been scanned at least once.
    project_scanned: bool,
    /// Thumbnail cache: path → texture handle (for image files).
    project_thumbnails: std::collections::HashMap<std::path::PathBuf, egui::TextureHandle>,
    /// Folder tree: all directories under resources/ (cached on scan).
    project_folders: Vec<std::path::PathBuf>,
    /// Index of entry being dragged internally (file → folder move).
    project_dragging: Option<usize>,
    /// Status message for drag-and-drop operations (e.g. "Copied 3 files").
    project_drop_status: String,
    /// Tick counter for status message fade-out.
    project_drop_status_ttl: u32,

    // --- Hierarchy tree view (Unity-style nested location ownership) ---
    /// Which nation nodes are collapsed in the hierarchy tree. 0 = "Unowned" group.
    hierarchy_collapsed: HashSet<u32>,

    // --- New World creation form ---
    new_world_province_count: u32,
    new_world_nation_count: u32,
    new_world_map_width: u32,
    new_world_map_height: u32,
    /// 0 = Square, 1 = Hex
    new_world_map_type: u32,
}

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut engine = EngineContext::new();
        engine.set_hot_reload(true);
        let (font_files, loaded_font_families) = load_custom_fonts(&cc.egui_ctx);
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
            map_zoom: 1.0,
            map_pan: egui::Vec2::ZERO,
            brush_radius: 0,
            show_map_names: false,
            audio_files: scan_resource_dir("audio", &["mp3", "ogg", "wav", "flac", "aac"]),
            image_files: scan_resource_dir("assets", &["png", "jpg", "jpeg", "bmp", "webp"]),
            media_selected_audio: None,
            media_selected_image: None,
            media_textures: std::collections::HashMap::new(),
            font_files,
            media_selected_font: None,
            loaded_font_families,
            font_preview_text: "The quick brown fox jumps over the lazy dog.\n0123456789 !@#$%^&*()".to_string(),

            project_dir: std::path::PathBuf::from("resources"),
            project_entries: Vec::new(),
            project_selected: None,
            project_tile_size: 72.0,
            project_scanned: false,
            project_thumbnails: std::collections::HashMap::new(),
            project_folders: Vec::new(),
            project_dragging: None,
            project_drop_status: String::new(),
            project_drop_status_ttl: 0,

            hierarchy_collapsed: HashSet::new(),

            new_world_province_count: 100,
            new_world_nation_count: 20,
            new_world_map_width: 20,
            new_world_map_height: 10,
            new_world_map_type: 0,
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
        // Map editor shortcuts: Tab toggles paint mode, [ / ] change brush size
        if self.mode == EditorMode::MapEditor {
            if ctx.input(|i| i.key_pressed(egui::Key::Tab)) {
                self.map_paint_mode = match self.map_paint_mode {
                    MapEditorPaintMode::PaintOwnership => MapEditorPaintMode::EditProvinces,
                    MapEditorPaintMode::EditProvinces => MapEditorPaintMode::PaintOwnership,
                };
            }
            if ctx.input(|i| i.key_pressed(egui::Key::OpenBracket)) {
                self.brush_radius = self.brush_radius.saturating_sub(1);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::CloseBracket)) {
                self.brush_radius = (self.brush_radius + 1).min(3);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::N)) {
                self.show_map_names = !self.show_map_names;
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

        // --- Feed viewport state to the Viewport resource for raycast ---
        {
            let world = self.engine.world_mut();
            if let Some(mut vp) = world.get_resource_mut::<Viewport>() {
                vp.base_cell = 14.0;
                vp.zoom = self.map_zoom;
                vp.pan_x = self.map_pan.x;
                vp.pan_y = self.map_pan.y;
            }
        }

        // --- Script game UI (immediate-mode command buffer) ---
        self.render_game_ui(ctx);

        // --- Bottom panel: Resource Browser (Unity-style Project panel) ---
        self.ui_project_browser(ctx);
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
            PendingContextAction::SetTag(scope, id) => {
                self.ensure_tags();
                match scope {
                    ScopeKind::Province => self.selected_province = Some(id),
                    ScopeKind::Nation => self.selected_nation = Some(id),
                }
            }
            PendingContextAction::AddModifier(scope, id) => {
                self.ensure_modifiers();
                match scope {
                    ScopeKind::Province => self.selected_province = Some(id),
                    ScopeKind::Nation => self.selected_nation = Some(id),
                }
            }
            PendingContextAction::FireEventProvince(id) => {
                self.ensure_events();
                self.selected_province = Some(id);
            }
            PendingContextAction::SpawnArmyProvince(id) => {
                self.ensure_armies();
                self.selected_province = Some(id);
            }
            PendingContextAction::DeleteEvent(raw) => {
                if let Some(id) = NonZeroU32::new(raw).map(teleology_core::EventId) {
                    let world = self.engine.world_mut();
                    if let Some(mut reg) = world.get_resource_mut::<EventRegistry>() {
                        reg.remove(id);
                    }
                }
                if self.event_selected_raw == Some(raw) {
                    self.event_selected_raw = None;
                }
                self.event_graph_pos.remove(&raw);
            }
            PendingContextAction::DuplicateEvent(raw) => {
                if let Some(id) = NonZeroU32::new(raw).map(teleology_core::EventId) {
                    let world = self.engine.world_mut();
                    if let Some(mut reg) = world.get_resource_mut::<EventRegistry>() {
                        if let Some(new_id) = reg.duplicate(id) {
                            let new_raw = new_id.raw();
                            // Place duplicate slightly offset from original
                            let pos = self.event_graph_pos.get(&raw).copied()
                                .unwrap_or(egui::Pos2::new(100.0, 100.0));
                            self.event_graph_pos.insert(new_raw, pos + egui::Vec2::new(30.0, 30.0));
                            self.event_selected_raw = Some(new_raw);
                        }
                    }
                }
            }
            PendingContextAction::ClearEventConnections(raw) => {
                let world = self.engine.world_mut();
                if let Some(mut reg) = world.get_resource_mut::<EventRegistry>() {
                    if let Some(def) = reg.events.get_mut(&raw) {
                        for ch in &mut def.choices {
                            ch.next_event = None;
                        }
                    }
                }
            }
            PendingContextAction::DeleteTree(raw) => {
                if let Some(id) = NonZeroU32::new(raw).map(teleology_core::TreeId) {
                    let world = self.engine.world_mut();
                    if let Some(mut trees) = world.get_resource_mut::<ProgressTrees>() {
                        trees.remove_tree(id);
                    }
                }
                if self.progress_selected_tree_raw == Some(raw) {
                    self.progress_selected_tree_raw = None;
                    self.progress_selected_node_raw = None;
                }
                // Clean up graph positions for nodes in this tree
                self.progress_graph_pos.retain(|&(tr, _), _| tr != raw);
            }
            PendingContextAction::DeleteNode(tree_raw, node_raw) => {
                if let (Some(tid), Some(nid)) = (
                    NonZeroU32::new(tree_raw).map(teleology_core::TreeId),
                    NonZeroU32::new(node_raw).map(teleology_core::NodeId),
                ) {
                    let world = self.engine.world_mut();
                    if let Some(mut trees) = world.get_resource_mut::<ProgressTrees>() {
                        trees.remove_node(tid, nid);
                    }
                }
                if self.progress_selected_node_raw == Some(node_raw) {
                    self.progress_selected_node_raw = None;
                }
                self.progress_graph_pos.remove(&(tree_raw, node_raw));
            }
            PendingContextAction::ClearNodePrereqs(tree_raw, node_raw) => {
                let world = self.engine.world_mut();
                if let Some(mut trees) = world.get_resource_mut::<ProgressTrees>() {
                    if let Some(t) = trees.trees.iter_mut().find(|t| t.id.raw() == tree_raw) {
                        if let Some(n) = t.nodes.iter_mut().find(|n| n.id.raw() == node_raw) {
                            n.prerequisites.clear();
                        }
                    }
                }
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
