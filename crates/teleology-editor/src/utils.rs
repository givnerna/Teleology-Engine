use eframe::egui;

/// Non-province tile (empty / water / impassable). Dark blue so clearly distinct from land.
pub const TILE_EMPTY_COLOR: egui::Color32 = egui::Color32::from_rgb(0x0f, 0x1f, 0x2f);
/// Province tile with no nation owner yet. Light warm tan so clearly distinct from empty (blue) and owned (nation colors).
pub const TILE_UNOWNED_PROVINCE_COLOR: egui::Color32 = egui::Color32::from_rgb(0x9a, 0x85, 0x6b);
/// Stroke between provinces (same or different nation) so you can tell provinces apart.
pub fn province_border_stroke() -> egui::Stroke {
    egui::Stroke::new(1.0, egui::Color32::from_rgb(0x22, 0x22, 0x22))
}

pub const NATION_COLORS: [egui::Color32; 16] = [
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
pub fn egui_key_to_code(key: egui::Key) -> u32 {
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
pub fn hex_brush_tiles(cq: u32, cr: u32, radius: u32, bounds: (u32, u32)) -> Vec<(u32, u32)> {
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

pub fn nation_color(nation_raw: u32) -> egui::Color32 {
    if nation_raw == 0 {
        return egui::Color32::from_gray(80);
    }
    let i = ((nation_raw - 1) as usize) % NATION_COLORS.len();
    NATION_COLORS[i]
}

/// Unity/Unreal-style darker panel background.
pub fn panel_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(0x2d, 0x2d, 0x30))
        .inner_margin(egui::Margin::same(6.0))
}

/// Section header bar for Hierarchy / Scene / Inspector (Unity-style).
/// Uses Sense::focusable_noninteractive() so it never steals clicks from the map or other content below.
pub fn panel_header(ui: &mut egui::Ui, title: &str) {
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
pub fn scan_resource_dir(subdir: &str, extensions: &[&str]) -> Vec<std::path::PathBuf> {
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
pub enum FileKind {
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
    pub fn from_path(path: &std::path::Path) -> Self {
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

    pub fn icon(self) -> &'static str {
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

    pub fn color(self) -> egui::Color32 {
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
pub struct ResourceEntry {
    pub path: std::path::PathBuf,
    pub name: String,
    pub kind: FileKind,
    pub size: u64,
}

/// Scan a directory for the resource browser (folders + all files).
pub fn scan_directory(dir: &std::path::Path) -> Vec<ResourceEntry> {
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
pub fn collect_folders(root: &std::path::Path) -> Vec<std::path::PathBuf> {
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

/// Default keyword highlight color (golden underline).
pub const KEYWORD_DEFAULT_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 200, 80);

/// Render text with keyword highlighting and hover tooltips.
/// Keywords found in `text` are rendered as colored, underlined spans.
/// Hovering a keyword shows a tooltip panel with its title, description, and optional icon.
pub fn render_keyword_text(
    ui: &mut egui::Ui,
    text: &str,
    font: egui::FontId,
    text_color: egui::Color32,
    keywords: &teleology_core::KeywordRegistry,
    thumbnails: &mut std::collections::HashMap<std::path::PathBuf, egui::TextureHandle>,
    ctx: &egui::Context,
) {
    let matches = keywords.find_matches(text);
    if matches.is_empty() {
        // No keywords — plain label
        ui.label(egui::RichText::new(text).font(font).color(text_color));
        return;
    }

    // Render as a horizontal_wrapped layout with mixed spans
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let mut cursor = 0usize;
        for (start, end, entry_idx) in &matches {
            // Plain text before this keyword
            if *start > cursor {
                let plain = &text[cursor..*start];
                ui.label(egui::RichText::new(plain).font(font.clone()).color(text_color));
            }
            // Keyword span
            let kw_text = &text[*start..*end];
            let entry = &keywords.entries[*entry_idx];
            let kw_color = if entry.color[3] > 0 {
                egui::Color32::from_rgba_unmultiplied(
                    entry.color[0], entry.color[1], entry.color[2], entry.color[3],
                )
            } else {
                KEYWORD_DEFAULT_COLOR
            };
            let resp = ui.add(
                egui::Label::new(
                    egui::RichText::new(kw_text)
                        .font(font.clone())
                        .color(kw_color)
                        .underline()
                )
                .sense(egui::Sense::hover()),
            );
            if resp.hovered() {
                egui::show_tooltip_at_pointer(ctx, ui.layer_id(), egui::Id::new(("kw_tip", *entry_idx)), |ui| {
                    ui.set_max_width(300.0);
                    let frame = egui::Frame::default()
                        .inner_margin(egui::Margin::same(8.0))
                        .rounding(4.0)
                        .fill(egui::Color32::from_rgba_unmultiplied(20, 20, 30, 240))
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(80)));
                    frame.show(ui, |ui| {
                        // Icon + title row
                        ui.horizontal(|ui| {
                            if !entry.icon.is_empty() {
                                let icon_path = std::path::Path::new(&entry.icon);
                                if !thumbnails.contains_key(icon_path) {
                                    if let Some(tex) = load_image_texture(ctx, icon_path) {
                                        thumbnails.insert(icon_path.to_path_buf(), tex);
                                    }
                                }
                                if let Some(tex) = thumbnails.get(icon_path) {
                                    let size = egui::vec2(20.0, 20.0);
                                    ui.image(egui::load::SizedTexture::new(tex.id(), size));
                                }
                            }
                            ui.label(
                                egui::RichText::new(&entry.title)
                                    .strong()
                                    .color(kw_color)
                                    .font(egui::FontId::proportional(14.0)),
                            );
                        });
                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(&entry.description)
                                .color(egui::Color32::from_gray(200))
                                .font(egui::FontId::proportional(12.0)),
                        );
                    });
                });
            }
            cursor = *end;
        }
        // Trailing plain text
        if cursor < text.len() {
            let tail = &text[cursor..];
            ui.label(egui::RichText::new(tail).font(font).color(text_color));
        }
    });
}

/// Recursively copy a directory and its contents to a new location.
pub fn copy_dir_recursive(src: &std::path::Path, dest: &std::path::Path) -> std::io::Result<()> {
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
pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Load an image file into an egui texture.
pub fn load_image_texture(
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
pub fn load_custom_fonts(ctx: &egui::Context) -> (Vec<std::path::PathBuf>, Vec<String>) {
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
