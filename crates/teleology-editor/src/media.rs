use eframe::egui;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use crate::utils::*;
use crate::EditorApp;

impl EditorApp {
    pub fn ui_project_browser(&mut self, ctx: &egui::Context) {
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

    pub fn ui_media(&mut self, ctx: &egui::Context) {
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
}
