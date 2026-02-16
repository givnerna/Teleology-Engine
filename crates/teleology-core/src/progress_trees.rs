//! Generic progress trees (tech, culture, ideology, etc.).
//!
//! Trees are represented as a DAG of nodes with prerequisites.
//! Definitions are data-driven; progress state is stored per scope (nation/province/etc.).

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;

use crate::world::{NationId, ProvinceId};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TreeId(pub NonZeroU32);

impl TreeId {
    #[inline]
    pub fn raw(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub NonZeroU32);

impl NodeId {
    #[inline]
    pub fn raw(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ProgressNode {
    pub id: NodeId,
    pub name: String,
    /// Total progress required to unlock.
    pub cost: f64,
    pub prerequisites: Vec<NodeId>,
    /// Game-defined opaque payload for unlock effects.
    pub effects_payload: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ProgressTreeDefinition {
    pub id: TreeId,
    pub name: String,
    pub nodes: Vec<ProgressNode>,
}

/// Registry of all progress trees (definitions).
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct ProgressTrees {
    pub trees: Vec<ProgressTreeDefinition>,
    pub next_tree_raw: u32,
    pub next_node_raw: u32,

    #[serde(skip)]
    node_index: HashMap<(u32, u32), usize>, // (tree_raw, node_raw) -> index
}

impl ProgressTrees {
    pub fn new() -> Self {
        Self {
            trees: Vec::new(),
            next_tree_raw: 1,
            next_node_raw: 1,
            node_index: HashMap::new(),
        }
    }

    fn ensure_index(&mut self) {
        if !self.node_index.is_empty() {
            return;
        }
        self.rebuild_index();
    }

    pub fn rebuild_index(&mut self) {
        self.node_index.clear();
        for t in &self.trees {
            self.next_tree_raw = self.next_tree_raw.max(t.id.raw().saturating_add(1));
            for (i, n) in t.nodes.iter().enumerate() {
                self.next_node_raw = self.next_node_raw.max(n.id.raw().saturating_add(1));
                self.node_index.insert((t.id.raw(), n.id.raw()), i);
            }
        }
        self.next_tree_raw = self.next_tree_raw.max(1);
        self.next_node_raw = self.next_node_raw.max(1);
    }

    pub fn add_tree(&mut self, name: impl Into<String>) -> TreeId {
        let raw = self.next_tree_raw.max(1);
        self.next_tree_raw = raw.saturating_add(1);
        let id = TreeId(NonZeroU32::new(raw).unwrap());
        self.trees.push(ProgressTreeDefinition {
            id,
            name: name.into(),
            nodes: Vec::new(),
        });
        self.node_index.clear();
        id
    }

    pub fn add_node(
        &mut self,
        tree: TreeId,
        name: impl Into<String>,
        cost: f64,
        prerequisites: Vec<NodeId>,
        effects_payload: Vec<u8>,
    ) -> NodeId {
        let raw = self.next_node_raw.max(1);
        self.next_node_raw = raw.saturating_add(1);
        let id = NodeId(NonZeroU32::new(raw).unwrap());
        if let Some(t) = self.trees.iter_mut().find(|t| t.id == tree) {
            t.nodes.push(ProgressNode {
                id,
                name: name.into(),
                cost,
                prerequisites,
                effects_payload,
            });
        }
        self.node_index.clear();
        id
    }

    pub fn get_node(&mut self, tree: TreeId, node: NodeId) -> Option<&ProgressNode> {
        self.ensure_index();
        let t = self.trees.iter().find(|t| t.id == tree)?;
        let idx = *self.node_index.get(&(tree.raw(), node.raw()))?;
        t.nodes.get(idx)
    }
}

/// Per-tree progress state (unlocked + current progress values).
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct TreeProgressState {
    pub unlocked: HashSet<u32>,           // node_raw
    pub progress: HashMap<u32, f64>,      // node_raw -> accumulated
}

/// Progress state scoped to nations and provinces (expandable).
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct ProgressState {
    pub per_nation: Vec<HashMap<u32, TreeProgressState>>,   // nation_index -> (tree_raw -> state)
    pub per_province: Vec<HashMap<u32, TreeProgressState>>, // province_index -> (tree_raw -> state)
}

impl ProgressState {
    pub fn new(nation_count: usize, province_count: usize) -> Self {
        Self {
            per_nation: vec![HashMap::new(); nation_count],
            per_province: vec![HashMap::new(); province_count],
        }
    }

    fn nation_state_mut(&mut self, nation: NationId, tree: TreeId) -> &mut TreeProgressState {
        self.per_nation[nation.index()]
            .entry(tree.raw())
            .or_default()
    }

    fn province_state_mut(&mut self, province: ProvinceId, tree: TreeId) -> &mut TreeProgressState {
        self.per_province[province.index()]
            .entry(tree.raw())
            .or_default()
    }

    pub fn is_unlocked_nation(&self, nation: NationId, tree: TreeId, node: NodeId) -> bool {
        self.per_nation
            .get(nation.index())
            .and_then(|m| m.get(&tree.raw()))
            .map(|s| s.unlocked.contains(&node.raw()))
            .unwrap_or(false)
    }

    pub fn unlock_nation(&mut self, nation: NationId, tree: TreeId, node: NodeId) {
        self.nation_state_mut(nation, tree).unlocked.insert(node.raw());
        self.nation_state_mut(nation, tree).progress.remove(&node.raw());
    }

    pub fn add_progress_nation(&mut self, nation: NationId, tree: TreeId, node: NodeId, amount: f64) {
        let s = self.nation_state_mut(nation, tree);
        *s.progress.entry(node.raw()).or_insert(0.0) += amount.max(0.0);
    }

    pub fn is_unlocked_province(&self, province: ProvinceId, tree: TreeId, node: NodeId) -> bool {
        self.per_province
            .get(province.index())
            .and_then(|m| m.get(&tree.raw()))
            .map(|s| s.unlocked.contains(&node.raw()))
            .unwrap_or(false)
    }

    pub fn unlock_province(&mut self, province: ProvinceId, tree: TreeId, node: NodeId) {
        self.province_state_mut(province, tree).unlocked.insert(node.raw());
        self.province_state_mut(province, tree).progress.remove(&node.raw());
    }

    pub fn add_progress_province(&mut self, province: ProvinceId, tree: TreeId, node: NodeId, amount: f64) {
        let s = self.province_state_mut(province, tree);
        *s.progress.entry(node.raw()).or_insert(0.0) += amount.max(0.0);
    }
}

