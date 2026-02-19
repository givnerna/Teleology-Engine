//! Visual editor for the Teleology engine. Modes: Map Editor, World view, Settings.
//! Runs on Windows, Mac, Linux (native) and WebGL (browser).

use eframe::egui;
use std::collections::HashSet;
use std::num::NonZeroU32;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use teleology_core::{
    add_province_to_world, compute_adjacency, pull_next_event, queue_event,
    register_builtin_templates, ArmyComposition,
    ArmyRegistry, CharacterGenConfig, EventBus, EventPopupStyle, EventQueue, EventRegistry,
    EventTemplate, GameDate, GameTime, PopupAnchor,
    MapFile, MapKind, NationId, NationModifiers, NationStore, NationTags, ProgressState,
    ProgressTrees, ProvinceId, ProvinceModifiers, ProvinceStore, ProvinceTags, TagId, TagRegistry,
    TagTypeId, TickUnit, TimeConfig, Army, spawn_army, ActiveEvent, WorldBounds,
    UiCommand, UiCommandBuffer, UiPrefabRegistry, Viewport,
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

/// Collect hex tiles within `radius` rings of (cq, cr) that are in-bounds.
fn hex_brush_tiles(cq: u32, cr: u32, radius: u32, bounds: (u32, u32)) -> Vec<(u32, u32)> {
    let (w, h) = bounds;
    if radius == 0 {
        return vec![(cq, cr)];
    }
    let mut out = Vec::new();
    let ir = radius as i32;
    for dr in -ir..=ir {
        for dq in -ir..=ir {
            // Manhattan-ish distance check for hex (offset coords, approximate)
            if dq.abs() + dr.abs() > ir * 2 { continue; }
            let nq = cq as i32 + dq;
            let nr = cr as i32 + dr;
            if nq >= 0 && nr >= 0 && (nq as u32) < w && (nr as u32) < h {
                out.push((nq as u32, nr as u32));
            }
        }
    }
    out
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

/// Scan `resources/{subdir}` for files matching any of the given extensions.
fn scan_resource_dir(subdir: &str, extensions: &[&str]) -> Vec<std::path::PathBuf> {
    let dir = std::path::PathBuf::from("resources").join(subdir);
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut files: Vec<std::path::PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    files
}

/// File type category for resource browser icons.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FileKind {
    Folder,
    Image,
    Audio,
    Font,
    Script,
    Map,
    Json,
    Prefab,
    Other,
}

impl FileKind {
    fn from_path(path: &std::path::Path) -> Self {
        if path.is_dir() {
            return FileKind::Folder;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "png" | "jpg" | "jpeg" | "bmp" | "webp" | "gif" | "tga" => FileKind::Image,
            "mp3" | "ogg" | "wav" | "flac" | "aac" => FileKind::Audio,
            "ttf" | "otf" => FileKind::Font,
            "cpp" | "c" | "h" | "hpp" | "rs" | "lua" | "py" => FileKind::Script,
            "tmap" => FileKind::Map,
            "json" => {
                // Check if filename hints at prefab
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if stem.contains("prefab") {
                    FileKind::Prefab
                } else {
                    FileKind::Json
                }
            }
            _ => FileKind::Other,
        }
    }

    fn icon(self) -> &'static str {
        match self {
            FileKind::Folder => "\u{1F4C1}",  // 📁
            FileKind::Image => "\u{1F5BC}",    // 🖼
            FileKind::Audio => "\u{1F3B5}",    // 🎵
            FileKind::Font => "\u{1F524}",     // 🔤
            FileKind::Script => "\u{1F4DC}",   // 📜
            FileKind::Map => "\u{1F5FA}",      // 🗺
            FileKind::Json => "\u{2699}",      // ⚙
            FileKind::Prefab => "\u{1F9E9}",   // 🧩
            FileKind::Other => "\u{1F4C4}",    // 📄
        }
    }

    fn color(self) -> egui::Color32 {
        match self {
            FileKind::Folder => egui::Color32::from_rgb(220, 190, 100),
            FileKind::Image => egui::Color32::from_rgb(120, 200, 120),
            FileKind::Audio => egui::Color32::from_rgb(120, 160, 220),
            FileKind::Font => egui::Color32::from_rgb(200, 140, 200),
            FileKind::Script => egui::Color32::from_rgb(220, 180, 100),
            FileKind::Map => egui::Color32::from_rgb(100, 200, 180),
            FileKind::Json => egui::Color32::from_rgb(180, 180, 180),
            FileKind::Prefab => egui::Color32::from_rgb(180, 120, 220),
            FileKind::Other => egui::Color32::from_rgb(150, 150, 150),
        }
    }
}

/// One entry in the resource browser.
#[derive(Clone)]
struct ResourceEntry {
    path: std::path::PathBuf,
    name: String,
    kind: FileKind,
    size: u64,
}

/// Scan a directory for the resource browser (folders + all files).
fn scan_directory(dir: &std::path::Path) -> Vec<ResourceEntry> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut result: Vec<ResourceEntry> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_string_lossy().to_string();
            // Skip hidden files
            if name.starts_with('.') {
                return None;
            }
            let kind = FileKind::from_path(&path);
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            Some(ResourceEntry { path, name, kind, size })
        })
        .collect();
    // Folders first, then files, alphabetical within each group
    result.sort_by(|a, b| {
        let a_dir = a.kind == FileKind::Folder;
        let b_dir = b.kind == FileKind::Folder;
        b_dir.cmp(&a_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    result
}

/// Recursively collect all directories under `root` (including `root` itself).
fn collect_folders(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut result = vec![root.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.starts_with('.') {
                    result.extend(collect_folders(&path));
                }
            }
        }
    }
    result.sort();
    result
}

/// Recursively copy a directory and its contents to a new location.
fn copy_dir_recursive(src: &std::path::Path, dest: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            std::fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

/// Format file size for display.
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Load an image file into an egui texture.
fn load_image_texture(
    ctx: &egui::Context,
    path: &std::path::Path,
) -> Option<egui::TextureHandle> {
    let data = std::fs::read(path).ok()?;
    let img = image::load_from_memory(&data).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let pixels = img.into_raw();
    let color_image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
    Some(ctx.load_texture(
        path.display().to_string(),
        color_image,
        egui::TextureOptions::LINEAR,
    ))
}

/// Scan `resources/fonts/` and install every .ttf / .otf into egui.
/// Returns `(font_file_paths, family_names_loaded)`.
fn load_custom_fonts(ctx: &egui::Context) -> (Vec<std::path::PathBuf>, Vec<String>) {
    let files = scan_resource_dir("fonts", &["ttf", "otf"]);
    if files.is_empty() {
        return (files, Vec::new());
    }
    let mut fonts = egui::FontDefinitions::default();
    let mut families = Vec::new();
    for path in &files {
        let Ok(data) = std::fs::read(path) else { continue };
        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let family_name = stem.clone();
        fonts.font_data.insert(
            family_name.clone(),
            egui::FontData::from_owned(data).into(),
        );
        // Register as its own FontFamily so it can be selected by name.
        let family = egui::FontFamily::Name(family_name.clone().into());
        fonts
            .families
            .entry(family.clone())
            .or_default()
            .insert(0, family_name.clone());
        // Also prepend to Proportional so the font is usable everywhere.
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .push(family_name.clone());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push(family_name.clone());
        families.push(family_name);
    }
    ctx.set_fonts(fonts);
    (files, families)
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

    /// Render script-driven game UI from the UiCommandBuffer.
    fn render_game_ui(&mut self, ctx: &egui::Context) {
        let world = self.engine.world();
        let (commands, prev_clicked) = {
            let Some(buffer) = world.get_resource::<UiCommandBuffer>() else { return };
            (buffer.commands.clone(), buffer.clicked_buttons.clone())
        };

        if commands.is_empty() {
            return;
        }

        // State for walking the command stream
        let mut clicked = Vec::new();
        let mut pending_color: Option<egui::Color32> = None;
        let mut pending_font_size: Option<f32> = None;

        // We process commands in a flat walk. Windows use egui::Area + egui::Frame
        // for positioning. Layout groups use closures captured in a stack approach,
        // but since egui is immediate-mode we process sequentially.
        let mut i = 0;
        while i < commands.len() {
            match &commands[i] {
                UiCommand::BeginWindow { title, x, y, w, h } => {
                    // Collect commands until matching EndWindow
                    let start = i + 1;
                    let mut depth = 1u32;
                    let mut end = start;
                    while end < commands.len() && depth > 0 {
                        match &commands[end] {
                            UiCommand::BeginWindow { .. } => depth += 1,
                            UiCommand::EndWindow => depth -= 1,
                            _ => {}
                        }
                        if depth > 0 {
                            end += 1;
                        }
                    }
                    let inner_cmds = &commands[start..end];
                    let title = title.clone();
                    let (wx, wy, ww, wh) = (*x, *y, *w, *h);

                    egui::Window::new(&title)
                        .id(egui::Id::new(format!("game_ui_{}", title)))
                        .fixed_pos([wx, wy])
                        .fixed_size([ww, wh])
                        .title_bar(false)
                        .collapsible(false)
                        .resizable(false)
                        .frame(egui::Frame::none()
                            .fill(egui::Color32::from_black_alpha(180))
                            .inner_margin(6.0)
                            .rounding(4.0))
                        .show(ctx, |ui| {
                            Self::render_ui_commands(
                                ui,
                                inner_cmds,
                                &prev_clicked,
                                &mut clicked,
                                &mut pending_color,
                                &mut pending_font_size,
                            );
                        });

                    i = if end < commands.len() { end + 1 } else { end };
                }
                UiCommand::EndWindow => {
                    // Stray EndWindow outside a BeginWindow; skip
                    i += 1;
                }
                _ => {
                    // Top-level commands outside any window — skip (require a window)
                    i += 1;
                }
            }
        }

        // Write back results
        let world = self.engine.world_mut();
        if let Some(mut buf) = world.get_resource_mut::<UiCommandBuffer>() {
            buf.clicked_buttons = clicked;
            buf.commands.clear();
        }
    }

    /// Render a slice of UI commands into an egui Ui.
    fn render_ui_commands(
        ui: &mut egui::Ui,
        commands: &[UiCommand],
        prev_clicked: &[u32],
        clicked: &mut Vec<u32>,
        pending_color: &mut Option<egui::Color32>,
        pending_font_size: &mut Option<f32>,
    ) {
        let mut i = 0;
        while i < commands.len() {
            match &commands[i] {
                UiCommand::BeginHorizontal => {
                    // Collect until EndHorizontal
                    let start = i + 1;
                    let mut depth = 1u32;
                    let mut end = start;
                    while end < commands.len() && depth > 0 {
                        match &commands[end] {
                            UiCommand::BeginHorizontal => depth += 1,
                            UiCommand::EndHorizontal => depth -= 1,
                            _ => {}
                        }
                        if depth > 0 { end += 1; }
                    }
                    let inner = &commands[start..end];
                    ui.horizontal(|ui| {
                        Self::render_ui_commands(ui, inner, prev_clicked, clicked, pending_color, pending_font_size);
                    });
                    i = if end < commands.len() { end + 1 } else { end };
                }
                UiCommand::EndHorizontal => { i += 1; }

                UiCommand::BeginVertical => {
                    let start = i + 1;
                    let mut depth = 1u32;
                    let mut end = start;
                    while end < commands.len() && depth > 0 {
                        match &commands[end] {
                            UiCommand::BeginVertical => depth += 1,
                            UiCommand::EndVertical => depth -= 1,
                            _ => {}
                        }
                        if depth > 0 { end += 1; }
                    }
                    let inner = &commands[start..end];
                    ui.vertical(|ui| {
                        Self::render_ui_commands(ui, inner, prev_clicked, clicked, pending_color, pending_font_size);
                    });
                    i = if end < commands.len() { end + 1 } else { end };
                }
                UiCommand::EndVertical => { i += 1; }

                UiCommand::Label { text, font_size } => {
                    let size = if *font_size > 0.0 {
                        *font_size
                    } else {
                        pending_font_size.take().unwrap_or(14.0)
                    };
                    let mut rt = egui::RichText::new(text).size(size);
                    if let Some(color) = pending_color.take() {
                        rt = rt.color(color);
                    }
                    ui.label(rt);
                    i += 1;
                }

                UiCommand::Button { id, text } => {
                    let mut rt = egui::RichText::new(text);
                    if let Some(color) = pending_color.take() {
                        rt = rt.color(color);
                    }
                    if let Some(size) = pending_font_size.take() {
                        rt = rt.size(size);
                    }
                    if ui.button(rt).clicked() {
                        clicked.push(*id);
                    }
                    i += 1;
                }

                UiCommand::ProgressBar { fraction, text, w } => {
                    pending_color.take();
                    pending_font_size.take();
                    let bar = egui::ProgressBar::new(*fraction)
                        .text(text.as_str())
                        .desired_width(*w);
                    ui.add(bar);
                    i += 1;
                }

                UiCommand::Image { path: _path, w, h } => {
                    pending_color.take();
                    pending_font_size.take();
                    // Placeholder: show a colored rect where the image would go
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(*w, *h),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(rect, 0.0, egui::Color32::from_gray(60));
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "[img]",
                        egui::FontId::proportional(10.0),
                        egui::Color32::from_gray(140),
                    );
                    i += 1;
                }

                UiCommand::Separator => {
                    pending_color.take();
                    pending_font_size.take();
                    ui.separator();
                    i += 1;
                }

                UiCommand::Spacing { amount } => {
                    pending_color.take();
                    pending_font_size.take();
                    ui.add_space(*amount);
                    i += 1;
                }

                UiCommand::SetColor { r, g, b, a } => {
                    *pending_color = Some(egui::Color32::from_rgba_unmultiplied(*r, *g, *b, *a));
                    i += 1;
                }

                UiCommand::SetFontSize { size } => {
                    *pending_font_size = Some(*size);
                    i += 1;
                }

                // Nested windows inside a window are skipped (not supported)
                UiCommand::BeginWindow { .. } => {
                    let mut depth = 1u32;
                    i += 1;
                    while i < commands.len() && depth > 0 {
                        match &commands[i] {
                            UiCommand::BeginWindow { .. } => depth += 1,
                            UiCommand::EndWindow => depth -= 1,
                            _ => {}
                        }
                        i += 1;
                    }
                }
                UiCommand::EndWindow => { i += 1; }
            }
        }
    }

    /// Unity-style resource browser: folder tree on left, file grid on right.
    fn ui_project_browser(&mut self, ctx: &egui::Context) {
        // Initial scan
        if !self.project_scanned {
            self.project_entries = scan_directory(&self.project_dir);
            self.project_folders = collect_folders(&std::path::PathBuf::from("resources"));
            self.project_scanned = true;
        }

        // --- OS file drag-and-drop: copy dropped files into current project directory ---
        let dropped: Vec<egui::DroppedFile> = ctx.input(|i| i.raw.dropped_files.clone());
        if !dropped.is_empty() {
            let mut copied = 0u32;
            for file in &dropped {
                if let Some(src_path) = &file.path {
                    if let Some(file_name) = src_path.file_name() {
                        let dest = self.project_dir.join(file_name);
                        if src_path.is_dir() {
                            // Recursively copy directory
                            if copy_dir_recursive(src_path, &dest).is_ok() {
                                copied += 1;
                            }
                        } else if std::fs::copy(src_path, &dest).is_ok() {
                            copied += 1;
                        }
                    }
                } else if let Some(bytes) = &file.bytes {
                    // Dropped from browser or clipboard — has bytes but no path
                    let name = if file.name.is_empty() { "dropped_file" } else { &file.name };
                    let dest = self.project_dir.join(name);
                    if std::fs::write(&dest, bytes.as_ref()).is_ok() {
                        copied += 1;
                    }
                }
            }
            if copied > 0 {
                self.project_entries = scan_directory(&self.project_dir);
                self.project_folders = collect_folders(&std::path::PathBuf::from("resources"));
                self.project_selected = None;
                self.project_drop_status = format!("Copied {} file{}", copied, if copied == 1 { "" } else { "s" });
                self.project_drop_status_ttl = 180; // ~3 seconds at 60fps
            }
        }

        // Tick down status message
        if self.project_drop_status_ttl > 0 {
            self.project_drop_status_ttl -= 1;
            if self.project_drop_status_ttl == 0 {
                self.project_drop_status.clear();
            }
        }

        // Check if OS files are hovering over the window (for visual feedback)
        let os_hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());

        egui::TopBottomPanel::bottom("project_browser")
            .default_height(200.0)
            .resizable(true)
            .frame(panel_frame())
            .show(ctx, |ui| {
                // --- Drop overlay: visual feedback when dragging files from OS ---
                if os_hovering {
                    let panel_rect = ui.max_rect();
                    ui.painter().rect_filled(
                        panel_rect,
                        4.0,
                        egui::Color32::from_rgba_unmultiplied(60, 120, 220, 40),
                    );
                    ui.painter().rect_stroke(
                        panel_rect.shrink(2.0),
                        4.0,
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(100, 160, 255)),
                    );
                    ui.painter().text(
                        panel_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Drop files here to import",
                        egui::FontId::proportional(18.0),
                        egui::Color32::from_rgb(180, 210, 255),
                    );
                }

                // --- Top bar: breadcrumb + controls ---
                ui.horizontal(|ui| {
                    ui.style_mut().spacing.item_spacing.x = 4.0;

                    // Breadcrumb navigation
                    let parts: Vec<String> = self.project_dir
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().to_string())
                        .collect();
                    for (i, part) in parts.iter().enumerate() {
                        if i > 0 {
                            ui.label(
                                egui::RichText::new("/")
                                    .small()
                                    .color(ui.visuals().weak_text_color()),
                            );
                        }
                        let is_last = i == parts.len() - 1;
                        if is_last {
                            ui.strong(part);
                        } else if ui.small_button(part).clicked() {
                            let new_dir: std::path::PathBuf = parts[..=i].iter().collect();
                            self.project_dir = new_dir;
                            self.project_entries = scan_directory(&self.project_dir);
                            self.project_selected = None;
                        }
                    }

                    ui.separator();

                    // Up button
                    if ui.small_button("\u{2B06} Up").on_hover_text("Go to parent folder").clicked() {
                        if let Some(parent) = self.project_dir.parent() {
                            // Don't go above project root
                            if parent.components().count() > 0 {
                                self.project_dir = parent.to_path_buf();
                                self.project_entries = scan_directory(&self.project_dir);
                                self.project_selected = None;
                            }
                        }
                    }

                    if ui.small_button("\u{21BB} Refresh").on_hover_text("Re-scan directory").clicked() {
                        self.project_entries = scan_directory(&self.project_dir);
                        self.project_folders = collect_folders(&std::path::PathBuf::from("resources"));
                        self.project_selected = None;
                    }

                    // Drop status message (fades after a few seconds)
                    if !self.project_drop_status.is_empty() {
                        let alpha = ((self.project_drop_status_ttl as f32 / 60.0).min(1.0) * 255.0) as u8;
                        ui.label(
                            egui::RichText::new(&self.project_drop_status)
                                .small()
                                .color(egui::Color32::from_rgba_unmultiplied(100, 220, 100, alpha)),
                        );
                    }

                    ui.separator();

                    // Tile size slider
                    ui.label(
                        egui::RichText::new("Size:")
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    );
                    ui.add(egui::Slider::new(&mut self.project_tile_size, 48.0..=128.0).show_value(false));

                    ui.separator();

                    // Script loader (moved from old bottom panel)
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        ui.label(
                            egui::RichText::new("Script:")
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        );
                        ui.add(egui::TextEdit::singleline(&mut self.script_path_input).desired_width(200.0));
                        if ui.small_button("Load").clicked() {
                            let path = PathBuf::from(self.script_path_input.trim());
                            if path.exists() {
                                let _ = self.engine.load_script(&path);
                            }
                        }
                        ui.checkbox(&mut self.hot_reload, "Hot reload");
                        self.engine.set_hot_reload(self.hot_reload);
                    }
                    #[cfg(target_arch = "wasm32")]
                    ui.label("WebGL");
                });

                ui.add_space(2.0);
                ui.separator();

                // --- Body: folder tree (left) + file grid (right) ---
                ui.horizontal_top(|ui| {
                    // Folder tree (left panel, fixed width)
                    let tree_width = 160.0;
                    ui.allocate_ui_with_layout(
                        egui::vec2(tree_width, ui.available_height()),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("project_tree")
                                .show(ui, |ui| {
                                    for folder in &self.project_folders.clone() {
                                        let depth = folder
                                            .strip_prefix("resources")
                                            .map(|r| r.components().count())
                                            .unwrap_or(0);
                                        let indent = depth as f32 * 14.0;
                                        let display_name = folder
                                            .file_name()
                                            .unwrap_or(folder.as_os_str())
                                            .to_string_lossy()
                                            .to_string();
                                        let is_selected = *folder == self.project_dir;

                                        ui.horizontal(|ui| {
                                            ui.add_space(indent);
                                            let label = if is_selected {
                                                egui::RichText::new(format!("\u{1F4C2} {}", display_name))
                                                    .strong()
                                                    .color(egui::Color32::from_rgb(220, 190, 100))
                                            } else {
                                                egui::RichText::new(format!("\u{1F4C1} {}", display_name))
                                                    .color(egui::Color32::from_rgb(200, 180, 120))
                                            };
                                            if ui.selectable_label(is_selected, label).clicked() {
                                                self.project_dir = folder.clone();
                                                self.project_entries = scan_directory(&self.project_dir);
                                                self.project_selected = None;
                                            }
                                        });
                                    }
                                });
                        },
                    );

                    ui.separator();

                    // File grid (right, fills remaining space)
                    egui::ScrollArea::both()
                        .id_salt("project_grid")
                        .show(ui, |ui| {
                            if self.project_entries.is_empty() {
                                ui.label(
                                    egui::RichText::new("Empty folder")
                                        .italics()
                                        .color(ui.visuals().weak_text_color()),
                                );
                                return;
                            }

                            let tile = self.project_tile_size;
                            let available_width = ui.available_width();
                            let cols = ((available_width / (tile + 8.0)).floor() as usize).max(1);
                            let entries = self.project_entries.clone();

                            let mut clicked_idx: Option<usize> = None;
                            let mut double_clicked_idx: Option<usize> = None;
                            let mut drop_move: Option<(usize, usize)> = None; // (src_idx, dest_folder_idx)

                            // Track pointer position for drop detection
                            let pointer_pos = ui.input(|i| i.pointer.hover_pos());

                            egui::Grid::new("resource_grid")
                                .spacing([4.0, 4.0])
                                .show(ui, |ui| {
                                    for (i, entry) in entries.iter().enumerate() {
                                        let is_selected = self.project_selected == Some(i);

                                        let (rect, response) = ui.allocate_exact_size(
                                            egui::vec2(tile, tile + 16.0),
                                            egui::Sense::click_and_drag(),
                                        );

                                        if response.clicked() {
                                            clicked_idx = Some(i);
                                        }
                                        if response.double_clicked() {
                                            double_clicked_idx = Some(i);
                                        }

                                        // Drag tracking: start drag on non-folder entries
                                        if response.drag_started() && entry.kind != FileKind::Folder {
                                            self.project_dragging = Some(i);
                                        }

                                        // Drop detection: if we're dragging and pointer is over a folder
                                        let is_drag_target = entry.kind == FileKind::Folder
                                            && self.project_dragging.is_some()
                                            && pointer_pos.map_or(false, |p| rect.contains(p));

                                        // Background
                                        let bg = if is_drag_target {
                                            // Highlight folder as drop target
                                            egui::Color32::from_rgba_unmultiplied(60, 180, 60, 80)
                                        } else if is_selected {
                                            egui::Color32::from_rgba_unmultiplied(80, 120, 200, 60)
                                        } else if response.hovered() {
                                            egui::Color32::from_rgba_unmultiplied(80, 80, 80, 40)
                                        } else {
                                            egui::Color32::TRANSPARENT
                                        };
                                        ui.painter().rect_filled(rect, 4.0, bg);

                                        // Draw drop target border
                                        if is_drag_target {
                                            ui.painter().rect_stroke(
                                                rect.shrink(1.0),
                                                4.0,
                                                egui::Stroke::new(2.0, egui::Color32::from_rgb(80, 220, 80)),
                                            );
                                        }

                                        // Icon area
                                        let icon_rect = egui::Rect::from_min_size(
                                            rect.min,
                                            egui::vec2(tile, tile - 4.0),
                                        );

                                        // Image thumbnail for image files
                                        let mut drew_thumb = false;
                                        if entry.kind == FileKind::Image {
                                            if !self.project_thumbnails.contains_key(&entry.path) {
                                                if let Some(tex) = load_image_texture(ctx, &entry.path) {
                                                    self.project_thumbnails.insert(entry.path.clone(), tex);
                                                }
                                            }
                                            if let Some(tex) = self.project_thumbnails.get(&entry.path) {
                                                let img_size = tex.size_vec2();
                                                let scale = ((tile - 8.0) / img_size.x)
                                                    .min((tile - 12.0) / img_size.y);
                                                let display = img_size * scale;
                                                let img_rect = egui::Rect::from_center_size(
                                                    icon_rect.center(),
                                                    display,
                                                );
                                                ui.painter().image(
                                                    tex.id(),
                                                    img_rect,
                                                    egui::Rect::from_min_max(
                                                        egui::pos2(0.0, 0.0),
                                                        egui::pos2(1.0, 1.0),
                                                    ),
                                                    egui::Color32::WHITE,
                                                );
                                                drew_thumb = true;
                                            }
                                        }

                                        if !drew_thumb {
                                            // Draw file type icon
                                            let icon_text = entry.kind.icon();
                                            let icon_size = (tile * 0.45).clamp(16.0, 48.0);
                                            ui.painter().text(
                                                icon_rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                icon_text,
                                                egui::FontId::proportional(icon_size),
                                                entry.kind.color(),
                                            );
                                        }

                                        // File name below icon
                                        let name_rect = egui::Rect::from_min_max(
                                            egui::pos2(rect.min.x, rect.max.y - 14.0),
                                            rect.max,
                                        );
                                        let truncated = if entry.name.len() > 12 {
                                            format!("{}…", &entry.name[..11])
                                        } else {
                                            entry.name.clone()
                                        };
                                        ui.painter().text(
                                            name_rect.center(),
                                            egui::Align2::CENTER_CENTER,
                                            &truncated,
                                            egui::FontId::proportional(10.0),
                                            if is_selected {
                                                egui::Color32::WHITE
                                            } else {
                                                egui::Color32::from_gray(200)
                                            },
                                        );

                                        // Tooltip (suppress during drag)
                                        if self.project_dragging.is_none() {
                                            response.on_hover_ui(|ui| {
                                                ui.strong(&entry.name);
                                                ui.label(entry.path.display().to_string());
                                                if entry.kind != FileKind::Folder {
                                                    ui.label(format_size(entry.size));
                                                }
                                            });
                                        }

                                        // Detect drop onto folder: pointer released while over this folder
                                        if is_drag_target {
                                            let released = ui.input(|i| i.pointer.any_released());
                                            if released {
                                                if let Some(src) = self.project_dragging {
                                                    drop_move = Some((src, i));
                                                }
                                            }
                                        }

                                        // End of row
                                        if (i + 1) % cols == 0 {
                                            ui.end_row();
                                        }
                                    }
                                });

                            // Draw drag cursor indicator
                            if let Some(drag_idx) = self.project_dragging {
                                if let Some(pos) = pointer_pos {
                                    if let Some(drag_entry) = entries.get(drag_idx) {
                                        ui.painter().text(
                                            pos + egui::vec2(12.0, -8.0),
                                            egui::Align2::LEFT_BOTTOM,
                                            &drag_entry.name,
                                            egui::FontId::proportional(11.0),
                                            egui::Color32::from_rgba_unmultiplied(200, 200, 255, 200),
                                        );
                                    }
                                }

                                // Clear drag state when pointer released
                                let released = ui.input(|i| i.pointer.any_released());
                                if released {
                                    self.project_dragging = None;
                                }
                            }

                            // Process internal drag-and-drop move: file → folder
                            if let Some((src_idx, dest_idx)) = drop_move {
                                if let (Some(src_entry), Some(dest_entry)) = (entries.get(src_idx), entries.get(dest_idx)) {
                                    if let Some(file_name) = src_entry.path.file_name() {
                                        let dest_path = dest_entry.path.join(file_name);
                                        if std::fs::rename(&src_entry.path, &dest_path).is_ok() {
                                            self.project_entries = scan_directory(&self.project_dir);
                                            self.project_folders = collect_folders(&std::path::PathBuf::from("resources"));
                                            self.project_selected = None;
                                            self.project_drop_status = format!("Moved \"{}\" to {}", src_entry.name, dest_entry.name);
                                            self.project_drop_status_ttl = 180;
                                        }
                                    }
                                }
                                self.project_dragging = None;
                            }

                            // Handle clicks
                            if let Some(idx) = clicked_idx {
                                self.project_selected = Some(idx);
                            }
                            if let Some(idx) = double_clicked_idx {
                                let entry = &entries[idx];
                                if entry.kind == FileKind::Folder {
                                    self.project_dir = entry.path.clone();
                                    self.project_entries = scan_directory(&self.project_dir);
                                    self.project_selected = None;
                                } else if entry.kind == FileKind::Script {
                                    // Double-click script → load it
                                    #[cfg(not(target_arch = "wasm32"))]
                                    {
                                        // If it looks like a compiled library, load as script
                                        let ext = entry.path.extension()
                                            .and_then(|e| e.to_str())
                                            .unwrap_or("");
                                        if ext == "so" || ext == "dll" || ext == "dylib" {
                                            let _ = self.engine.load_script(&entry.path);
                                        } else {
                                            self.script_path_input = entry.path.display().to_string();
                                        }
                                    }
                                } else if entry.kind == FileKind::Audio {
                                    // Double-click audio → play it
                                    let _ = self.engine.audio_play_file(&entry.path, false, 0.8);
                                }
                            }
                        });
                });
            });
    }

    fn ui_media(&mut self, ctx: &egui::Context) {
        // --- Left panel: file browser for audio + images ---
        egui::SidePanel::left("media_browser")
            .default_width(280.0)
            .resizable(true)
            .frame(panel_frame())
            .show(ctx, |ui| {
                panel_header(ui, "Browser");
                ui.add_space(4.0);

                if ui.button("Refresh").on_hover_text("Re-scan resources/ folders (audio, assets, fonts)").clicked() {
                    self.audio_files = scan_resource_dir("audio", &["mp3", "ogg", "wav", "flac", "aac"]);
                    self.image_files = scan_resource_dir("assets", &["png", "jpg", "jpeg", "bmp", "webp"]);
                    let (ff, fam) = load_custom_fonts(ctx);
                    self.font_files = ff;
                    self.loaded_font_families = fam;
                    self.media_selected_font = None;
                }
                ui.add_space(6.0);

                // ---- Audio files ----
                ui.strong("Audio");
                ui.label(
                    egui::RichText::new("resources/audio/")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
                ui.add_space(4.0);
                if self.audio_files.is_empty() {
                    ui.label(
                        egui::RichText::new("No audio files. Place .mp3/.ogg/.wav/.flac files in resources/audio/")
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("audio_list")
                        .max_height(200.0)
                        .show(ui, |ui| {
                            for (i, path) in self.audio_files.iter().enumerate() {
                                let name = path.file_name().unwrap_or_default().to_string_lossy();
                                let selected = self.media_selected_audio == Some(i);
                                if ui.selectable_label(selected, name.as_ref()).clicked() {
                                    self.media_selected_audio = Some(i);
                                    self.audio_path_input = path.display().to_string();
                                }
                            }
                        });
                }

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);

                // ---- Image files ----
                ui.strong("Images");
                ui.label(
                    egui::RichText::new("resources/assets/")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
                ui.add_space(4.0);
                if self.image_files.is_empty() {
                    ui.label(
                        egui::RichText::new("No image files. Place .png/.jpg/.bmp/.webp files in resources/assets/")
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("image_list")
                        .max_height(200.0)
                        .show(ui, |ui| {
                            for (i, path) in self.image_files.iter().enumerate() {
                                let name = path.file_name().unwrap_or_default().to_string_lossy();
                                let selected = self.media_selected_image == Some(i);
                                if ui.selectable_label(selected, name.as_ref()).clicked() {
                                    self.media_selected_image = Some(i);
                                }
                            }
                        });
                }

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);

                // ---- Font files ----
                ui.strong("Fonts");
                ui.label(
                    egui::RichText::new("resources/fonts/")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
                ui.add_space(4.0);
                if self.font_files.is_empty() {
                    ui.label(
                        egui::RichText::new("No font files. Place .ttf/.otf files in resources/fonts/")
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("font_list")
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            for (i, path) in self.font_files.iter().enumerate() {
                                let name = path.file_name().unwrap_or_default().to_string_lossy();
                                let selected = self.media_selected_font == Some(i);
                                if ui.selectable_label(selected, name.as_ref()).clicked() {
                                    self.media_selected_font = Some(i);
                                }
                            }
                        });
                }
            });

        // --- Center panel: preview / controls ---
        egui::CentralPanel::default()
            .frame(panel_frame())
            .show(ctx, |ui| {
                panel_header(ui, "Preview");
                ui.add_space(8.0);

                // ---- Audio controls ----
                ui.group(|ui| {
                    ui.strong("Audio");
                    ui.add_space(4.0);

                    if let Some(idx) = self.media_selected_audio {
                        if let Some(path) = self.audio_files.get(idx) {
                            let name = path.file_name().unwrap_or_default().to_string_lossy();
                            ui.label(format!("Selected: {}", name));
                            ui.label(
                                egui::RichText::new(path.display().to_string())
                                    .small()
                                    .color(ui.visuals().weak_text_color()),
                            );
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if ui.button("Play").clicked() {
                                    let _ = self.engine.audio_play_file(path, false, 0.8);
                                }
                                if ui.button("Loop").clicked() {
                                    let _ = self.engine.audio_play_file(path, true, 0.6);
                                }
                                ui.separator();
                                if ui.button("Vol 100%").clicked() {
                                    self.engine.audio_set_master_volume(1.0);
                                }
                                if ui.button("Vol 50%").clicked() {
                                    self.engine.audio_set_master_volume(0.5);
                                }
                            });
                        }
                    } else {
                        ui.label(
                            egui::RichText::new("Select an audio file from the browser.")
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        );
                    }

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Custom path:");
                        ui.text_edit_singleline(&mut self.audio_path_input);
                        #[cfg(not(target_arch = "wasm32"))]
                        if ui.small_button("Pick").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_file() {
                                self.audio_path_input = path.display().to_string();
                            }
                        }
                        if ui.small_button("Play").clicked() {
                            let p = std::path::PathBuf::from(self.audio_path_input.trim());
                            let _ = self.engine.audio_play_file(&p, false, 0.8);
                        }
                    });
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(12.0);

                // ---- Image preview ----
                ui.group(|ui| {
                    ui.strong("Image Preview");
                    ui.add_space(4.0);

                    if let Some(idx) = self.media_selected_image {
                        if let Some(path) = self.image_files.get(idx).cloned() {
                            let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                            ui.label(format!("Selected: {}", name));
                            ui.label(
                                egui::RichText::new(path.display().to_string())
                                    .small()
                                    .color(ui.visuals().weak_text_color()),
                            );
                            ui.add_space(6.0);

                            // Load texture on demand, cache it
                            if !self.media_textures.contains_key(&path) {
                                if let Some(tex) = load_image_texture(ctx, &path) {
                                    self.media_textures.insert(path.clone(), tex);
                                }
                            }
                            if let Some(tex) = self.media_textures.get(&path) {
                                let img_size = tex.size_vec2();
                                let available = ui.available_size();
                                let scale = (available.x / img_size.x)
                                    .min(available.y / img_size.y)
                                    .min(1.0);
                                let display_size = img_size * scale;
                                ui.image(egui::load::SizedTexture::new(tex.id(), display_size));
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{}x{} px",
                                        img_size.x as u32,
                                        img_size.y as u32
                                    ))
                                    .small()
                                    .color(ui.visuals().weak_text_color()),
                                );
                            } else {
                                ui.label(
                                    egui::RichText::new("Failed to load image.")
                                        .color(ui.visuals().error_fg_color),
                                );
                            }
                        }
                    } else {
                        ui.label(
                            egui::RichText::new("Select an image file from the browser.")
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        );
                    }
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(12.0);

                // ---- Font preview ----
                ui.group(|ui| {
                    ui.strong("Font Preview");
                    ui.add_space(4.0);

                    if let Some(idx) = self.media_selected_font {
                        if let Some(path) = self.font_files.get(idx) {
                            let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                            let family_name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                            ui.label(format!("Selected: {}", file_name));
                            ui.label(
                                egui::RichText::new(format!("Family: \"{}\"", family_name))
                                    .small()
                                    .color(ui.visuals().weak_text_color()),
                            );
                            ui.add_space(6.0);

                            ui.label("Preview text:");
                            ui.text_edit_multiline(&mut self.font_preview_text);
                            ui.add_space(6.0);

                            let family = egui::FontFamily::Name(family_name.into());
                            for &size in &[14.0_f32, 20.0, 28.0, 40.0] {
                                ui.label(
                                    egui::RichText::new(&self.font_preview_text)
                                        .font(egui::FontId::new(size, family.clone()))
                                        .color(egui::Color32::WHITE),
                                );
                                ui.add_space(4.0);
                            }
                        }
                    } else if self.loaded_font_families.is_empty() {
                        ui.label(
                            egui::RichText::new("No fonts loaded. Place .ttf/.otf files in resources/fonts/ and click Refresh.")
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("Select a font from the browser.")
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        );
                    }
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
                            image: String::new(),
                            image_w: 0.0,
                            image_h: 0.0,
                        });
                        self.event_selected_raw = Some(id.raw());
                    }
                    self.pending_create_event = false;
                }

                // --- Template gallery ---
                ui.add_space(4.0);
                ui.collapsing("Templates", |ui| {
                    ui.label(
                        egui::RichText::new("Create from template (customize after):")
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    );
                    ui.add_space(2.0);
                    let templates: &[(&str, EventTemplate)] = &[
                        ("Notification", EventTemplate::Notification),
                        ("Binary Choice", EventTemplate::BinaryChoice),
                        ("Three-Way", EventTemplate::ThreeWayChoice),
                        ("Narrative", EventTemplate::Narrative),
                        ("Diplomatic", EventTemplate::DiplomaticProposal),
                    ];
                    for (label, tmpl) in templates {
                        if ui.button(*label).clicked() {
                            if world.get_resource::<EventRegistry>().is_none() {
                                world.insert_resource(EventRegistry::new());
                                world.insert_resource(EventQueue::default());
                                world.insert_resource(ActiveEvent::default());
                                world.insert_resource(EventPopupStyle::default());
                            }
                            if let Some(mut reg) = world.get_resource_mut::<EventRegistry>() {
                                let id = reg.insert(tmpl.create());
                                self.event_selected_raw = Some(id.raw());
                            }
                        }
                    }
                    ui.add_space(2.0);
                    if ui.button("Register all templates").on_hover_text("Create all 5 templates at once").clicked() {
                        if world.get_resource::<EventRegistry>().is_none() {
                            world.insert_resource(EventRegistry::new());
                            world.insert_resource(EventQueue::default());
                            world.insert_resource(ActiveEvent::default());
                            world.insert_resource(EventPopupStyle::default());
                        }
                        if let Some(mut reg) = world.get_resource_mut::<EventRegistry>() {
                            let ids = register_builtin_templates(&mut reg);
                            self.event_selected_raw = Some(ids[0].raw());
                        }
                    }
                });

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

            // Expand visible rect so partially-offscreen nodes/edges still render
            let vis_rect = rect.expand(node_size.x.max(node_size.y));

            // Draw connections first (cull if both endpoints offscreen)
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
                    // Skip edge if both endpoints are outside the visible area
                    if !vis_rect.contains(from) && !vis_rect.contains(to) { continue; }
                    painter.line_segment(
                        [from, to],
                        egui::Stroke::new(2.0, ui.visuals().widgets.active.bg_fill),
                    );
                }
            }

            // Draw nodes + interactions (cull if entirely offscreen)
            for raw in ids {
                let Some(def) = reg_snapshot.events.get(&raw) else { continue };
                let pos = *self.event_graph_pos.get(&raw).unwrap();
                let top_left = rect.min + (pos.to_vec2() + self.event_graph_pan);
                let node_rect = egui::Rect::from_min_size(top_left, node_size);

                // Skip if node is entirely outside visible area
                if !vis_rect.intersects(node_rect) { continue; }

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

            // Expand visible rect slightly so edges/nodes partially offscreen still render
            let vis_rect = rect.expand(node_size.x.max(node_size.y));

            // Draw edges: prereq -> node (cull if both endpoints offscreen)
            for n in &tree.nodes {
                let to_key = (tree_raw, n.id.raw());
                let Some(to_pos) = self.progress_graph_pos.get(&to_key).copied() else { continue };
                for prereq in &n.prerequisites {
                    let from_key = (tree_raw, prereq.raw());
                    let Some(from_pos) = self.progress_graph_pos.get(&from_key).copied() else { continue };
                    let from = rect.min + (from_pos.to_vec2() + self.progress_graph_pan) + egui::Vec2::new(node_size.x, node_size.y * 0.5);
                    let to = rect.min + (to_pos.to_vec2() + self.progress_graph_pan) + egui::Vec2::new(0.0, node_size.y * 0.5);
                    // Skip edge if both endpoints are outside the visible area
                    if !vis_rect.contains(from) && !vis_rect.contains(to) { continue; }
                    painter.line_segment([from, to], egui::Stroke::new(2.0, ui.visuals().widgets.active.bg_fill));
                }
            }

            // Nodes (cull if entirely offscreen)
            for n in &tree.nodes {
                let raw = n.id.raw();
                let key = (tree_raw, raw);
                let pos = *self.progress_graph_pos.get(&key).unwrap();
                let top_left = rect.min + (pos.to_vec2() + self.progress_graph_pan);
                let node_rect = egui::Rect::from_min_size(top_left, node_size);

                // Skip rendering if node is entirely outside visible area
                if !vis_rect.intersects(node_rect) { continue; }

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
        let show_names = self.show_map_names;
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
                let time = self.engine.world().get_resource::<GameTime>().copied();
                let tick_unit = self.engine.world().get_resource::<TimeConfig>()
                    .map(|c| c.tick_unit).unwrap_or(TickUnit::Day);
                let needs_time = matches!(tick_unit, TickUnit::Second | TickUnit::Minute | TickUnit::Hour);
                ui.horizontal(|ui| {
                    ui.strong("Date:");
                    ui.add_space(4.0);
                    if needs_time {
                        if let Some(t) = time {
                            ui.label(format!("{}-{:02}-{:02} {:02}:{:02}:{:02}", date.year, date.month, date.day, t.hour, t.minute, t.second));
                        } else {
                            ui.label(format!("{}-{:02}-{:02}", date.year, date.month, date.day));
                        }
                    } else {
                        ui.label(format!("{}-{:02}-{:02}", date.year, date.month, date.day));
                    }
                });
                ui.add_space(4.0);

                // --- Toolbar row: paint mode + brush size + zoom + shortcuts hint ---
                ui.horizontal(|ui| {
                    ui.label("Mode:");
                    ui.radio_value(
                        &mut self.map_paint_mode,
                        MapEditorPaintMode::PaintOwnership,
                        "Paint ownership",
                    );
                    ui.radio_value(
                        &mut self.map_paint_mode,
                        MapEditorPaintMode::EditProvinces,
                        "Edit provinces",
                    );
                    ui.separator();
                    ui.label("Brush:");
                    let brush_label = match self.brush_radius {
                        0 => "1x1",
                        1 => "3x3",
                        2 => "5x5",
                        _ => "7x7",
                    };
                    if ui.button(brush_label).on_hover_text("[ / ] to change").clicked() {
                        self.brush_radius = (self.brush_radius + 1) % 4;
                    }
                    ui.separator();
                    ui.label(format!("Zoom: {:.0}%", self.map_zoom * 100.0));
                    if ui.small_button("Reset").on_hover_text("Reset zoom & pan").clicked() {
                        self.map_zoom = 1.0;
                        self.map_pan = egui::Vec2::ZERO;
                    }
                    ui.separator();
                    ui.checkbox(&mut self.show_map_names, "Names")
                        .on_hover_text("Show province/nation labels on the map (N)");
                });
                ui.add_space(2.0);

                // --- Help text ---
                let hint = if paint_ownership {
                    "LMB: paint nation | RMB: erase (set empty) | Scroll: zoom | Middle-drag: pan | Tab: switch mode | [ ] brush size"
                } else {
                    "LMB: paint province | RMB: erase (set empty) | Scroll: zoom | Middle-drag: pan | Tab: switch mode | [ ] brush size"
                };
                ui.label(
                    egui::RichText::new(hint)
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
                ui.add_space(4.0);

                let world = self.engine.world();
                let map_kind = world.get_resource::<MapKind>();
                let store = world.get_resource::<ProvinceStore>();

                // --- Tile hit result: coordinates + erase flag ---
                #[derive(Clone, Copy)]
                enum TileHit {
                    Square(u32, u32),
                    Hex(u32, u32),
                }

                // --- Hover info for tooltip ---
                struct HoverInfo {
                    coords: String,
                    province_raw: u32,
                    owner_raw: u32,
                    dev: [u16; 3],
                    terrain: u8,
                }

                let zoom = self.map_zoom;

                let (_map_bounds, tile_hit, erase_hit, hover_info) = {
                    if let (Some(mk), Some(st)) = (map_kind, store) {
                        match mk {
                            MapKind::Square(map) => {
                                let base_cell = 14.0_f32;
                                let cell_size = base_cell * zoom;
                                let canvas_w = map.width as f32 * cell_size;
                                let canvas_h = map.height as f32 * cell_size;
                                let (response, painter) = ui.allocate_painter(
                                    ui.available_size().max(egui::Vec2::new(canvas_w, canvas_h)),
                                    egui::Sense::click_and_drag(),
                                );
                                let rect = response.rect;

                                // --- Zoom (scroll wheel) ---
                                let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
                                if response.hovered() && scroll.abs() > 0.1 {
                                    let factor = 1.0 + scroll * 0.002;
                                    let new_zoom = (self.map_zoom * factor).clamp(0.25, 6.0);
                                    // Zoom toward cursor
                                    if let Some(cursor) = response.hover_pos() {
                                        let rel = cursor - rect.min - self.map_pan;
                                        self.map_pan += rel * (1.0 - new_zoom / self.map_zoom);
                                    }
                                    self.map_zoom = new_zoom;
                                }
                                // --- Pan (middle-drag) ---
                                if response.dragged_by(egui::PointerButton::Middle) {
                                    self.map_pan += response.drag_delta();
                                }

                                let cell_size = base_cell * self.map_zoom;
                                let origin = rect.min + self.map_pan;

                                // --- Render tiles ---
                                // Visible range culling
                                let vis_x0 = (((rect.min.x - origin.x) / cell_size).floor() as i32).max(0) as u32;
                                let vis_y0 = (((rect.min.y - origin.y) / cell_size).floor() as i32).max(0) as u32;
                                let vis_x1 = (((rect.max.x - origin.x) / cell_size).ceil() as i32 + 1).max(0) as u32;
                                let vis_y1 = (((rect.max.y - origin.y) / cell_size).ceil() as i32 + 1).max(0) as u32;

                                for y in vis_y0..vis_y1.min(map.height) {
                                    for x in vis_x0..vis_x1.min(map.width) {
                                        let raw = map.get(x, y);
                                        let (owner_raw, is_selected_prov, is_selected_nation) = if raw == 0 {
                                            (0u32, false, false)
                                        } else {
                                            let owner = st
                                                .get(ProvinceId(NonZeroU32::new(raw).unwrap()))
                                                .and_then(|p| p.owner)
                                                .map(|n| n.0.get())
                                                .unwrap_or(0);
                                            let sp = self.selected_province == Some(raw);
                                            let sn = owner != 0 && self.selected_nation == Some(owner);
                                            (owner, sp, sn)
                                        };
                                        let base_color = if raw == 0 {
                                            TILE_EMPTY_COLOR
                                        } else if owner_raw == 0 {
                                            TILE_UNOWNED_PROVINCE_COLOR
                                        } else {
                                            nation_color(owner_raw)
                                        };
                                        let xf = origin.x + x as f32 * cell_size;
                                        let yf = origin.y + y as f32 * cell_size;
                                        let tile_rect = egui::Rect::from_min_size(
                                            egui::Pos2::new(xf, yf),
                                            egui::Vec2::new(cell_size - 1.0, cell_size - 1.0),
                                        );
                                        painter.rect_filled(tile_rect, 0.0, base_color);
                                        // --- Highlight selected province/nation ---
                                        if is_selected_prov {
                                            painter.rect_stroke(
                                                tile_rect,
                                                0.0,
                                                egui::Stroke::new(2.0, egui::Color32::from_rgb(0xFF, 0xFF, 0x00)),
                                            );
                                        } else if is_selected_nation {
                                            painter.rect_stroke(
                                                tile_rect,
                                                0.0,
                                                egui::Stroke::new(1.0, egui::Color32::from_rgba_premultiplied(0xFF, 0xFF, 0xFF, 0x60)),
                                            );
                                        }
                                    }
                                }
                                // Province borders
                                for y in vis_y0..vis_y1.min(map.height) {
                                    for x in vis_x0..vis_x1.min(map.width) {
                                        let raw = map.get(x, y);
                                        let xf = origin.x + x as f32 * cell_size;
                                        let yf = origin.y + y as f32 * cell_size;
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
                                // --- Province / nation name labels ---
                                if show_names {
                                    let mut centroids: std::collections::HashMap<u32, (f32, f32, u32, u32)> =
                                        std::collections::HashMap::new();
                                    for y in vis_y0..vis_y1.min(map.height) {
                                        for x in vis_x0..vis_x1.min(map.width) {
                                            let raw = map.get(x, y);
                                            if raw == 0 { continue; }
                                            let owner_raw = st
                                                .get(ProvinceId(NonZeroU32::new(raw).unwrap()))
                                                .and_then(|p| p.owner)
                                                .map(|n| n.0.get())
                                                .unwrap_or(0);
                                            let e = centroids.entry(raw).or_insert((0.0, 0.0, 0, owner_raw));
                                            e.0 += origin.x + (x as f32 + 0.5) * cell_size;
                                            e.1 += origin.y + (y as f32 + 0.5) * cell_size;
                                            e.2 += 1;
                                        }
                                    }
                                    let font_size = (cell_size * 0.75).clamp(7.0, 18.0);
                                    let font_id = egui::FontId::proportional(font_size);
                                    for (prov_raw, (sum_x, sum_y, count, owner_raw)) in &centroids {
                                        let cx = sum_x / *count as f32;
                                        let cy = sum_y / *count as f32;
                                        let label = if paint_ownership && *owner_raw != 0 {
                                            format!("N{}", owner_raw)
                                        } else {
                                            format!("P{}", prov_raw)
                                        };
                                        let pos = egui::Pos2::new(cx, cy);
                                        let galley = painter.layout_no_wrap(label, font_id.clone(), egui::Color32::WHITE);
                                        let text_rect = egui::Rect::from_center_size(pos, galley.size());
                                        painter.rect_filled(
                                            text_rect.expand(2.0),
                                            2.0,
                                            egui::Color32::from_rgba_premultiplied(0, 0, 0, 160),
                                        );
                                        painter.galley(text_rect.min, galley, egui::Color32::WHITE);
                                    }
                                }
                                // --- Cursor → tile mapping (zoom/pan aware) ---
                                let to_tile = |pos: egui::Pos2| -> Option<(u32, u32)> {
                                    let lx = (pos.x - origin.x) / cell_size;
                                    let ly = (pos.y - origin.y) / cell_size;
                                    if lx < 0.0 || ly < 0.0 { return None; }
                                    let rx = lx as u32;
                                    let ry = ly as u32;
                                    (rx < map.width && ry < map.height).then_some((rx, ry))
                                };
                                // --- Hover tooltip ---
                                let hover = response.hover_pos().and_then(|pos| {
                                    let (rx, ry) = to_tile(pos)?;
                                    let prov_raw = map.get(rx, ry);
                                    let (owner, dev, terrain) = if prov_raw != 0 {
                                        if let Some(p) = st.get(ProvinceId(NonZeroU32::new(prov_raw).unwrap())) {
                                            (p.owner.map(|n| n.0.get()).unwrap_or(0), p.development, p.terrain)
                                        } else {
                                            (0, [0, 0, 0], 0)
                                        }
                                    } else {
                                        (0, [0, 0, 0], 0)
                                    };
                                    Some(HoverInfo {
                                        coords: format!("({}, {})", rx, ry),
                                        province_raw: prov_raw,
                                        owner_raw: owner,
                                        dev,
                                        terrain,
                                    })
                                });
                                // --- Brush preview (draw cursor outline) ---
                                if let Some(pos) = response.hover_pos() {
                                    if let Some((cx, cy)) = to_tile(pos) {
                                        let r = self.brush_radius as i32;
                                        for dy in -r..=r {
                                            for dx in -r..=r {
                                                let tx = cx as i32 + dx;
                                                let ty = cy as i32 + dy;
                                                if tx >= 0 && ty >= 0 && (tx as u32) < map.width && (ty as u32) < map.height {
                                                    let bx = origin.x + tx as f32 * cell_size;
                                                    let by = origin.y + ty as f32 * cell_size;
                                                    painter.rect_stroke(
                                                        egui::Rect::from_min_size(
                                                            egui::Pos2::new(bx, by),
                                                            egui::Vec2::splat(cell_size),
                                                        ),
                                                        0.0,
                                                        egui::Stroke::new(1.5, egui::Color32::from_rgba_premultiplied(0xFF, 0xFF, 0xFF, 0xAA)),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                // --- Primary click/drag → paint ---
                                let primary_hit = (response.clicked() || response.dragged_by(egui::PointerButton::Primary))
                                    .then(|| response.interact_pointer_pos())
                                    .flatten()
                                    .and_then(|pos| to_tile(pos).map(|(rx, ry)| TileHit::Square(rx, ry)));
                                // --- Secondary click/drag → erase ---
                                let secondary_hit = (response.secondary_clicked() || response.dragged_by(egui::PointerButton::Secondary))
                                    .then(|| response.interact_pointer_pos())
                                    .flatten()
                                    .and_then(|pos| to_tile(pos).map(|(rx, ry)| TileHit::Square(rx, ry)));
                                (Some((map.width, map.height)), primary_hit, secondary_hit, hover)
                            }
                            MapKind::Hex(hex) => {
                                let base_cell = 14.0_f32;
                                let cell_size = base_cell * zoom;
                                let w = hex.width;
                                let h = hex.height;
                                let hex_w = cell_size * 1.732;
                                let hex_h = cell_size * 2.0;
                                let total_w = w as f32 * hex_w * 0.75 + hex_w * 0.25;
                                let total_h = h as f32 * hex_h * 0.5 + hex_h * 0.5;
                                let (response, painter) = ui.allocate_painter(
                                    ui.available_size().max(egui::Vec2::new(total_w, total_h)),
                                    egui::Sense::click_and_drag(),
                                );
                                let rect = response.rect;

                                // --- Zoom (scroll wheel) ---
                                let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
                                if response.hovered() && scroll.abs() > 0.1 {
                                    let factor = 1.0 + scroll * 0.002;
                                    let new_zoom = (self.map_zoom * factor).clamp(0.25, 6.0);
                                    if let Some(cursor) = response.hover_pos() {
                                        let rel = cursor - rect.min - self.map_pan;
                                        self.map_pan += rel * (1.0 - new_zoom / self.map_zoom);
                                    }
                                    self.map_zoom = new_zoom;
                                }
                                // --- Pan (middle-drag) ---
                                if response.dragged_by(egui::PointerButton::Middle) {
                                    self.map_pan += response.drag_delta();
                                }

                                let cell_size = base_cell * self.map_zoom;
                                let hex_w = cell_size * 1.732;
                                let hex_h = cell_size * 2.0;
                                let origin = rect.min + self.map_pan;

                                // Visible range culling: compute the range of hex rows/cols visible on screen
                                let vis_q0 = ((rect.min.x - origin.x) / hex_w - 1.0).floor().max(0.0) as u32;
                                let vis_q1 = (((rect.max.x - origin.x) / hex_w) + 2.0).ceil().max(0.0) as u32;
                                let vis_r0 = ((rect.min.y - origin.y) / (hex_h * 0.5) - 1.0).floor().max(0.0) as u32;
                                let vis_r1 = (((rect.max.y - origin.y) / (hex_h * 0.5)) + 2.0).ceil().max(0.0) as u32;

                                for r in vis_r0..vis_r1.min(h) {
                                    for q in vis_q0..vis_q1.min(w) {
                                        let raw = hex.get(q, r);
                                        let cx = origin.x + (q as f32 + 0.5 * (r % 2) as f32) * hex_w;
                                        let cy = origin.y + (r as f32 + 0.5) * hex_h * 0.5;
                                        let (owner_raw, is_selected_prov, is_selected_nation) = if raw == 0 {
                                            (0u32, false, false)
                                        } else {
                                            let owner = st
                                                .get(ProvinceId(NonZeroU32::new(raw).unwrap()))
                                                .and_then(|p| p.owner)
                                                .map(|n| n.0.get())
                                                .unwrap_or(0);
                                            let sp = self.selected_province == Some(raw);
                                            let sn = owner != 0 && self.selected_nation == Some(owner);
                                            (owner, sp, sn)
                                        };
                                        let base_color = if raw == 0 {
                                            TILE_EMPTY_COLOR
                                        } else if owner_raw == 0 {
                                            TILE_UNOWNED_PROVINCE_COLOR
                                        } else {
                                            nation_color(owner_raw)
                                        };
                                        let mut points = Vec::with_capacity(6);
                                        for i in 0..6 {
                                            let a = std::f32::consts::FRAC_PI_3 * (i as f32);
                                            points.push(egui::Pos2::new(
                                                cx + cell_size * a.cos(),
                                                cy + cell_size * a.sin(),
                                            ));
                                        }
                                        let stroke = if is_selected_prov {
                                            egui::Stroke::new(2.5, egui::Color32::from_rgb(0xFF, 0xFF, 0x00))
                                        } else if is_selected_nation {
                                            egui::Stroke::new(1.5, egui::Color32::from_rgba_premultiplied(0xFF, 0xFF, 0xFF, 0x60))
                                        } else {
                                            province_border_stroke()
                                        };
                                        painter.add(egui::Shape::convex_polygon(points, base_color, stroke));
                                    }
                                }
                                // --- Province / nation name labels (hex) ---
                                if show_names {
                                    let mut centroids: std::collections::HashMap<u32, (f32, f32, u32, u32)> =
                                        std::collections::HashMap::new();
                                    for r in vis_r0..vis_r1.min(h) {
                                        for q in vis_q0..vis_q1.min(w) {
                                            let raw = hex.get(q, r);
                                            if raw == 0 { continue; }
                                            let owner_raw = st
                                                .get(ProvinceId(NonZeroU32::new(raw).unwrap()))
                                                .and_then(|p| p.owner)
                                                .map(|n| n.0.get())
                                                .unwrap_or(0);
                                            let hx = origin.x + (q as f32 + 0.5 * (r % 2) as f32) * hex_w;
                                            let hy = origin.y + (r as f32 + 0.5) * hex_h * 0.5;
                                            let e = centroids.entry(raw).or_insert((0.0, 0.0, 0, owner_raw));
                                            e.0 += hx;
                                            e.1 += hy;
                                            e.2 += 1;
                                        }
                                    }
                                    let font_size = (cell_size * 0.75).clamp(7.0, 18.0);
                                    let font_id = egui::FontId::proportional(font_size);
                                    for (prov_raw, (sum_x, sum_y, count, owner_raw)) in &centroids {
                                        let cx = sum_x / *count as f32;
                                        let cy = sum_y / *count as f32;
                                        let label = if paint_ownership && *owner_raw != 0 {
                                            format!("N{}", owner_raw)
                                        } else {
                                            format!("P{}", prov_raw)
                                        };
                                        let pos = egui::Pos2::new(cx, cy);
                                        let galley = painter.layout_no_wrap(label, font_id.clone(), egui::Color32::WHITE);
                                        let text_rect = egui::Rect::from_center_size(pos, galley.size());
                                        painter.rect_filled(
                                            text_rect.expand(2.0),
                                            2.0,
                                            egui::Color32::from_rgba_premultiplied(0, 0, 0, 160),
                                        );
                                        painter.galley(text_rect.min, galley, egui::Color32::WHITE);
                                    }
                                }
                                // --- Cursor → hex mapping ---
                                let to_hex = |pos: egui::Pos2| -> Option<(u32, u32)> {
                                    let px = (pos.x - origin.x) / hex_w;
                                    let py = (pos.y - origin.y) / (hex_h * 0.5);
                                    let r = (py - 0.5).floor();
                                    if r < 0.0 { return None; }
                                    let r = r as u32;
                                    let q = (px - 0.5 * (r % 2) as f32).floor();
                                    if q < 0.0 { return None; }
                                    let q = q as u32;
                                    (q < w && r < h).then_some((q, r))
                                };
                                // --- Hover tooltip ---
                                let hover = response.hover_pos().and_then(|pos| {
                                    let (hq, hr) = to_hex(pos)?;
                                    let prov_raw = hex.get(hq, hr);
                                    let (owner, dev, terrain) = if prov_raw != 0 {
                                        if let Some(p) = st.get(ProvinceId(NonZeroU32::new(prov_raw).unwrap())) {
                                            (p.owner.map(|n| n.0.get()).unwrap_or(0), p.development, p.terrain)
                                        } else {
                                            (0, [0, 0, 0], 0)
                                        }
                                    } else {
                                        (0, [0, 0, 0], 0)
                                    };
                                    Some(HoverInfo {
                                        coords: format!("({}, {})", hq, hr),
                                        province_raw: prov_raw,
                                        owner_raw: owner,
                                        dev,
                                        terrain,
                                    })
                                });
                                let primary_hit = (response.clicked() || response.dragged_by(egui::PointerButton::Primary))
                                    .then(|| response.interact_pointer_pos())
                                    .flatten()
                                    .and_then(|pos| to_hex(pos).map(|(q, r)| TileHit::Hex(q, r)));
                                let secondary_hit = (response.secondary_clicked() || response.dragged_by(egui::PointerButton::Secondary))
                                    .then(|| response.interact_pointer_pos())
                                    .flatten()
                                    .and_then(|pos| to_hex(pos).map(|(q, r)| TileHit::Hex(q, r)));
                                (Some((w, h)), primary_hit, secondary_hit, hover)
                            }
                            MapKind::Irregular(vec) => {
                                ui.label(format!(
                                    "Irregular map: {} provinces. Use Assign (below) to set owners.",
                                    vec.polygons.len()
                                ));
                                (None, None, None, None)
                            }
                        }
                    } else {
                        ui.label("No map. Create world with map_size(), map_hex(), or load a map.");
                        (None, None, None, None)
                    }
                };

                // --- Hover tooltip (non-blocking, near cursor) ---
                if let Some(info) = hover_info {
                    egui::show_tooltip_at_pointer(ctx, ui.layer_id(), egui::Id::new("map_hover_tip"), |ui| {
                        ui.set_max_width(220.0);
                        if info.province_raw == 0 {
                            ui.label(format!("{} -- Empty tile", info.coords));
                        } else {
                            ui.label(format!("{} -- Province {}", info.coords, info.province_raw));
                            let owner_text = if info.owner_raw == 0 { "Unowned".to_string() } else { format!("Nation {}", info.owner_raw) };
                            ui.label(format!("Owner: {}", owner_text));
                            let terrain_text = if info.terrain == 0 { "Land" } else { "Sea" };
                            ui.label(format!("Terrain: {}  Dev: {}/{}/{}", terrain_text, info.dev[0], info.dev[1], info.dev[2]));
                        }
                    });
                }

                // --- Apply primary paint (with brush radius) ---
                if let Some(hit) = tile_hit {
                    if !self.stroke_undo_pushed {
                        self.push_undo();
                        self.stroke_undo_pushed = true;
                    }
                    let br = self.brush_radius as i32;
                    match hit {
                        TileHit::Square(rx, ry) => {
                            if paint_ownership {
                                if let Some(nid) = self.selected_nation {
                                    for dy in -br..=br {
                                        for dx in -br..=br {
                                            let tx = rx as i32 + dx;
                                            let ty = ry as i32 + dy;
                                            if tx < 0 || ty < 0 { continue; }
                                            let (tx, ty) = (tx as u32, ty as u32);
                                            let raw = self.engine.world()
                                                .get_resource::<MapKind>()
                                                .and_then(|mk| match mk { MapKind::Square(m) => (tx < m.width && ty < m.height).then(|| m.get(tx, ty)), _ => None })
                                                .unwrap_or(0);
                                            if raw != 0 {
                                                let world_mut = self.engine.world_mut();
                                                let mut store = world_mut.get_resource_mut::<ProvinceStore>().unwrap();
                                                let pid = ProvinceId(NonZeroU32::new(raw).unwrap());
                                                let nation_id = NationId(NonZeroU32::new(nid).unwrap());
                                                if let Some(p) = store.get_mut(pid) {
                                                    p.owner = Some(nation_id);
                                                }
                                            }
                                        }
                                    }
                                }
                            } else {
                                let selected_prov = self.selected_province.unwrap_or(1);
                                for dy in -br..=br {
                                    for dx in -br..=br {
                                        let tx = rx as i32 + dx;
                                        let ty = ry as i32 + dy;
                                        if tx < 0 || ty < 0 { continue; }
                                        let (tx, ty) = (tx as u32, ty as u32);
                                        if let Some(mut mk) = self.engine.world_mut().get_resource_mut::<MapKind>() {
                                            if let MapKind::Square(ref mut m) = *mk {
                                                if tx < m.width && ty < m.height {
                                                    m.set(tx, ty, selected_prov);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        TileHit::Hex(q, r) => {
                            if paint_ownership {
                                if let Some(nid) = self.selected_nation {
                                    // For hex, brush_radius 0 = single tile, otherwise apply to neighbors
                                    let tiles = hex_brush_tiles(q, r, self.brush_radius,
                                        self.engine.world().get_resource::<MapKind>()
                                            .map(|mk| match mk { MapKind::Hex(m) => (m.width, m.height), _ => (0, 0) })
                                            .unwrap_or((0, 0)));
                                    for (hq, hr) in tiles {
                                        let raw = self.engine.world()
                                            .get_resource::<MapKind>()
                                            .and_then(|mk| match mk { MapKind::Hex(m) => Some(m.get(hq, hr)), _ => None })
                                            .unwrap_or(0);
                                        if raw != 0 {
                                            let world_mut = self.engine.world_mut();
                                            let mut store = world_mut.get_resource_mut::<ProvinceStore>().unwrap();
                                            let pid = ProvinceId(NonZeroU32::new(raw).unwrap());
                                            let nation_id = NationId(NonZeroU32::new(nid).unwrap());
                                            if let Some(p) = store.get_mut(pid) {
                                                p.owner = Some(nation_id);
                                            }
                                        }
                                    }
                                }
                            } else {
                                let selected_prov = self.selected_province.unwrap_or(1);
                                let tiles = hex_brush_tiles(q, r, self.brush_radius,
                                    self.engine.world().get_resource::<MapKind>()
                                        .map(|mk| match mk { MapKind::Hex(m) => (m.width, m.height), _ => (0, 0) })
                                        .unwrap_or((0, 0)));
                                for (hq, hr) in tiles {
                                    if let Some(mut mk) = self.engine.world_mut().get_resource_mut::<MapKind>() {
                                        if let MapKind::Hex(ref mut m) = *mk {
                                            m.set(hq, hr, selected_prov);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // --- Apply erase (right-click): set tile to 0 (empty) ---
                if let Some(hit) = erase_hit {
                    if !self.stroke_undo_pushed {
                        self.push_undo();
                        self.stroke_undo_pushed = true;
                    }
                    let br = self.brush_radius as i32;
                    match hit {
                        TileHit::Square(rx, ry) => {
                            for dy in -br..=br {
                                for dx in -br..=br {
                                    let tx = rx as i32 + dx;
                                    let ty = ry as i32 + dy;
                                    if tx < 0 || ty < 0 { continue; }
                                    let (tx, ty) = (tx as u32, ty as u32);
                                    if let Some(mut mk) = self.engine.world_mut().get_resource_mut::<MapKind>() {
                                        if let MapKind::Square(ref mut m) = *mk {
                                            if tx < m.width && ty < m.height {
                                                m.set(tx, ty, 0);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        TileHit::Hex(q, r) => {
                            let tiles = hex_brush_tiles(q, r, self.brush_radius,
                                self.engine.world().get_resource::<MapKind>()
                                    .map(|mk| match mk { MapKind::Hex(m) => (m.width, m.height), _ => (0, 0) })
                                    .unwrap_or((0, 0)));
                            for (hq, hr) in tiles {
                                if let Some(mut mk) = self.engine.world_mut().get_resource_mut::<MapKind>() {
                                    if let MapKind::Hex(ref mut m) = *mk {
                                        m.set(hq, hr, 0);
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
                            .button(egui::RichText::new("Assign province -> nation").color(egui::Color32::WHITE))
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
                    let style = world
                        .get_resource::<EventPopupStyle>()
                        .cloned()
                        .unwrap_or_default();

                    if let (Some(inst), Some(def)) = (inst, def) {
                        let mut chosen_next: Option<teleology_core::EventId> = None;
                        let mut close = false;

                        // Build styled frame
                        let [br, bg, bb, ba] = style.bg_color;
                        let frame = egui::Frame::none()
                            .fill(egui::Color32::from_rgba_unmultiplied(br, bg, bb, ba))
                            .inner_margin(egui::Margin::same(12.0))
                            .rounding(6.0)
                            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(
                                br.saturating_add(40), bg.saturating_add(40), bb.saturating_add(40), ba,
                            )));

                        let mut win = egui::Window::new("")
                            .title_bar(false)
                            .collapsible(false)
                            .resizable(false)
                            .frame(frame);

                        // Anchor / position
                        match style.anchor {
                            PopupAnchor::Center => {
                                win = win.anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]);
                            }
                            PopupAnchor::Fixed { x, y } => {
                                win = win.fixed_pos([x, y]);
                            }
                        }

                        // Width
                        if style.width > 0.0 {
                            win = win.fixed_size([style.width, 0.0]);
                        }

                        win.show(ctx, |ui| {
                            // Title
                            let [tr, tg, tb, ta] = style.title_color;
                            let title_size = if style.title_font_size > 0.0 { style.title_font_size } else { 18.0 };
                            ui.label(
                                egui::RichText::new(&def.title)
                                    .font(egui::FontId::proportional(title_size))
                                    .strong()
                                    .color(egui::Color32::from_rgba_unmultiplied(tr, tg, tb, ta)),
                            );
                            ui.add_space(4.0);
                            ui.separator();
                            ui.add_space(4.0);

                            // Image: per-event image overrides global style image
                            let (img_path, img_w, img_h) = if !def.image.is_empty() {
                                (def.image.as_str(), def.image_w, def.image_h)
                            } else if !style.image_path.is_empty() {
                                (style.image_path.as_str(), style.image_w, style.image_h)
                            } else {
                                ("", 0.0, 0.0)
                            };
                            if !img_path.is_empty() {
                                let disp_w = if img_w > 0.0 { img_w } else { 200.0 };
                                let disp_h = if img_h > 0.0 { img_h } else { 100.0 };
                                let (r, _) = ui.allocate_exact_size(
                                    egui::vec2(disp_w, disp_h),
                                    egui::Sense::hover(),
                                );
                                // Try to load image texture
                                if !self.project_thumbnails.contains_key(std::path::Path::new(img_path)) {
                                    if let Some(tex) = load_image_texture(ctx, std::path::Path::new(img_path)) {
                                        self.project_thumbnails.insert(std::path::PathBuf::from(img_path), tex);
                                    }
                                }
                                if let Some(tex) = self.project_thumbnails.get(std::path::Path::new(img_path)) {
                                    ui.painter().image(
                                        tex.id(),
                                        r,
                                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                        egui::Color32::WHITE,
                                    );
                                } else {
                                    ui.painter().rect_filled(
                                        r,
                                        4.0,
                                        egui::Color32::from_rgba_unmultiplied(60, 60, 80, 180),
                                    );
                                    ui.painter().text(
                                        r.center(),
                                        egui::Align2::CENTER_CENTER,
                                        img_path,
                                        egui::FontId::proportional(10.0),
                                        egui::Color32::from_gray(140),
                                    );
                                }
                                ui.add_space(6.0);
                            }

                            // Body
                            let [dr, dg, db, da] = style.body_color;
                            let body_size = if style.body_font_size > 0.0 { style.body_font_size } else { 14.0 };
                            ui.label(
                                egui::RichText::new(&def.body)
                                    .font(egui::FontId::proportional(body_size))
                                    .color(egui::Color32::from_rgba_unmultiplied(dr, dg, db, da)),
                            );
                            ui.add_space(10.0);

                            // Choice buttons
                            let [cr, cg, cb, _ca] = style.button_color;
                            for ch in &def.choices {
                                let btn = egui::Button::new(
                                    egui::RichText::new(&ch.text)
                                        .color(egui::Color32::from_rgb(cr, cg, cb)),
                                );
                                if ui.add_sized([ui.available_width(), 28.0], btn).clicked() {
                                    close = true;
                                    chosen_next = ch.next_event;
                                }
                                ui.add_space(2.0);
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
                                        if owner_raw == 0 { "—".to_string() } else { owner_raw.to_string() },
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
            let time = world.get_resource::<GameTime>().copied();
            let tick_unit = world.get_resource::<TimeConfig>()
                .map(|c| c.tick_unit).unwrap_or(TickUnit::Day);
            let needs_time = matches!(tick_unit, TickUnit::Second | TickUnit::Minute | TickUnit::Hour);
            if needs_time {
                if let Some(t) = time {
                    ui.label(format!("Date: {}-{:02}-{:02} {:02}:{:02}:{:02}  (tick {})", date.year, date.month, date.day, t.hour, t.minute, t.second, t.tick));
                } else {
                    ui.label(format!("Date: {}-{:02}-{:02}", date.year, date.month, date.day));
                }
            } else {
                if let Some(t) = time {
                    ui.label(format!("Date: {}-{:02}-{:02}  (tick {})", date.year, date.month, date.day, t.tick));
                } else {
                    ui.label(format!("Date: {}-{:02}-{:02}", date.year, date.month, date.day));
                }
            }
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
                        image: String::new(),
                        image_w: 0.0,
                        image_h: 0.0,
                    });
                    queue_event(world, id, teleology_core::EventScope::global(), Vec::new());
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
                            teleology_core::EventScopeRef::global(),
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

            ui.separator();
            ui.collapsing("UI Prefabs", |ui| {
                let has_registry = world.get_resource::<UiPrefabRegistry>().is_some();
                if !has_registry {
                    if ui.button("Initialize prefab registry").clicked() {
                        world.insert_resource(UiPrefabRegistry::new());
                    }
                    ui.label("Prefab registry initializes on first use (script API or button above).");
                    return;
                }

                let prefab_count = world.get_resource::<UiPrefabRegistry>()
                    .map(|r| r.prefabs.len()).unwrap_or(0);
                ui.label(format!("Prefabs: {}", prefab_count));

                // List all prefabs
                let names: Vec<String> = world.get_resource::<UiPrefabRegistry>()
                    .map(|r| r.names_sorted().into_iter().map(String::from).collect())
                    .unwrap_or_default();

                let mut to_delete: Option<String> = None;
                let mut to_preview: Option<String> = None;
                for name in &names {
                    ui.horizontal(|ui| {
                        ui.label(name);
                        let cmd_count = world.get_resource::<UiPrefabRegistry>()
                            .and_then(|r| r.get(name))
                            .map(|p| p.commands.len())
                            .unwrap_or(0);
                        ui.label(
                            egui::RichText::new(format!("({} cmds)", cmd_count))
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        );
                        if ui.small_button("Preview").clicked() {
                            to_preview = Some(name.clone());
                        }
                        if ui.small_button("Delete").clicked() {
                            to_delete = Some(name.clone());
                        }
                    });
                }
                if let Some(name) = to_delete {
                    if let Some(mut reg) = world.get_resource_mut::<UiPrefabRegistry>() {
                        reg.remove(&name);
                    }
                }
                // Preview: instantiate with empty params into the command buffer
                if let Some(name) = to_preview {
                    let expanded = world.get_resource::<UiPrefabRegistry>()
                        .and_then(|r| r.get(&name).cloned())
                        .map(|p| p.instantiate(&[]));
                    if let Some(cmds) = expanded {
                        if let Some(mut buf) = world.get_resource_mut::<UiCommandBuffer>() {
                            for cmd in cmds {
                                buf.commands.push(cmd);
                            }
                        }
                    }
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        if ui.button("Save all…").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("JSON", &["json"])
                                .set_file_name("ui_prefabs.json")
                                .save_file()
                            {
                                if let Some(reg) = world.get_resource::<UiPrefabRegistry>() {
                                    let _ = reg.save_to_file(&path);
                                }
                            }
                        }
                        if ui.button("Load all…").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("JSON", &["json"])
                                .pick_file()
                            {
                                if let Ok(reg) = UiPrefabRegistry::load_from_file(&path) {
                                    world.insert_resource(reg);
                                }
                            }
                        }
                        if ui.button("Load prefab…").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("JSON", &["json"])
                                .pick_file()
                            {
                                if let Some(mut reg) = world.get_resource_mut::<UiPrefabRegistry>() {
                                    let _ = reg.load_prefab(&path);
                                }
                            }
                        }
                    }
                });
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
