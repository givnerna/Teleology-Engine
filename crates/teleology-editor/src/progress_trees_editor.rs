use eframe::egui;
use std::num::NonZeroU32;
use teleology_core::{
    ProgressState, ProgressTrees, WorldBounds,
};
use crate::utils::{panel_frame, panel_header};
use crate::{EditorApp, PendingContextAction};

impl EditorApp {
    pub(crate) fn ui_progress_trees_editor(&mut self, ctx: &egui::Context) {
        self.process_pending_context_action();
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
                    // Ensure resources exist before creating tree
                    if world.get_resource::<ProgressTrees>().is_none() {
                        world.insert_resource(ProgressTrees::new());
                        if let Some(b) = world.get_resource::<WorldBounds>().cloned() {
                            world.insert_resource(ProgressState::new(
                                b.nation_count as usize,
                                b.province_count as usize,
                            ));
                        }
                    }
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
                        let resp = ui.selectable_label(selected, format!("{} ({})", name, raw));
                        if resp.clicked() {
                            self.progress_selected_tree_raw = Some(raw);
                            self.progress_selected_node_raw = None;
                        }
                        resp.context_menu(|ui| {
                            ui.label(format!("Tree: {}", name));
                            ui.separator();
                            if ui.button("Delete tree").clicked() {
                                self.pending_context_action = Some(PendingContextAction::DeleteTree(raw));
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
                resp.context_menu(|ui| {
                    ui.label(format!("Node {}", raw));
                    ui.separator();
                    if ui.button("Clear prerequisites").clicked() {
                        self.pending_context_action = Some(PendingContextAction::ClearNodePrereqs(tree_raw, raw));
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Delete node").clicked() {
                        self.pending_context_action = Some(PendingContextAction::DeleteNode(tree_raw, raw));
                        ui.close_menu();
                    }
                });

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
}
