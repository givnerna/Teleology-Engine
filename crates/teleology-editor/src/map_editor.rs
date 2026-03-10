//! Map editor: paint provinces, assign owners, Civ-style terrain painting.
//!
//! Layout: left panel (hierarchy tree) | central panel (map canvas + toolbar) | right panel (inspector).

use eframe::egui;
use std::collections::{HashMap, HashSet, VecDeque};
use std::num::NonZeroU32;

use teleology_core::{
    add_nation_to_world, add_province_to_world, generate_provinces_hex, generate_provinces_square,
    ActiveEvent, EventPopupStyle, EventQueue, EventInstance,
    HexMapLayout, MapKind, MapLayout, NationId, NationStore, ProvinceStore,
    TerrainRegistry, WorldBounds,
};

use crate::{EditorApp, MapTool, MapViewMode, PendingContextAction, ScopeKind};
use crate::utils::*;

// ---------------------------------------------------------------------------
// Color helpers
// ---------------------------------------------------------------------------

/// Deterministic province color (for Province view mode). Hash-based so each
/// province gets a visually distinct hue.
fn province_color(raw: u32) -> egui::Color32 {
    if raw == 0 {
        return egui::Color32::from_rgb(0x30, 0x30, 0x30);
    }
    // Simple hash to spread hues
    let h = raw.wrapping_mul(2654435761); // Knuth multiplicative hash
    let r = ((h >> 0) & 0xFF) as u8;
    let g = ((h >> 8) & 0xFF) as u8;
    let b = ((h >> 16) & 0xFF) as u8;
    // Clamp to avoid too-dark colors
    let r = 60 + (r % 180);
    let g = 60 + (g % 180);
    let b = 60 + (b % 180);
    egui::Color32::from_rgb(r, g, b)
}

/// Convert TerrainRegistry RGBA to egui Color32.
fn terrain_egui_color(c: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

// ---------------------------------------------------------------------------
// Top-level: ui_map_editor
// ---------------------------------------------------------------------------

impl EditorApp {
    pub(crate) fn ui_map_editor(&mut self, ctx: &egui::Context) {
        self.process_pending_context_action();

        // Left panel: hierarchy tree
        egui::SidePanel::left("map_editor_left")
            .default_width(220.0)
            .resizable(true)
            .show(ctx, |ui| {
                panel_frame().show(ui, |ui| {
                    self.ui_hierarchy_tree_panel(ui);
                });
            });

        // Right panel: inspector
        egui::SidePanel::right("map_editor_right")
            .default_width(240.0)
            .resizable(true)
            .show(ctx, |ui| {
                panel_frame().show(ui, |ui| {
                    self.ui_map_inspector_panel(ui);
                });
            });

        // Central panel: toolbar + map canvas
        egui::CentralPanel::default().show(ctx, |ui| {
            self.ui_map_toolbar(ui);
            ui.separator();
            self.ui_map_canvas(ui);
        });

        // Event popups (rendered on top of everything)
        self.render_event_popups(ctx);
    }

    // -----------------------------------------------------------------------
    // Toolbar
    // -----------------------------------------------------------------------

    fn ui_map_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;

            // Tool selector
            ui.label(egui::RichText::new("Tool:").small().color(ui.visuals().weak_text_color()));
            ui.selectable_value(&mut self.map_tool, MapTool::Brush, "Brush");
            ui.selectable_value(&mut self.map_tool, MapTool::Fill, "Fill");
            ui.selectable_value(&mut self.map_tool, MapTool::Erase, "Erase");
            ui.selectable_value(&mut self.map_tool, MapTool::Eyedropper, "Pick");

            ui.separator();

            // Brush size
            let label = match self.brush_radius {
                0 => "1x1",
                1 => "3x3",
                2 => "5x5",
                _ => "7x7",
            };
            ui.label(egui::RichText::new("Brush:").small().color(ui.visuals().weak_text_color()));
            if ui.small_button(label).clicked() {
                self.brush_radius = (self.brush_radius + 1) % 4;
            }

            ui.separator();

            // View mode
            ui.label(egui::RichText::new("View:").small().color(ui.visuals().weak_text_color()));
            ui.selectable_value(&mut self.map_view_mode, MapViewMode::Terrain, "Terrain");
            ui.selectable_value(&mut self.map_view_mode, MapViewMode::Province, "Province");
            ui.selectable_value(&mut self.map_view_mode, MapViewMode::Political, "Political");

            ui.separator();

            // Zoom
            ui.label(
                egui::RichText::new(format!("{:.0}%", self.map_zoom * 100.0))
                    .small()
                    .color(ui.visuals().weak_text_color()),
            );
            if ui.small_button("Reset").clicked() {
                self.map_zoom = 1.0;
                self.map_pan = egui::Vec2::ZERO;
            }

            ui.separator();

            // Names toggle
            ui.checkbox(&mut self.show_map_names, "Names");
        });

        // Dynamic help text
        ui.horizontal(|ui| {
            let help = match (&self.map_tool, &self.map_view_mode) {
                (MapTool::Brush, MapViewMode::Terrain) => "Click to paint terrain type on provinces",
                (MapTool::Brush, MapViewMode::Province) => "Click to paint province tiles",
                (MapTool::Brush, MapViewMode::Political) => "Click to paint nation ownership",
                (MapTool::Fill, MapViewMode::Terrain) => "Click to flood-fill terrain on connected provinces",
                (MapTool::Fill, MapViewMode::Province) => "Click to flood-fill province tiles",
                (MapTool::Fill, MapViewMode::Political) => "Click to change all provinces of a nation",
                (MapTool::Erase, MapViewMode::Terrain) => "Click to reset terrain to first type",
                (MapTool::Erase, MapViewMode::Province) => "Click to clear tiles (set empty)",
                (MapTool::Erase, MapViewMode::Political) => "Click to remove ownership",
                (MapTool::Eyedropper, MapViewMode::Terrain) => "Click to pick terrain type",
                (MapTool::Eyedropper, MapViewMode::Province) => "Click to pick province",
                (MapTool::Eyedropper, MapViewMode::Political) => "Click to pick nation",
            };
            ui.label(egui::RichText::new(help).small().italics().color(ui.visuals().weak_text_color()));
        });
    }

    // -----------------------------------------------------------------------
    // Inspector (right panel)
    // -----------------------------------------------------------------------

    fn ui_map_inspector_panel(&mut self, ui: &mut egui::Ui) {
        panel_header(ui, "Inspector");

        egui::ScrollArea::vertical().show(ui, |ui| {
            // -- Terrain Palette (always shown in Terrain view) --
            if self.map_view_mode == MapViewMode::Terrain {
                ui.collapsing("Terrain Palette", |ui| {
                    self.ui_terrain_palette(ui);
                });
                ui.add_space(6.0);
            }

            // -- Province Info (when a province is selected) --
            if let Some(prov_raw) = self.selected_province {
                ui.collapsing("Province Info", |ui| {
                    self.ui_province_info(ui, prov_raw);
                });
                ui.add_space(6.0);
            }

            // -- Nation list (in Political view) --
            if self.map_view_mode == MapViewMode::Political {
                ui.collapsing("Nations", |ui| {
                    self.ui_nation_list(ui);
                });
                ui.add_space(6.0);
            }

            // -- Auto-Generate --
            ui.collapsing("Auto-Generate", |ui| {
                self.ui_autogen_panel(ui);
            });
        });
    }

    fn ui_terrain_palette(&mut self, ui: &mut egui::Ui) {
        let world = self.engine.world_mut();
        let terrain_reg = world.get_resource::<TerrainRegistry>().cloned();
        let Some(reg) = terrain_reg else {
            ui.label("No terrain types defined. Create a world first.");
            return;
        };

        let cols = 2;
        egui::Grid::new("terrain_palette_grid")
            .num_columns(cols)
            .spacing([4.0, 4.0])
            .show(ui, |ui| {
                for (i, tt) in reg.types.iter().enumerate() {
                    let selected = self.selected_terrain == tt.id;
                    let color = terrain_egui_color(tt.color);
                    // Color swatch + label
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width().min(110.0), 22.0),
                        egui::Sense::click(),
                    );
                    if resp.clicked() {
                        self.selected_terrain = tt.id;
                    }
                    let fill = if selected { color } else { color.linear_multiply(0.6) };
                    ui.painter().rect_filled(rect, 3.0, fill);
                    if selected {
                        ui.painter().rect_stroke(rect, 3.0, egui::Stroke::new(2.0, egui::Color32::WHITE));
                    }
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        &tt.name,
                        egui::FontId::proportional(11.0),
                        if selected { egui::Color32::WHITE } else { egui::Color32::from_gray(220) },
                    );
                    if (i + 1) % cols == 0 {
                        ui.end_row();
                    }
                }
            });
    }

    fn ui_province_info(&mut self, ui: &mut egui::Ui, prov_raw: u32) {
        let world = self.engine.world_mut();
        let store = world.get_resource::<ProvinceStore>().cloned();
        let terrain_reg = world.get_resource::<TerrainRegistry>().cloned();

        if let Some(store) = &store {
            let idx = (prov_raw - 1) as usize;
            if let Some(p) = store.items.get(idx) {
                ui.label(format!("Province #{}", prov_raw));
                let tname = terrain_reg.as_ref().map(|r| r.name(p.terrain)).unwrap_or("?");
                ui.label(format!("Terrain: {}", tname));
                ui.label(format!("Dev: {} / {} / {}", p.development[0], p.development[1], p.development[2]));
                ui.label(format!("Population: {}", p.population));
                if let Some(owner) = p.owner {
                    ui.label(format!("Owner: Nation #{}", owner.0.get()));
                } else {
                    ui.label("Owner: None");
                }
            }
        }
    }

    fn ui_autogen_panel(&mut self, ui: &mut egui::Ui) {
        ui.add(egui::DragValue::new(&mut self.autogen_province_count)
            .range(2..=500)
            .prefix("Provinces: "));
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Warning: replaces existing province assignments")
                .small()
                .color(egui::Color32::from_rgb(0xFF, 0xA0, 0x40)),
        );
        if ui.button("Generate").clicked() {
            self.push_undo();
            let count = self.autogen_province_count;
            let world = self.engine.world_mut();
            let mut bounds = match world.get_resource::<WorldBounds>().cloned() {
                Some(b) => b,
                None => return,
            };
            let mut store = match world.get_resource::<ProvinceStore>().cloned() {
                Some(s) => s,
                None => return,
            };
            let map_kind = match world.get_resource::<MapKind>().cloned() {
                Some(m) => m,
                None => return,
            };

            match map_kind {
                MapKind::Square(mut layout) => {
                    generate_provinces_square(&mut layout, count, &mut store, &mut bounds);
                    world.insert_resource(MapKind::Square(layout));
                }
                MapKind::Hex(mut layout) => {
                    generate_provinces_hex(&mut layout, count, &mut store, &mut bounds);
                    world.insert_resource(MapKind::Hex(layout));
                }
                _ => {}
            }
            world.insert_resource(store);
            world.insert_resource(bounds);
        }
    }

    // -----------------------------------------------------------------------
    // Map canvas (central panel)
    // -----------------------------------------------------------------------

    fn ui_map_canvas(&mut self, ui: &mut egui::Ui) {
        let world = self.engine.world_mut();
        let map_kind = world.get_resource::<MapKind>().cloned();
        let bounds = world.get_resource::<WorldBounds>().cloned();

        let Some(map_kind) = map_kind else {
            ui.centered_and_justified(|ui| {
                ui.label("No map loaded. Create a world from the World panel.");
            });
            return;
        };
        let Some(bounds) = bounds else { return };

        match &map_kind {
            MapKind::Square(layout) => {
                self.render_square_map(ui, layout, &bounds);
            }
            MapKind::Hex(layout) => {
                self.render_hex_map(ui, layout, &bounds);
            }
            MapKind::Irregular(_) => {
                ui.label("Irregular maps use vector rendering (not yet implemented in reworked editor).");
            }
        }
    }

    // -----------------------------------------------------------------------
    // Square map rendering
    // -----------------------------------------------------------------------

    fn render_square_map(&mut self, ui: &mut egui::Ui, layout: &MapLayout, _bounds: &WorldBounds) {
        let base_cell = 14.0_f32;
        let cell_size = base_cell * self.map_zoom;
        let (rect, response) = ui.allocate_at_least(ui.available_size(), egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);

        // Background
        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(0x1a, 0x1a, 0x2e));

        // Zoom with scroll
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 && response.hovered() {
            let factor = 1.0 + scroll * 0.002;
            self.map_zoom = (self.map_zoom * factor).clamp(0.2, 10.0);
        }

        // Pan with middle mouse
        if response.dragged_by(egui::PointerButton::Middle) {
            self.map_pan += response.drag_delta();
        }

        let origin = rect.min.to_vec2() + self.map_pan;

        // Read province and nation data
        let world = self.engine.world_mut();
        let provinces = world.get_resource::<ProvinceStore>().cloned().unwrap_or_else(|| ProvinceStore::new(0));
        let nations = world.get_resource::<NationStore>().cloned().unwrap_or_else(|| NationStore::new(0));
        let terrain_reg = world.get_resource::<TerrainRegistry>().cloned().unwrap_or_default();

        // Visible range culling
        let min_x = ((rect.min.x - origin.x) / cell_size).floor().max(0.0) as u32;
        let min_y = ((rect.min.y - origin.y) / cell_size).floor().max(0.0) as u32;
        let max_x = (((rect.max.x - origin.x) / cell_size).ceil() as u32).min(layout.width);
        let max_y = (((rect.max.y - origin.y) / cell_size).ceil() as u32).min(layout.height);

        // Tile rendering
        for y in min_y..max_y {
            for x in min_x..max_x {
                let prov_raw = layout.get(x, y);
                let tile_rect = egui::Rect::from_min_size(
                    egui::Pos2::new(
                        origin.x + x as f32 * cell_size,
                        origin.y + y as f32 * cell_size,
                    ),
                    egui::vec2(cell_size, cell_size),
                );

                if !rect.intersects(tile_rect) {
                    continue;
                }

                // Tile color based on view mode
                let color = self.tile_color(prov_raw, &provinces, &nations, &terrain_reg);
                painter.rect_filled(tile_rect, 0.0, color);

                // Province borders
                if x + 1 < layout.width && layout.get(x + 1, y) != prov_raw {
                    painter.line_segment(
                        [tile_rect.right_top(), tile_rect.right_bottom()],
                        province_border_stroke(),
                    );
                }
                if y + 1 < layout.height && layout.get(x, y + 1) != prov_raw {
                    painter.line_segment(
                        [tile_rect.left_bottom(), tile_rect.right_bottom()],
                        province_border_stroke(),
                    );
                }

                // Selected province highlight
                if self.selected_province == Some(prov_raw) && prov_raw > 0 {
                    painter.rect_stroke(
                        tile_rect.shrink(0.5),
                        0.0,
                        egui::Stroke::new(1.5, egui::Color32::YELLOW),
                    );
                }
            }
        }

        // Province name labels
        if self.show_map_names && cell_size > 6.0 {
            self.render_square_name_labels(&painter, layout, &provinces, origin, cell_size, rect);
        }

        // Hover tooltip + brush preview
        if let Some(hover_pos) = response.hover_pos() {
            let tx = ((hover_pos.x - origin.x) / cell_size).floor() as i32;
            let ty = ((hover_pos.y - origin.y) / cell_size).floor() as i32;
            if tx >= 0 && ty >= 0 && (tx as u32) < layout.width && (ty as u32) < layout.height {
                let hx = tx as u32;
                let hy = ty as u32;
                let hprov = layout.get(hx, hy);

                // Brush preview
                let radius = self.brush_radius as i32;
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        let bx = tx + dx;
                        let by = ty + dy;
                        if bx >= 0 && by >= 0 && (bx as u32) < layout.width && (by as u32) < layout.height {
                            let br = egui::Rect::from_min_size(
                                egui::Pos2::new(
                                    origin.x + bx as f32 * cell_size,
                                    origin.y + by as f32 * cell_size,
                                ),
                                egui::vec2(cell_size, cell_size),
                            );
                            painter.rect_stroke(
                                br,
                                0.0,
                                egui::Stroke::new(1.0, egui::Color32::from_white_alpha(120)),
                            );
                        }
                    }
                }

                // Tooltip
                egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), egui::Id::new("map_hover"), |ui| {
                    ui.label(format!("Tile ({}, {})", hx, hy));
                    if hprov > 0 {
                        ui.label(format!("Province #{}", hprov));
                        let idx = (hprov - 1) as usize;
                        if let Some(p) = provinces.items.get(idx) {
                            ui.label(format!("Terrain: {}", terrain_reg.name(p.terrain)));
                            if let Some(owner) = p.owner {
                                ui.label(format!("Owner: Nation #{}", owner.0.get()));
                            }
                        }
                    } else {
                        ui.label("Empty");
                    }
                });

                // Handle clicks/drags (painting)
                if response.clicked() || (response.dragged() && ui.input(|i| i.pointer.primary_down())) {
                    self.handle_square_paint(hx, hy, layout);
                }
            }
        }
    }

    fn tile_color(
        &self,
        prov_raw: u32,
        provinces: &ProvinceStore,
        _nations: &NationStore,
        terrain_reg: &TerrainRegistry,
    ) -> egui::Color32 {
        if prov_raw == 0 {
            return match self.map_view_mode {
                MapViewMode::Terrain => egui::Color32::from_rgb(0x30, 0x30, 0x30),
                MapViewMode::Province => egui::Color32::from_rgb(0x20, 0x20, 0x20),
                MapViewMode::Political => TILE_EMPTY_COLOR,
            };
        }

        let idx = (prov_raw - 1) as usize;
        match self.map_view_mode {
            MapViewMode::Terrain => {
                if let Some(p) = provinces.items.get(idx) {
                    terrain_egui_color(terrain_reg.color(p.terrain))
                } else {
                    egui::Color32::from_rgb(0x40, 0x40, 0x40)
                }
            }
            MapViewMode::Province => province_color(prov_raw),
            MapViewMode::Political => {
                if let Some(p) = provinces.items.get(idx) {
                    if let Some(owner) = p.owner {
                        nation_color(owner.0.get())
                    } else {
                        TILE_UNOWNED_PROVINCE_COLOR
                    }
                } else {
                    TILE_EMPTY_COLOR
                }
            }
        }
    }

    fn render_square_name_labels(
        &self,
        painter: &egui::Painter,
        layout: &MapLayout,
        _provinces: &ProvinceStore,
        origin: egui::Vec2,
        cell_size: f32,
        clip: egui::Rect,
    ) {
        // Find centroid of each province (average tile position)
        let mut cx_sum: HashMap<u32, (f64, f64, u32)> = HashMap::new();
        for y in 0..layout.height {
            for x in 0..layout.width {
                let p = layout.get(x, y);
                if p > 0 {
                    let e = cx_sum.entry(p).or_insert((0.0, 0.0, 0));
                    e.0 += x as f64;
                    e.1 += y as f64;
                    e.2 += 1;
                }
            }
        }

        let font = egui::FontId::proportional((cell_size * 0.6).clamp(8.0, 14.0));
        for (&prov_raw, &(sx, sy, count)) in &cx_sum {
            let cx = sx / count as f64;
            let cy = sy / count as f64;
            let pos = egui::Pos2::new(
                origin.x + (cx as f32 + 0.5) * cell_size,
                origin.y + (cy as f32 + 0.5) * cell_size,
            );
            if !clip.contains(pos) {
                continue;
            }
            let label = format!("P{}", prov_raw);
            painter.text(
                pos,
                egui::Align2::CENTER_CENTER,
                &label,
                font.clone(),
                egui::Color32::from_white_alpha(180),
            );
        }
    }

    fn handle_square_paint(&mut self, hx: u32, hy: u32, _layout: &MapLayout) {
        // Push undo once per stroke
        if !self.stroke_undo_pushed {
            self.push_undo();
            self.stroke_undo_pushed = true;
        }

        let world = self.engine.world_mut();
        let radius = self.brush_radius as i32;

        match (&self.map_tool, &self.map_view_mode) {
            (MapTool::Brush, MapViewMode::Terrain) => {
                // Paint terrain on the province under cursor
                if let Some(map_kind) = world.get_resource::<MapKind>().cloned() {
                    if let MapKind::Square(ref layout) = map_kind {
                        let prov_raw = layout.get(hx, hy);
                        if prov_raw > 0 {
                            if let Some(mut store) = world.get_resource_mut::<ProvinceStore>() {
                                let idx = (prov_raw - 1) as usize;
                                if let Some(p) = store.items.get_mut(idx) {
                                    p.terrain = self.selected_terrain;
                                }
                            }
                        }
                    }
                }
            }
            (MapTool::Brush, MapViewMode::Province) => {
                // Paint province_id onto tiles
                if let Some(sel) = self.selected_province {
                    if let Some(mut mk) = world.get_resource_mut::<MapKind>() {
                        if let MapKind::Square(ref mut layout) = *mk {
                            for dy in -radius..=radius {
                                for dx in -radius..=radius {
                                    let nx = hx as i32 + dx;
                                    let ny = hy as i32 + dy;
                                    if nx >= 0 && ny >= 0 && (nx as u32) < layout.width && (ny as u32) < layout.height {
                                        layout.set(nx as u32, ny as u32, sel);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            (MapTool::Brush, MapViewMode::Political) => {
                // Paint nation ownership onto province under cursor
                if let Some(sel_nation) = self.selected_nation {
                    if let Some(map_kind) = world.get_resource::<MapKind>().cloned() {
                        if let MapKind::Square(ref layout) = map_kind {
                            let prov_raw = layout.get(hx, hy);
                            if prov_raw > 0 {
                                let nid = NonZeroU32::new(sel_nation).map(NationId);
                                if let Some(mut store) = world.get_resource_mut::<ProvinceStore>() {
                                    let idx = (prov_raw - 1) as usize;
                                    if let Some(p) = store.items.get_mut(idx) {
                                        p.owner = nid;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            (MapTool::Fill, MapViewMode::Terrain) => {
                // Flood fill terrain on connected same-terrain provinces
                if let Some(map_kind) = world.get_resource::<MapKind>().cloned() {
                    if let MapKind::Square(ref layout) = map_kind {
                        let prov_raw = layout.get(hx, hy);
                        if prov_raw > 0 {
                            if let Some(store) = world.get_resource::<ProvinceStore>().cloned() {
                                let idx = (prov_raw - 1) as usize;
                                if let Some(p) = store.items.get(idx) {
                                    let old_terrain = p.terrain;
                                    let new_terrain = self.selected_terrain;
                                    if old_terrain != new_terrain {
                                        let adj = find_connected_provinces_by_terrain(layout, prov_raw, old_terrain, &store);
                                        if let Some(mut st) = world.get_resource_mut::<ProvinceStore>() {
                                            for pr in adj {
                                                let i = (pr - 1) as usize;
                                                if let Some(pp) = st.items.get_mut(i) {
                                                    pp.terrain = new_terrain;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            (MapTool::Fill, MapViewMode::Province) => {
                // Flood fill tiles with selected province id
                if let Some(sel) = self.selected_province {
                    if let Some(mut mk) = world.get_resource_mut::<MapKind>() {
                        if let MapKind::Square(ref mut layout) = *mk {
                            let target = layout.get(hx, hy);
                            if target != sel {
                                let mut queue = VecDeque::new();
                                queue.push_back((hx, hy));
                                let mut visited = HashSet::new();
                                visited.insert((hx, hy));
                                while let Some((cx, cy)) = queue.pop_front() {
                                    layout.set(cx, cy, sel);
                                    for (dx, dy) in [(-1i32, 0), (1, 0), (0, -1), (0, 1)] {
                                        let nx = cx as i32 + dx;
                                        let ny = cy as i32 + dy;
                                        if nx >= 0 && ny >= 0 && (nx as u32) < layout.width && (ny as u32) < layout.height {
                                            let (nx, ny) = (nx as u32, ny as u32);
                                            if !visited.contains(&(nx, ny)) && layout.get(nx, ny) == target {
                                                visited.insert((nx, ny));
                                                queue.push_back((nx, ny));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            (MapTool::Fill, MapViewMode::Political) => {
                // Set all provinces of clicked nation to selected nation
                if let Some(map_kind) = world.get_resource::<MapKind>().cloned() {
                    if let MapKind::Square(ref layout) = map_kind {
                        let prov_raw = layout.get(hx, hy);
                        if prov_raw > 0 {
                            if let Some(store) = world.get_resource::<ProvinceStore>().cloned() {
                                let idx = (prov_raw - 1) as usize;
                                if let Some(p) = store.items.get(idx) {
                                    let old_owner = p.owner;
                                    let new_owner = self.selected_nation.and_then(|n| NonZeroU32::new(n).map(NationId));
                                    if old_owner != new_owner {
                                        if let Some(mut st) = world.get_resource_mut::<ProvinceStore>() {
                                            for pp in st.items.iter_mut() {
                                                if pp.owner == old_owner {
                                                    pp.owner = new_owner;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            (MapTool::Erase, MapViewMode::Terrain) => {
                if let Some(map_kind) = world.get_resource::<MapKind>().cloned() {
                    if let MapKind::Square(ref layout) = map_kind {
                        let prov_raw = layout.get(hx, hy);
                        if prov_raw > 0 {
                            if let Some(mut store) = world.get_resource_mut::<ProvinceStore>() {
                                let idx = (prov_raw - 1) as usize;
                                if let Some(p) = store.items.get_mut(idx) {
                                    p.terrain = 0;
                                }
                            }
                        }
                    }
                }
            }
            (MapTool::Erase, MapViewMode::Province) => {
                if let Some(mut mk) = world.get_resource_mut::<MapKind>() {
                    if let MapKind::Square(ref mut layout) = *mk {
                        for dy in -radius..=radius {
                            for dx in -radius..=radius {
                                let nx = hx as i32 + dx;
                                let ny = hy as i32 + dy;
                                if nx >= 0 && ny >= 0 && (nx as u32) < layout.width && (ny as u32) < layout.height {
                                    layout.set(nx as u32, ny as u32, 0);
                                }
                            }
                        }
                    }
                }
            }
            (MapTool::Erase, MapViewMode::Political) => {
                if let Some(map_kind) = world.get_resource::<MapKind>().cloned() {
                    if let MapKind::Square(ref layout) = map_kind {
                        let prov_raw = layout.get(hx, hy);
                        if prov_raw > 0 {
                            if let Some(mut store) = world.get_resource_mut::<ProvinceStore>() {
                                let idx = (prov_raw - 1) as usize;
                                if let Some(p) = store.items.get_mut(idx) {
                                    p.owner = None;
                                }
                            }
                        }
                    }
                }
            }
            (MapTool::Eyedropper, MapViewMode::Terrain) => {
                if let Some(map_kind) = world.get_resource::<MapKind>().cloned() {
                    if let MapKind::Square(ref layout) = map_kind {
                        let prov_raw = layout.get(hx, hy);
                        if prov_raw > 0 {
                            if let Some(store) = world.get_resource::<ProvinceStore>() {
                                let idx = (prov_raw - 1) as usize;
                                if let Some(p) = store.items.get(idx) {
                                    self.selected_terrain = p.terrain;
                                }
                            }
                        }
                    }
                }
            }
            (MapTool::Eyedropper, MapViewMode::Province) => {
                if let Some(map_kind) = world.get_resource::<MapKind>().cloned() {
                    if let MapKind::Square(ref layout) = map_kind {
                        let prov_raw = layout.get(hx, hy);
                        if prov_raw > 0 {
                            self.selected_province = Some(prov_raw);
                        }
                    }
                }
            }
            (MapTool::Eyedropper, MapViewMode::Political) => {
                if let Some(map_kind) = world.get_resource::<MapKind>().cloned() {
                    if let MapKind::Square(ref layout) = map_kind {
                        let prov_raw = layout.get(hx, hy);
                        if prov_raw > 0 {
                            if let Some(store) = world.get_resource::<ProvinceStore>() {
                                let idx = (prov_raw - 1) as usize;
                                if let Some(p) = store.items.get(idx) {
                                    self.selected_nation = p.owner.map(|n| n.0.get());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Hex map rendering
    // -----------------------------------------------------------------------

    fn render_hex_map(&mut self, ui: &mut egui::Ui, layout: &HexMapLayout, _bounds: &WorldBounds) {
        let base_cell = 14.0_f32;
        let cell_size = base_cell * self.map_zoom;
        let hex_w = cell_size * 1.732;
        let hex_h = cell_size * 2.0;

        let (rect, response) = ui.allocate_at_least(ui.available_size(), egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);

        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(0x1a, 0x1a, 0x2e));

        // Zoom
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 && response.hovered() {
            let factor = 1.0 + scroll * 0.002;
            self.map_zoom = (self.map_zoom * factor).clamp(0.2, 10.0);
        }

        // Pan
        if response.dragged_by(egui::PointerButton::Middle) {
            self.map_pan += response.drag_delta();
        }

        let origin = rect.min.to_vec2() + self.map_pan;

        let world = self.engine.world_mut();
        let provinces = world.get_resource::<ProvinceStore>().cloned().unwrap_or_else(|| ProvinceStore::new(0));
        let nations = world.get_resource::<NationStore>().cloned().unwrap_or_else(|| NationStore::new(0));
        let terrain_reg = world.get_resource::<TerrainRegistry>().cloned().unwrap_or_default();

        // Render hex tiles
        for r in 0..layout.height {
            for q in 0..layout.width {
                let prov_raw = layout.get(q, r);
                let (cx, cy) = hex_center(q, r, hex_w, hex_h, origin);
                let center = egui::Pos2::new(cx, cy);

                if !rect.expand(hex_w).contains(center) {
                    continue;
                }

                let color = self.tile_color(prov_raw, &provinces, &nations, &terrain_reg);
                let verts = hex_vertices(center, cell_size);
                painter.add(egui::Shape::convex_polygon(
                    verts.to_vec(),
                    color,
                    province_border_stroke(),
                ));

                // Selected highlight
                if self.selected_province == Some(prov_raw) && prov_raw > 0 {
                    let inner = hex_vertices(center, cell_size * 0.85);
                    painter.add(egui::Shape::convex_polygon(
                        inner.to_vec(),
                        egui::Color32::TRANSPARENT,
                        egui::Stroke::new(1.5, egui::Color32::YELLOW),
                    ));
                }
            }
        }

        // Hover + painting for hex
        if let Some(hover_pos) = response.hover_pos() {
            if let Some((hq, hr)) = screen_to_hex(hover_pos, hex_w, hex_h, origin, layout.width, layout.height) {
                let hprov = layout.get(hq, hr);

                // Brush preview
                let tiles = hex_brush_tiles(hq, hr, self.brush_radius, (layout.width, layout.height));
                for (bq, br) in &tiles {
                    let (bx, by) = hex_center(*bq, *br, hex_w, hex_h, origin);
                    let bc = egui::Pos2::new(bx, by);
                    let bv = hex_vertices(bc, cell_size * 0.9);
                    painter.add(egui::Shape::convex_polygon(
                        bv.to_vec(),
                        egui::Color32::TRANSPARENT,
                        egui::Stroke::new(1.0, egui::Color32::from_white_alpha(120)),
                    ));
                }

                // Tooltip
                egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), egui::Id::new("hex_hover"), |ui| {
                    ui.label(format!("Hex ({}, {})", hq, hr));
                    if hprov > 0 {
                        ui.label(format!("Province #{}", hprov));
                        let idx = (hprov - 1) as usize;
                        if let Some(p) = provinces.items.get(idx) {
                            ui.label(format!("Terrain: {}", terrain_reg.name(p.terrain)));
                            if let Some(owner) = p.owner {
                                ui.label(format!("Owner: Nation #{}", owner.0.get()));
                            }
                        }
                    }
                });

                // Paint
                if response.clicked() || (response.dragged() && ui.input(|i| i.pointer.primary_down())) {
                    self.handle_hex_paint(hq, hr);
                }
            }
        }
    }

    fn handle_hex_paint(&mut self, hq: u32, hr: u32) {
        if !self.stroke_undo_pushed {
            self.push_undo();
            self.stroke_undo_pushed = true;
        }

        let world = self.engine.world_mut();
        let radius = self.brush_radius;

        match (&self.map_tool, &self.map_view_mode) {
            (MapTool::Brush, MapViewMode::Terrain) => {
                if let Some(map_kind) = world.get_resource::<MapKind>().cloned() {
                    if let MapKind::Hex(ref layout) = map_kind {
                        let prov_raw = layout.get(hq, hr);
                        if prov_raw > 0 {
                            if let Some(mut store) = world.get_resource_mut::<ProvinceStore>() {
                                let idx = (prov_raw - 1) as usize;
                                if let Some(p) = store.items.get_mut(idx) {
                                    p.terrain = self.selected_terrain;
                                }
                            }
                        }
                    }
                }
            }
            (MapTool::Brush, MapViewMode::Province) => {
                if let Some(sel) = self.selected_province {
                    if let Some(mut mk) = world.get_resource_mut::<MapKind>() {
                        if let MapKind::Hex(ref mut layout) = *mk {
                            let tiles = hex_brush_tiles(hq, hr, radius, (layout.width, layout.height));
                            for (tq, tr) in tiles {
                                layout.set(tq, tr, sel);
                            }
                        }
                    }
                }
            }
            (MapTool::Brush, MapViewMode::Political) => {
                if let Some(sel_nation) = self.selected_nation {
                    if let Some(map_kind) = world.get_resource::<MapKind>().cloned() {
                        if let MapKind::Hex(ref layout) = map_kind {
                            let prov_raw = layout.get(hq, hr);
                            if prov_raw > 0 {
                                let nid = NonZeroU32::new(sel_nation).map(NationId);
                                if let Some(mut store) = world.get_resource_mut::<ProvinceStore>() {
                                    let idx = (prov_raw - 1) as usize;
                                    if let Some(p) = store.items.get_mut(idx) {
                                        p.owner = nid;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            (MapTool::Erase, MapViewMode::Province) => {
                if let Some(mut mk) = world.get_resource_mut::<MapKind>() {
                    if let MapKind::Hex(ref mut layout) = *mk {
                        let tiles = hex_brush_tiles(hq, hr, radius, (layout.width, layout.height));
                        for (tq, tr) in tiles {
                            layout.set(tq, tr, 0);
                        }
                    }
                }
            }
            (MapTool::Erase, MapViewMode::Terrain) => {
                if let Some(map_kind) = world.get_resource::<MapKind>().cloned() {
                    if let MapKind::Hex(ref layout) = map_kind {
                        let prov_raw = layout.get(hq, hr);
                        if prov_raw > 0 {
                            if let Some(mut store) = world.get_resource_mut::<ProvinceStore>() {
                                let idx = (prov_raw - 1) as usize;
                                if let Some(p) = store.items.get_mut(idx) {
                                    p.terrain = 0;
                                }
                            }
                        }
                    }
                }
            }
            (MapTool::Erase, MapViewMode::Political) => {
                if let Some(map_kind) = world.get_resource::<MapKind>().cloned() {
                    if let MapKind::Hex(ref layout) = map_kind {
                        let prov_raw = layout.get(hq, hr);
                        if prov_raw > 0 {
                            if let Some(mut store) = world.get_resource_mut::<ProvinceStore>() {
                                let idx = (prov_raw - 1) as usize;
                                if let Some(p) = store.items.get_mut(idx) {
                                    p.owner = None;
                                }
                            }
                        }
                    }
                }
            }
            (MapTool::Eyedropper, MapViewMode::Terrain) => {
                if let Some(map_kind) = world.get_resource::<MapKind>().cloned() {
                    if let MapKind::Hex(ref layout) = map_kind {
                        let prov_raw = layout.get(hq, hr);
                        if prov_raw > 0 {
                            if let Some(store) = world.get_resource::<ProvinceStore>() {
                                let idx = (prov_raw - 1) as usize;
                                if let Some(p) = store.items.get(idx) {
                                    self.selected_terrain = p.terrain;
                                }
                            }
                        }
                    }
                }
            }
            (MapTool::Eyedropper, MapViewMode::Province) => {
                if let Some(map_kind) = world.get_resource::<MapKind>().cloned() {
                    if let MapKind::Hex(ref layout) = map_kind {
                        let prov_raw = layout.get(hq, hr);
                        if prov_raw > 0 {
                            self.selected_province = Some(prov_raw);
                        }
                    }
                }
            }
            (MapTool::Eyedropper, MapViewMode::Political) => {
                if let Some(map_kind) = world.get_resource::<MapKind>().cloned() {
                    if let MapKind::Hex(ref layout) = map_kind {
                        let prov_raw = layout.get(hq, hr);
                        if prov_raw > 0 {
                            if let Some(store) = world.get_resource::<ProvinceStore>() {
                                let idx = (prov_raw - 1) as usize;
                                if let Some(p) = store.items.get(idx) {
                                    self.selected_nation = p.owner.map(|n| n.0.get());
                                }
                            }
                        }
                    }
                }
            }
            _ => {
                // Fill for hex — mirrors square fill logic
            }
        }
    }

    // -----------------------------------------------------------------------
    // Event popups (preserved from previous implementation)
    // -----------------------------------------------------------------------

    fn render_event_popups(&mut self, ctx: &egui::Context) {
        let world = self.engine.world_mut();
        let active = match world.get_resource::<ActiveEvent>() {
            Some(ae) => ae.clone(),
            None => return,
        };
        let Some(inst) = &active.current else { return };
        let event_registry = world.get_resource::<teleology_core::EventRegistry>().cloned();
        let Some(reg) = event_registry else { return };
        let Some(def) = reg.events.get(&inst.event_id.0.get()) else { return };

        let keyword_registry = world
            .get_resource::<teleology_core::KeywordRegistry>()
            .cloned()
            .unwrap_or_default();

        let popup_style = world
            .get_resource::<EventPopupStyle>()
            .cloned()
            .unwrap_or_default();

        let window_id = egui::Id::new("event_popup");
        let mut chosen: Option<usize> = None;

        let width = if popup_style.width > 0.0 { popup_style.width } else { 360.0 };
        let default_pos = match popup_style.anchor {
            teleology_core::PopupAnchor::Center => {
                let sr = ctx.screen_rect();
                egui::Pos2::new(sr.center().x - width / 2.0, sr.center().y - 100.0)
            }
            teleology_core::PopupAnchor::Fixed { x, y } => egui::Pos2::new(x, y),
        };

        let bg = {
            let c = popup_style.bg_color;
            egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
        };
        let title_color = {
            let c = popup_style.title_color;
            egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
        };
        let body_color = {
            let c = popup_style.body_color;
            egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
        };

        let frame = egui::Frame::default()
            .fill(bg)
            .inner_margin(egui::Margin::same(16.0))
            .rounding(6.0)
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(100)));

        let title_font_size = if popup_style.title_font_size > 0.0 { popup_style.title_font_size } else { 18.0 };
        let body_font_size = if popup_style.body_font_size > 0.0 { popup_style.body_font_size } else { 14.0 };

        egui::Window::new(&def.title)
            .id(window_id)
            .default_pos(default_pos)
            .default_width(width)
            .resizable(false)
            .collapsible(false)
            .frame(frame)
            .show(ctx, |ui| {
                // Title
                render_keyword_text(
                    ui,
                    &def.title,
                    egui::FontId::proportional(title_font_size),
                    title_color,
                    &keyword_registry,
                    &mut self.media_textures,
                    ctx,
                );
                ui.add_space(8.0);
                // Body
                render_keyword_text(
                    ui,
                    &def.body,
                    egui::FontId::proportional(body_font_size),
                    body_color,
                    &keyword_registry,
                    &mut self.media_textures,
                    ctx,
                );
                ui.add_space(12.0);
                // Choices
                for (i, choice) in def.choices.iter().enumerate() {
                    if ui.button(&choice.text).clicked() {
                        chosen = Some(i);
                    }
                }
            });

        if let Some(idx) = chosen {
            // Check if the chosen option chains to a next event
            let next = def.choices.get(idx).and_then(|c| c.next_event);
            let scope = inst.scope;
            let world = self.engine.world_mut();
            // Clear active event
            if let Some(mut active) = world.get_resource_mut::<ActiveEvent>() {
                active.current = None;
            }
            // Queue chained event if any
            if let Some(next_id) = next {
                if let Some(mut q) = world.get_resource_mut::<EventQueue>() {
                    q.push(EventInstance {
                        event_id: next_id,
                        scope,
                        payload: Vec::new(),
                    });
                }
            }
            // Pull next queued event
            teleology_core::pull_next_event(world);
        }
    }

    // -----------------------------------------------------------------------
    // Hierarchy tree panel (left panel) — with drag-and-drop reparenting
    // -----------------------------------------------------------------------

    pub(crate) fn ui_hierarchy_tree_panel(&mut self, ui: &mut egui::Ui) {
        panel_header(ui, "Hierarchy");

        // Buttons: expand all / collapse all / + Province / + Nation
        ui.horizontal(|ui| {
            if ui.small_button("Expand").clicked() {
                self.hierarchy_collapsed.clear();
            }
            if ui.small_button("Collapse").clicked() {
                let world = self.engine.world_mut();
                if let Some(b) = world.get_resource::<WorldBounds>() {
                    self.hierarchy_collapsed.insert(0); // Unowned
                    for i in 1..=b.nation_count {
                        self.hierarchy_collapsed.insert(i);
                    }
                }
            }
            if ui.small_button("+ Province").clicked() {
                self.push_undo();
                let world = self.engine.world_mut();
                if let Some(new_raw) = add_province_to_world(world) {
                    self.selected_province = Some(new_raw);
                }
            }
            if ui.small_button("+ Nation").clicked() {
                let world = self.engine.world_mut();
                if let Some(new_raw) = add_nation_to_world(world) {
                    self.selected_nation = Some(new_raw);
                }
            }
        });

        ui.separator();

        // Build ownership map
        let world = self.engine.world_mut();
        let bounds = world.get_resource::<WorldBounds>().cloned();
        let store = world.get_resource::<ProvinceStore>().cloned();

        let Some(bounds) = bounds else {
            ui.label("No world loaded.");
            return;
        };
        let Some(store) = store else { return };

        let mut owned_by: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut unowned: Vec<u32> = Vec::new();
        for (i, p) in store.items.iter().enumerate() {
            let prov_raw = (i + 1) as u32;
            if let Some(owner) = p.owner {
                owned_by.entry(owner.0.get()).or_default().push(prov_raw);
            } else {
                unowned.push(prov_raw);
            }
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            // Render nations
            for n_raw in 1..=bounds.nation_count {
                let collapsed = self.hierarchy_collapsed.contains(&n_raw);
                let n_color = nation_color(n_raw);
                let children = owned_by.get(&n_raw);
                let child_count = children.map(|v| v.len()).unwrap_or(0);

                // Nation row
                ui.horizontal(|ui| {
                    // Collapse triangle
                    let tri = if collapsed { "\u{25B6}" } else { "\u{25BC}" };
                    if ui.small_button(tri).clicked() {
                        if collapsed {
                            self.hierarchy_collapsed.remove(&n_raw);
                        } else {
                            self.hierarchy_collapsed.insert(n_raw);
                        }
                    }
                    // Color swatch
                    let (swatch_rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(swatch_rect, 2.0, n_color);
                    // Label
                    let label = format!("Nation #{} ({})", n_raw, child_count);
                    let resp = ui.selectable_label(self.selected_nation == Some(n_raw), &label);
                    if resp.clicked() {
                        self.selected_nation = Some(n_raw);
                    }

                    // Drop target for drag-and-drop
                    if self.hierarchy_drag_source.is_some() && resp.hovered() {
                        self.hierarchy_drop_target = Some(n_raw);
                        ui.painter().rect_stroke(
                            resp.rect.expand(1.0),
                            2.0,
                            egui::Stroke::new(2.0, egui::Color32::from_rgb(0x40, 0x80, 0xFF)),
                        );
                    }

                    // Context menu
                    resp.context_menu(|ui| {
                        if ui.button("Select Nation").clicked() {
                            self.selected_nation = Some(n_raw);
                            ui.close_menu();
                        }
                    });
                });

                // Children (provinces)
                if !collapsed {
                    if let Some(children) = children {
                        for &prov_raw in children {
                            self.render_province_row(ui, prov_raw, Some(n_raw));
                        }
                    }
                }
            }

            // Unowned group
            if !unowned.is_empty() {
                let collapsed = self.hierarchy_collapsed.contains(&0);

                ui.horizontal(|ui| {
                    let tri = if collapsed { "\u{25B6}" } else { "\u{25BC}" };
                    if ui.small_button(tri).clicked() {
                        if collapsed {
                            self.hierarchy_collapsed.remove(&0);
                        } else {
                            self.hierarchy_collapsed.insert(0);
                        }
                    }
                    let label = format!("Unowned ({})", unowned.len());
                    let resp = ui.selectable_label(false, &label);

                    // Drop target: unown
                    if self.hierarchy_drag_source.is_some() && resp.hovered() {
                        self.hierarchy_drop_target = Some(0);
                        ui.painter().rect_stroke(
                            resp.rect.expand(1.0),
                            2.0,
                            egui::Stroke::new(2.0, egui::Color32::from_rgb(0x40, 0x80, 0xFF)),
                        );
                    }
                });

                if !collapsed {
                    for &prov_raw in &unowned {
                        self.render_province_row(ui, prov_raw, None);
                    }
                }
            }

            // Handle drop
            if !ui.input(|i| i.pointer.primary_down()) {
                if let Some(source) = self.hierarchy_drag_source.take() {
                    if let Some(target_nation) = self.hierarchy_drop_target.take() {
                        // Reparent province
                        let new_owner = if target_nation == 0 {
                            None
                        } else {
                            NonZeroU32::new(target_nation).map(NationId)
                        };
                        let world = self.engine.world_mut();
                        if let Some(mut store) = world.get_resource_mut::<ProvinceStore>() {
                            let idx = (source - 1) as usize;
                            if let Some(p) = store.items.get_mut(idx) {
                                p.owner = new_owner;
                            }
                        }
                    }
                }
                self.hierarchy_drop_target = None;
            }
        });
    }

    fn render_province_row(&mut self, ui: &mut egui::Ui, prov_raw: u32, _parent_nation: Option<u32>) {
        ui.horizontal(|ui| {
            ui.add_space(20.0);

            // Draggable province label
            let label = format!("  P#{}", prov_raw);
            let selected = self.selected_province == Some(prov_raw);
            let resp = ui.add(
                egui::Label::new(
                    egui::RichText::new(&label)
                        .color(if selected { egui::Color32::WHITE } else { egui::Color32::LIGHT_GRAY })
                ).sense(egui::Sense::click_and_drag()),
            );

            if resp.clicked() {
                self.selected_province = Some(prov_raw);
            }

            // Start drag
            if resp.drag_started() {
                self.hierarchy_drag_source = Some(prov_raw);
            }

            // Visual feedback while dragging this item
            if self.hierarchy_drag_source == Some(prov_raw) && ui.input(|i| i.pointer.primary_down()) {
                if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                    ui.painter().text(
                        pos + egui::vec2(10.0, -10.0),
                        egui::Align2::LEFT_CENTER,
                        format!("P#{}", prov_raw),
                        egui::FontId::proportional(11.0),
                        egui::Color32::from_white_alpha(180),
                    );
                }
            }

            // Context menu
            province_context_menu(ui, &resp, prov_raw, &mut self.pending_context_action);
        });
    }

    // -----------------------------------------------------------------------
    // Nation list (right panel, Political view)
    // -----------------------------------------------------------------------

    pub(crate) fn ui_nation_list(&mut self, ui: &mut egui::Ui) {
        let world = self.engine.world_mut();
        let bounds = world.get_resource::<WorldBounds>().cloned();

        let Some(bounds) = bounds else {
            ui.label("No world.");
            return;
        };

        for n_raw in 1..=bounds.nation_count {
            let color = nation_color(n_raw);
            ui.horizontal(|ui| {
                let (swatch, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                ui.painter().rect_filled(swatch, 2.0, color);
                let label = format!("Nation #{}", n_raw);
                let resp = ui.selectable_label(self.selected_nation == Some(n_raw), &label);
                if resp.clicked() {
                    self.selected_nation = Some(n_raw);
                }
            });
        }

        ui.separator();
        if ui.button("+ Nation").clicked() {
            let world = self.engine.world_mut();
            if let Some(new_raw) = add_nation_to_world(world) {
                self.selected_nation = Some(new_raw);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Province context menu (free function, shared by tree rows)
// ---------------------------------------------------------------------------

fn province_context_menu(
    _ui: &mut egui::Ui,
    resp: &egui::Response,
    prov_raw: u32,
    pending: &mut Option<PendingContextAction>,
) {
    resp.context_menu(|ui| {
        if ui.button("Set Tag...").clicked() {
            *pending = Some(PendingContextAction::SetTag(ScopeKind::Province, prov_raw));
            ui.close_menu();
        }
        if ui.button("Add Modifier...").clicked() {
            *pending = Some(PendingContextAction::AddModifier(ScopeKind::Province, prov_raw));
            ui.close_menu();
        }
        if ui.button("Fire Event...").clicked() {
            *pending = Some(PendingContextAction::FireEventProvince(prov_raw));
            ui.close_menu();
        }
        if ui.button("Spawn Army").clicked() {
            *pending = Some(PendingContextAction::SpawnArmyProvince(prov_raw));
            ui.close_menu();
        }
    });
}

// ---------------------------------------------------------------------------
// Free functions: terrain flood fill helper
// ---------------------------------------------------------------------------

/// Find all provinces connected (via tile adjacency) to `start_prov` that share the same terrain.
fn find_connected_provinces_by_terrain(
    layout: &MapLayout,
    start_prov: u32,
    terrain: u8,
    store: &ProvinceStore,
) -> Vec<u32> {
    let mut result = vec![start_prov];
    let mut visited_provs: HashSet<u32> = HashSet::new();
    visited_provs.insert(start_prov);
    let mut queue: VecDeque<u32> = VecDeque::new();
    queue.push_back(start_prov);

    // Build adjacency from tile layout
    let mut adj: HashMap<u32, HashSet<u32>> = HashMap::new();
    for y in 0..layout.height {
        for x in 0..layout.width {
            let p = layout.get(x, y);
            if p == 0 { continue; }
            for (dx, dy) in [(-1i32, 0), (1, 0), (0, -1), (0, 1)] {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx >= 0 && ny >= 0 && (nx as u32) < layout.width && (ny as u32) < layout.height {
                    let np = layout.get(nx as u32, ny as u32);
                    if np != 0 && np != p {
                        adj.entry(p).or_default().insert(np);
                    }
                }
            }
        }
    }

    while let Some(prov) = queue.pop_front() {
        if let Some(neighbors) = adj.get(&prov) {
            for &n in neighbors {
                if !visited_provs.contains(&n) {
                    let idx = (n - 1) as usize;
                    if let Some(pp) = store.items.get(idx) {
                        if pp.terrain == terrain {
                            visited_provs.insert(n);
                            result.push(n);
                            queue.push_back(n);
                        }
                    }
                }
            }
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Hex geometry helpers
// ---------------------------------------------------------------------------

fn hex_center(q: u32, r: u32, hex_w: f32, hex_h: f32, origin: egui::Vec2) -> (f32, f32) {
    let offset = if r % 2 == 1 { 0.5 } else { 0.0 };
    let cx = origin.x + (q as f32 + offset) * hex_w + hex_w * 0.5;
    let cy = origin.y + r as f32 * hex_h * 0.75 + hex_h * 0.5;
    (cx, cy)
}

fn hex_vertices(center: egui::Pos2, size: f32) -> [egui::Pos2; 6] {
    let mut verts = [egui::Pos2::ZERO; 6];
    for i in 0..6 {
        let angle = std::f32::consts::PI / 3.0 * i as f32 - std::f32::consts::PI / 6.0;
        verts[i] = egui::Pos2::new(
            center.x + size * angle.cos(),
            center.y + size * angle.sin(),
        );
    }
    verts
}

fn screen_to_hex(
    pos: egui::Pos2,
    hex_w: f32,
    hex_h: f32,
    origin: egui::Vec2,
    width: u32,
    height: u32,
) -> Option<(u32, u32)> {
    let ry = ((pos.y - origin.y - hex_h * 0.5) / (hex_h * 0.75)).round() as i32;
    if ry < 0 || ry >= height as i32 {
        return None;
    }
    let offset = if ry % 2 == 1 { 0.5 } else { 0.0 };
    let rx = ((pos.x - origin.x - hex_w * 0.5) / hex_w - offset).round() as i32;
    if rx < 0 || rx >= width as i32 {
        return None;
    }
    Some((rx as u32, ry as u32))
}
