use eframe::egui;
use std::num::NonZeroU32;
use teleology_core::{
    ArmyComposition, ArmyRegistry, Army, CharacterGenConfig, EventBus,
    EventRegistry,
    GameDate, GameTime, MapKind, NationId, NationStore, NationTags,
    ProgressTrees, ProvinceId, ProvinceModifiers, ProvinceStore, ProvinceTags,
    TagId, TagRegistry, TagTypeId, TickUnit, TimeConfig, WorldBounds, WorldBuilder,
    UiCommand, UiCommandBuffer, UiPrefabRegistry,
    queue_event, spawn_army,
};
use crate::utils::*;
use crate::EditorApp;

impl EditorApp {
    pub fn render_game_ui(&mut self, ctx: &egui::Context) {
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

    pub fn ui_world(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(panel_frame())
            .show(ctx, |ui| {
                panel_header(ui, "Scene");
                ui.heading("World");
                ui.add_space(4.0);

                // --- Current world info ---
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

                let bounds = world.get_resource::<WorldBounds>().cloned();
                let map_info = world.get_resource::<MapKind>().map(|mk| match mk {
                    MapKind::Square(m) => format!("Square {}x{}", m.width, m.height),
                    MapKind::Hex(m) => format!("Hex {}x{}", m.width, m.height),
                    MapKind::Irregular(v) => format!("Irregular ({} polygons)", v.polygons.len()),
                });

                if let Some(b) = &bounds {
                    ui.label(format!("Provinces: {}  Nations: {}", b.province_count, b.nation_count));
                }
                if let Some(info) = &map_info {
                    ui.label(format!("Map: {}", info));
                }

                // --- Hierarchy overview: nations with province counts ---
                if let Some(b) = &bounds {
                    let nations = world.get_resource::<NationStore>();
                    let provinces = world.get_resource::<ProvinceStore>();
                    if let (Some(nations), Some(provinces)) = (nations, provinces) {
                        let nation_count = b.nation_count as usize;
                        let province_count = b.province_count as usize;

                        // Build ownership counts
                        let mut owned_count = vec![0u32; nation_count + 1];
                        let mut unowned = 0u32;
                        for (_i, p) in provinces.items.iter().enumerate().take(province_count) {
                            if let Some(owner) = p.owner {
                                let n = owner.0.get() as usize;
                                if n <= nation_count {
                                    owned_count[n] += 1;
                                } else {
                                    unowned += 1;
                                }
                            } else {
                                unowned += 1;
                            }
                        }

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(4.0);
                        ui.strong("Ownership Overview");
                        ui.add_space(4.0);

                        egui::Grid::new("ownership_overview")
                            .striped(true)
                            .min_col_width(60.0)
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("Nation").strong());
                                ui.label(egui::RichText::new("Provinces").strong());
                                ui.label(egui::RichText::new("Prestige").strong());
                                ui.label(egui::RichText::new("Treasury").strong());
                                ui.end_row();

                                for (i, n) in nations.items.iter().enumerate().take(nation_count) {
                                    let nation_raw = (i + 1) as u32;
                                    let count = owned_count[nation_raw as usize];
                                    ui.horizontal(|ui| {
                                        let (rect, _) = ui.allocate_exact_size(
                                            egui::Vec2::new(10.0, 10.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().rect_filled(rect, 2.0, nation_color(nation_raw));
                                        ui.add_space(4.0);
                                        ui.label(format!("Nation {}", nation_raw));
                                    });
                                    ui.label(format!("{}", count));
                                    ui.label(format!("{}", n.prestige));
                                    ui.label(format!("{}", n.treasury));
                                    ui.end_row();
                                }

                                if unowned > 0 {
                                    ui.label(egui::RichText::new("Unowned").italics().color(ui.visuals().weak_text_color()));
                                    ui.label(format!("{}", unowned));
                                    ui.label("\u{2014}");
                                    ui.label("\u{2014}");
                                    ui.end_row();
                                }
                            });
                    }
                }

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);

                // --- New World creation ---
                ui.strong("Create New World");
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Replace the current world with a fresh one. This cannot be undone.")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
                ui.add_space(6.0);

                egui::Grid::new("new_world_form")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Map type:");
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut self.new_world_map_type, 0, "Square");
                            ui.radio_value(&mut self.new_world_map_type, 1, "Hex");
                        });
                        ui.end_row();

                        ui.label("Width:");
                        ui.add(egui::DragValue::new(&mut self.new_world_map_width).range(2..=200).speed(1));
                        ui.end_row();

                        ui.label("Height:");
                        ui.add(egui::DragValue::new(&mut self.new_world_map_height).range(2..=200).speed(1));
                        ui.end_row();

                        ui.label("Provinces:");
                        ui.add(egui::DragValue::new(&mut self.new_world_province_count).range(1..=10000).speed(1));
                        ui.end_row();

                        ui.label("Nations:");
                        ui.add(egui::DragValue::new(&mut self.new_world_nation_count).range(1..=1000).speed(1));
                        ui.end_row();
                    });

                ui.add_space(6.0);
                let total_tiles = self.new_world_map_width * self.new_world_map_height;
                ui.label(
                    egui::RichText::new(format!(
                        "{} tiles, {} provinces auto-distributed, {} nations",
                        total_tiles, self.new_world_province_count, self.new_world_nation_count,
                    ))
                    .small()
                    .color(ui.visuals().weak_text_color()),
                );

                ui.add_space(4.0);
                if ui.button("Create World").clicked() {
                    let world = self.engine.world_mut();
                    let mut builder = WorldBuilder::new()
                        .provinces(self.new_world_province_count)
                        .nations(self.new_world_nation_count);
                    builder = if self.new_world_map_type == 0 {
                        builder.map_size(self.new_world_map_width, self.new_world_map_height)
                    } else {
                        builder.map_hex(self.new_world_map_width, self.new_world_map_height)
                    };
                    builder.build(world);
                    self.selected_province = None;
                    self.selected_nation = None;
                    self.hierarchy_collapsed.clear();
                    self.undo_history.clear();
                    self.redo_history.clear();
                }

                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new("Tip: Switch to Map Editor to paint provinces and assign ownership.")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            });
    }

    pub fn ui_settings(&mut self, ctx: &egui::Context) {
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
                    ui.label("Tags initialize on first use (right-click province/nation -> Set tag, or script API).");
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
                    ui.label("Modifiers initialize on first use (right-click province/nation -> Add modifier, or script API).");
                } else {
                    ui.label("Modifiers in use.");
                }
            });

            ui.separator();
            ui.collapsing("Pop-up Events", |ui| {
                if world.get_resource::<EventRegistry>().is_none() {
                    ui.label("Events initialize on first use (right-click -> Fire event, Events mode New event, or script API).");
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
                    ui.label("Armies initialize on first use (right-click province -> Spawn army, or script API).");
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
                        if ui.button("Save all...").clicked() {
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
                        if ui.button("Load all...").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("JSON", &["json"])
                                .pick_file()
                            {
                                if let Ok(reg) = UiPrefabRegistry::load_from_file(&path) {
                                    world.insert_resource(reg);
                                }
                            }
                        }
                        if ui.button("Load prefab...").clicked() {
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
