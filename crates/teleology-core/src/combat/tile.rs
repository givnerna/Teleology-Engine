//! Civ 5-style one-unit-per-tile combat: instant strength comparison,
//! HP-based attrition, zone of control, flanking bonuses.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::armies::{Army, ArmyStatus};
use crate::world::{ProvinceAdjacency, ProvinceId, ScopeId};

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for Civ 5-style tile combat.
#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct TileCombatConfig {
    /// Multiplier for the strength comparison formula.
    pub base_strength_scale: f64,
    /// Maximum HP per unit.
    pub hp_max: u16,
    /// Defense bonus per terrain type (e.g. forest=0.25 means +25%).
    pub terrain_defense: HashMap<u8, f64>,
    /// Bonus per adjacent friendly unit.
    pub flanking_bonus_per_adjacent: f64,
    /// Maximum flanking bonus cap.
    pub max_flanking_bonus: f64,
    /// Ranged units don't take retaliation damage.
    pub ranged_no_retaliation: bool,
    /// Moving through tiles adjacent to an enemy costs all movement.
    pub zone_of_control: bool,
    /// Base movement points per turn.
    pub movement_points_base: u16,
    /// Defense bonus for not moving last turn.
    pub fortification_bonus: f64,
    /// Combat bonuses per experience level.
    pub experience_levels: Vec<f64>,
}

impl Default for TileCombatConfig {
    fn default() -> Self {
        Self {
            base_strength_scale: 30.0,
            hp_max: 100,
            terrain_defense: HashMap::new(),
            flanking_bonus_per_adjacent: 0.10,
            max_flanking_bonus: 0.30,
            ranged_no_retaliation: true,
            zone_of_control: true,
            movement_points_base: 2,
            fortification_bonus: 0.25,
            experience_levels: vec![0.0, 0.10, 0.20, 0.30],
        }
    }
}

// ---------------------------------------------------------------------------
// Per-unit HP component (tile model specific)
// ---------------------------------------------------------------------------

/// HP component for tile-based combat. Attached to army entities.
#[derive(Component, Clone, Copy, Serialize, Deserialize)]
pub struct UnitHealth {
    pub hp: u16,
    pub max_hp: u16,
    pub xp: u32,
    /// True if the unit did not move last turn (fortification bonus).
    pub fortified: bool,
    /// Remaining movement points this turn.
    pub movement_remaining: u16,
}

impl UnitHealth {
    pub fn new(max_hp: u16, movement: u16) -> Self {
        Self {
            hp: max_hp,
            max_hp,
            xp: 0,
            fortified: false,
            movement_remaining: movement,
        }
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Enforce 1 unit per tile: if two friendly units end up on the same tile,
/// push the newer one to an adjacent empty tile.
pub fn system_tile_enforce_1upt(
    adjacency: Res<ProvinceAdjacency>,
    mut army_query: Query<(&mut Army, Entity)>,
) {
    let mut occupied: HashMap<u32, Entity> = HashMap::new();
    let mut to_move: Vec<(Entity, u32)> = Vec::new();

    for (army, entity) in army_query.iter() {
        let prov_raw = army.location.raw();
        if occupied.contains_key(&prov_raw) {
            // Conflict: need to move this unit.
            to_move.push((entity, prov_raw));
        } else {
            occupied.insert(prov_raw, entity);
        }
    }

    // Move displaced units to adjacent empty tiles.
    for (entity, from_raw) in to_move {
        let from_id = ProvinceId::from_raw(from_raw);
        let neighbors = adjacency.get(from_id);
        for &n_raw in neighbors {
            if !occupied.contains_key(&n_raw) {
                if let Ok((mut army, _)) = army_query.get_mut(entity) {
                    army.location = ProvinceId::from_raw(n_raw);
                    occupied.insert(n_raw, entity);
                    break;
                }
            }
        }
        // If no adjacent tile available, unit stays (stacked as exception).
    }
}

/// Tile movement: move units along adjacency, respecting movement points.
pub fn system_tile_movement(
    _config: Res<TileCombatConfig>,
    adjacency: Res<ProvinceAdjacency>,
    mut army_query: Query<(&mut Army, &mut ArmyStatus, &mut UnitHealth)>,
) {
    for (mut army, mut status, mut health) in army_query.iter_mut() {
        if let ArmyStatus::Marching { to } = *status {
            if health.movement_remaining > 0 {
                let neighbors = adjacency.get(army.location);
                if neighbors.contains(&to.raw()) {
                    army.location = to;
                    health.movement_remaining = health.movement_remaining.saturating_sub(1);
                    health.fortified = false;
                    *status = ArmyStatus::Idle;
                }
            }
        }
    }
}

/// Reset movement points at the start of each turn (primary tick).
pub fn system_tile_reset_movement(
    config: Res<TileCombatConfig>,
    mut health_query: Query<&mut UnitHealth>,
) {
    for mut health in health_query.iter_mut() {
        let prev_remaining = health.movement_remaining;
        health.movement_remaining = config.movement_points_base;
        // If unit didn't move last turn, it's fortified.
        if prev_remaining == config.movement_points_base {
            health.fortified = true;
        }
    }
}
