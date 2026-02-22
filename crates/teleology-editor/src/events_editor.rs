use eframe::egui;
use std::num::NonZeroU32;
use teleology_core::{
    ActiveEvent, EventPopupStyle,
    EventQueue, EventRegistry, EventTemplate, register_builtin_templates,
};
use crate::utils::panel_frame;
use crate::utils::panel_header;
use crate::{EditorApp, PendingContextAction};

impl EditorApp {
    pub fn ui_events_editor(&mut self, ctx: &egui::Context) {
        self.process_pending_context_action();
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
                        let resp = ui.selectable_label(selected, format!("Event {}", raw));
                        if resp.clicked() {
                            self.event_selected_raw = Some(raw);
                        }
                        resp.context_menu(|ui| {
                            ui.label(format!("Event {}", raw));
                            ui.separator();
                            if ui.button("Duplicate").clicked() {
                                self.pending_context_action = Some(PendingContextAction::DuplicateEvent(raw));
                                ui.close_menu();
                            }
                            if ui.button("Clear connections").clicked() {
                                self.pending_context_action = Some(PendingContextAction::ClearEventConnections(raw));
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("Delete").clicked() {
                                self.pending_context_action = Some(PendingContextAction::DeleteEvent(raw));
                                ui.close_menu();
                            }
                        });
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
                        + egui::Vec2::new(node_size.x, 56.0 + choice_idx as f32 * 18.0);
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
                resp.context_menu(|ui| {
                    ui.label(format!("Event {}", raw));
                    ui.separator();
                    if ui.button("Duplicate").clicked() {
                        self.pending_context_action = Some(PendingContextAction::DuplicateEvent(raw));
                        ui.close_menu();
                    }
                    if ui.button("Clear connections").clicked() {
                        self.pending_context_action = Some(PendingContextAction::ClearEventConnections(raw));
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Delete").clicked() {
                        self.pending_context_action = Some(PendingContextAction::DeleteEvent(raw));
                        ui.close_menu();
                    }
                });

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
}
