use eframe::egui;
use std::num::NonZeroU32;
use teleology_core::{
    add_province_to_world, pull_next_event, queue_event,
    ActiveEvent, EventPopupStyle, EventRegistry,
    GameDate, GameTime, KeywordRegistry, MapKind, NationId, NationStore,
    PopupAnchor, ProvinceId, ProvinceStore, TickUnit, TimeConfig, WorldBounds,
};
use crate::utils::*;
use crate::{EditorApp, MapEditorPaintMode, PendingContextAction};

impl EditorApp {
    pub(crate) fn ui_map_editor(&mut self, ctx: &egui::Context) {
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

                let (_map_bounds, tile_hit, click_only_hit, erase_hit, hover_info) = {
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
                                // --- Click-only hit (for ownership painting, avoids painting multiple provinces on drag) ---
                                let click_only = response.clicked()
                                    .then(|| response.interact_pointer_pos())
                                    .flatten()
                                    .and_then(|pos| to_tile(pos).map(|(rx, ry)| TileHit::Square(rx, ry)));
                                // --- Secondary click/drag → erase ---
                                let secondary_hit = (response.secondary_clicked() || response.dragged_by(egui::PointerButton::Secondary))
                                    .then(|| response.interact_pointer_pos())
                                    .flatten()
                                    .and_then(|pos| to_tile(pos).map(|(rx, ry)| TileHit::Square(rx, ry)));
                                (Some((map.width, map.height)), primary_hit, click_only, secondary_hit, hover)
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
                                let click_only = response.clicked()
                                    .then(|| response.interact_pointer_pos())
                                    .flatten()
                                    .and_then(|pos| to_hex(pos).map(|(q, r)| TileHit::Hex(q, r)));
                                let secondary_hit = (response.secondary_clicked() || response.dragged_by(egui::PointerButton::Secondary))
                                    .then(|| response.interact_pointer_pos())
                                    .flatten()
                                    .and_then(|pos| to_hex(pos).map(|(q, r)| TileHit::Hex(q, r)));
                                (Some((w, h)), primary_hit, click_only, secondary_hit, hover)
                            }
                            MapKind::Irregular(vec) => {
                                ui.label(format!(
                                    "Irregular map: {} provinces. Use Assign (below) to set owners.",
                                    vec.polygons.len()
                                ));
                                (None, None, None, None, None)
                            }
                        }
                    } else {
                        ui.label("No map. Create world with map_size(), map_hex(), or load a map.");
                        (None, None, None, None, None)
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
                // In ownership mode use click-only to avoid painting multiple provinces on drag
                let effective_hit = if paint_ownership { click_only_hit } else { tile_hit };
                if let Some(hit) = effective_hit {
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
                    let kw_reg = world
                        .get_resource::<KeywordRegistry>()
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

                            // Body (with keyword highlighting)
                            let [dr, dg, db, da] = style.body_color;
                            let body_size = if style.body_font_size > 0.0 { style.body_font_size } else { 14.0 };
                            let body_color = egui::Color32::from_rgba_unmultiplied(dr, dg, db, da);
                            let body_font = egui::FontId::proportional(body_size);
                            render_keyword_text(
                                ui,
                                &def.body,
                                body_font,
                                body_color,
                                &kw_reg,
                                &mut self.project_thumbnails,
                                ctx,
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
                                    if ui.button("Set tag\u{2026}").clicked() {
                                        self.pending_context_action = Some(PendingContextAction::SetTagNation(id));
                                        ui.close_menu();
                                    }
                                    if ui.button("Add modifier\u{2026}").clicked() {
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
                                        if owner_raw == 0 { "\u{2014}".to_string() } else { owner_raw.to_string() },
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
                                    if ui.button("Set tag\u{2026}").clicked() {
                                        self.pending_context_action = Some(PendingContextAction::SetTagProvince(id));
                                        ui.close_menu();
                                    }
                                    if ui.button("Add modifier\u{2026}").clicked() {
                                        self.pending_context_action = Some(PendingContextAction::AddModifierProvince(id));
                                        ui.close_menu();
                                    }
                                    if ui.button("Fire event\u{2026}").clicked() {
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
                                    if ui.button("Set tag\u{2026}").clicked() {
                                        self.pending_context_action = Some(PendingContextAction::SetTagNation(id));
                                        ui.close_menu();
                                    }
                                    if ui.button("Add modifier\u{2026}").clicked() {
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
}
