//! Generic progress trees (tech, culture, ideology, etc.).
//!
//! Trees are represented as a DAG of nodes with prerequisites.
//! Definitions are data-driven; progress state is stored per scope (nation/province/etc.).

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::num::NonZeroU32;

use crate::world::{NationId, ProvinceId, ScopeId};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TreeId(pub NonZeroU32);

impl TreeId {
    #[inline]
    pub fn raw(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

    /// Remove a tree by ID.
    pub fn remove_tree(&mut self, tree: TreeId) {
        self.trees.retain(|t| t.id != tree);
        self.node_index.clear();
    }

    /// Remove a node from a tree. Also clears prerequisite references to it in sibling nodes.
    pub fn remove_node(&mut self, tree: TreeId, node: NodeId) {
        if let Some(t) = self.trees.iter_mut().find(|t| t.id == tree) {
            t.nodes.retain(|n| n.id != node);
            // Clear dangling prerequisite references
            for n in &mut t.nodes {
                n.prerequisites.retain(|p| *p != node);
            }
        }
        self.node_index.clear();
    }
}

/// Per-tree progress state (unlocked + current progress values).
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct TreeProgressState {
    pub unlocked: HashSet<u32>,           // node_raw
    pub progress: HashMap<u32, f64>,      // node_raw -> accumulated
}

/// Generic progress state indexed by any ScopeId (ProvinceId, NationId, etc.).
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ScopedProgress<Id: ScopeId> {
    pub per_scope: Vec<HashMap<u32, TreeProgressState>>,
    #[serde(skip)]
    _marker: PhantomData<Id>,
}

impl<Id: ScopeId> ScopedProgress<Id> {
    pub fn new(count: usize) -> Self {
        Self {
            per_scope: vec![HashMap::new(); count],
            _marker: PhantomData,
        }
    }

    pub fn is_unlocked(&self, id: Id, tree: TreeId, node: NodeId) -> bool {
        self.per_scope
            .get(id.index())
            .and_then(|m| m.get(&tree.raw()))
            .map(|s| s.unlocked.contains(&node.raw()))
            .unwrap_or(false)
    }

    pub fn unlock(&mut self, id: Id, tree: TreeId, node: NodeId) {
        if let Some(m) = self.per_scope.get_mut(id.index()) {
            let state = m.entry(tree.raw()).or_default();
            state.unlocked.insert(node.raw());
            state.progress.remove(&node.raw());
        }
    }

    pub fn add_progress(&mut self, id: Id, tree: TreeId, node: NodeId, amount: f64) {
        if let Some(m) = self.per_scope.get_mut(id.index()) {
            let state = m.entry(tree.raw()).or_default();
            *state.progress.entry(node.raw()).or_insert(0.0) += amount.max(0.0);
        }
    }
}

/// Progress for nations. Type alias for backwards compatibility.
pub type NationProgress = ScopedProgress<NationId>;

/// Progress for provinces. Type alias for backwards compatibility.
pub type ProvinceProgress = ScopedProgress<ProvinceId>;

// Resource impls for concrete types (Bevy requires Resource on concrete, not generic).
impl Resource for ScopedProgress<NationId> {}
impl Resource for ScopedProgress<ProvinceId> {}

/// Progress state scoped to nations and provinces (backwards-compatible wrapper).
///
/// New code should use `ScopedProgress<NationId>` and `ScopedProgress<ProvinceId>` directly
/// as separate Bevy resources. This struct is provided for migration convenience.
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct ProgressState {
    pub per_nation: Vec<HashMap<u32, TreeProgressState>>,
    pub per_province: Vec<HashMap<u32, TreeProgressState>>,
}

impl ProgressState {
    pub fn new(nation_count: usize, province_count: usize) -> Self {
        Self {
            per_nation: vec![HashMap::new(); nation_count],
            per_province: vec![HashMap::new(); province_count],
        }
    }

    // --- Generic scope accessors ---

    /// Get the per-scope storage for a given ScopeId type.
    fn scope_storage<Id: ScopeId>(&self) -> &Vec<HashMap<u32, TreeProgressState>> {
        if std::any::TypeId::of::<Id>() == std::any::TypeId::of::<NationId>() {
            &self.per_nation
        } else {
            &self.per_province
        }
    }

    fn scope_storage_mut<Id: ScopeId>(&mut self) -> &mut Vec<HashMap<u32, TreeProgressState>> {
        if std::any::TypeId::of::<Id>() == std::any::TypeId::of::<NationId>() {
            &mut self.per_nation
        } else {
            &mut self.per_province
        }
    }

    /// Check if a node is unlocked for any scope id.
    pub fn is_unlocked<Id: ScopeId>(&self, id: Id, tree: TreeId, node: NodeId) -> bool {
        self.scope_storage::<Id>()
            .get(id.index())
            .and_then(|m| m.get(&tree.raw()))
            .map(|s| s.unlocked.contains(&node.raw()))
            .unwrap_or(false)
    }

    /// Unlock a node for any scope id.
    pub fn unlock<Id: ScopeId>(&mut self, id: Id, tree: TreeId, node: NodeId) {
        let storage = self.scope_storage_mut::<Id>();
        let state = storage[id.index()]
            .entry(tree.raw())
            .or_default();
        state.unlocked.insert(node.raw());
        state.progress.remove(&node.raw());
    }

    /// Add progress toward a node for any scope id.
    pub fn add_progress<Id: ScopeId>(&mut self, id: Id, tree: TreeId, node: NodeId, amount: f64) {
        let storage = self.scope_storage_mut::<Id>();
        let state = storage[id.index()]
            .entry(tree.raw())
            .or_default();
        *state.progress.entry(node.raw()).or_insert(0.0) += amount.max(0.0);
    }

    // --- Convenience methods for backwards compatibility ---

    pub fn is_unlocked_nation(&self, nation: NationId, tree: TreeId, node: NodeId) -> bool {
        self.is_unlocked::<NationId>(nation, tree, node)
    }

    pub fn unlock_nation(&mut self, nation: NationId, tree: TreeId, node: NodeId) {
        self.unlock::<NationId>(nation, tree, node);
    }

    pub fn add_progress_nation(&mut self, nation: NationId, tree: TreeId, node: NodeId, amount: f64) {
        self.add_progress::<NationId>(nation, tree, node, amount);
    }

    pub fn is_unlocked_province(&self, province: ProvinceId, tree: TreeId, node: NodeId) -> bool {
        self.is_unlocked::<ProvinceId>(province, tree, node)
    }

    pub fn unlock_province(&mut self, province: ProvinceId, tree: TreeId, node: NodeId) {
        self.unlock::<ProvinceId>(province, tree, node);
    }

    pub fn add_progress_province(&mut self, province: ProvinceId, tree: TreeId, node: NodeId, amount: f64) {
        self.add_progress::<ProvinceId>(province, tree, node, amount);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::ScopeId;

    #[test]
    fn progress_trees_add_tree() {
        let mut trees = ProgressTrees::new();
        let tid = trees.add_tree("Technology");
        assert_eq!(tid.raw(), 1);
        assert_eq!(trees.trees.len(), 1);
        assert_eq!(trees.trees[0].name, "Technology");
    }

    #[test]
    fn progress_trees_add_node() {
        let mut trees = ProgressTrees::new();
        let tid = trees.add_tree("Tech");
        let nid = trees.add_node(tid, "Agriculture", 100.0, vec![], vec![]);
        assert_eq!(nid.raw(), 1);
        assert_eq!(trees.trees[0].nodes.len(), 1);
        assert_eq!(trees.trees[0].nodes[0].cost, 100.0);
    }

    #[test]
    fn progress_trees_node_prerequisites() {
        let mut trees = ProgressTrees::new();
        let tid = trees.add_tree("Tech");
        let n1 = trees.add_node(tid, "Farming", 50.0, vec![], vec![]);
        let n2 = trees.add_node(tid, "Irrigation", 100.0, vec![n1], vec![]);
        let node = trees.get_node(tid, n2).unwrap();
        assert_eq!(node.prerequisites, vec![n1]);
    }

    #[test]
    fn progress_trees_get_node() {
        let mut trees = ProgressTrees::new();
        let tid = trees.add_tree("Tech");
        let nid = trees.add_node(tid, "Mining", 75.0, vec![], vec![1, 2, 3]);
        let node = trees.get_node(tid, nid).unwrap();
        assert_eq!(node.name, "Mining");
        assert_eq!(node.effects_payload, vec![1, 2, 3]);
    }

    #[test]
    fn scoped_progress_unlock() {
        let mut sp = ScopedProgress::<NationId>::new(3);
        let nid = NationId::from_raw(1);
        let tid = TreeId(NonZeroU32::new(1).unwrap());
        let node = NodeId(NonZeroU32::new(1).unwrap());

        assert!(!sp.is_unlocked(nid, tid, node));
        sp.unlock(nid, tid, node);
        assert!(sp.is_unlocked(nid, tid, node));
    }

    #[test]
    fn scoped_progress_add_progress() {
        let mut sp = ScopedProgress::<NationId>::new(3);
        let nid = NationId::from_raw(1);
        let tid = TreeId(NonZeroU32::new(1).unwrap());
        let node = NodeId(NonZeroU32::new(1).unwrap());

        sp.add_progress(nid, tid, node, 50.0);
        sp.add_progress(nid, tid, node, 30.0);

        let state = &sp.per_scope[nid.index()][&tid.raw()];
        let prog = state.progress[&node.raw()];
        assert!((prog - 80.0).abs() < f64::EPSILON);
    }

    #[test]
    fn scoped_progress_unlock_clears_progress() {
        let mut sp = ScopedProgress::<NationId>::new(3);
        let nid = NationId::from_raw(1);
        let tid = TreeId(NonZeroU32::new(1).unwrap());
        let node = NodeId(NonZeroU32::new(1).unwrap());

        sp.add_progress(nid, tid, node, 50.0);
        sp.unlock(nid, tid, node);

        let state = &sp.per_scope[nid.index()][&tid.raw()];
        assert!(state.progress.get(&node.raw()).is_none());
        assert!(state.unlocked.contains(&node.raw()));
    }

    #[test]
    fn progress_state_nation_and_province() {
        let mut ps = ProgressState::new(3, 5);
        let nid = NationId::from_raw(1);
        let pid = ProvinceId::from_raw(2);
        let tid = TreeId(NonZeroU32::new(1).unwrap());
        let node = NodeId(NonZeroU32::new(1).unwrap());

        ps.unlock_nation(nid, tid, node);
        assert!(ps.is_unlocked_nation(nid, tid, node));
        assert!(!ps.is_unlocked_province(pid, tid, node));

        ps.add_progress_province(pid, tid, node, 25.0);
        assert!(!ps.is_unlocked_province(pid, tid, node));
        ps.unlock_province(pid, tid, node);
        assert!(ps.is_unlocked_province(pid, tid, node));
    }

    #[test]
    fn progress_state_generic_api() {
        let mut ps = ProgressState::new(3, 5);
        let nid = NationId::from_raw(1);
        let tid = TreeId(NonZeroU32::new(1).unwrap());
        let node = NodeId(NonZeroU32::new(1).unwrap());

        ps.add_progress::<NationId>(nid, tid, node, 10.0);
        ps.unlock::<NationId>(nid, tid, node);
        assert!(ps.is_unlocked::<NationId>(nid, tid, node));
    }
}
