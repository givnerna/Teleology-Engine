//! Combat system with four selectable models:
//!
//! - **StackBased** (Paradox): armies stack in provinces, abstract multi-day resolution
//! - **OneUnitPerTile** (Civ 5): one unit per tile, instant strength comparison
//! - **Deployment** (Humankind): armies deploy units to surrounding tiles, multi-round
//! - **TacticalGrid** (Total War): separate battle grid with formations and flanking
//!
//! Pick a model via `CombatModel` resource. The engine registers only the relevant systems.

pub mod stack;
pub mod tile;
pub mod deployment;
pub mod tactical;

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::num::NonZeroU32;

use crate::world::{GameDate, NationId, ProvinceId};

// ---------------------------------------------------------------------------
// Shared types (all models use these)
// ---------------------------------------------------------------------------

/// Stable id for a unit type definition.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UnitTypeId(pub NonZeroU32);

impl UnitTypeId {
    #[inline]
    pub fn raw(self) -> u32 { self.0.get() }
}

/// Unit type category.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UnitCategory {
    Infantry,
    Cavalry,
    Ranged,
    Siege,
    Naval,
    Custom(u32),
}

impl Default for UnitCategory {
    fn default() -> Self { UnitCategory::Infantry }
}

/// Definition of a unit type (infantry, cavalry, etc.).
#[derive(Clone, Serialize, Deserialize)]
pub struct UnitTypeDef {
    pub id: UnitTypeId,
    pub name: String,
    pub category: UnitCategory,
    pub base_strength: u16,
    pub base_morale: u16,
    pub movement_speed: u8,
    /// Custom stats for game-specific mechanics.
    pub custom_stats: HashMap<String, f64>,
}

/// Registry of all unit type definitions.
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct UnitTypeRegistry {
    pub types: Vec<UnitTypeDef>,
    next_raw: u32,
}

impl UnitTypeRegistry {
    pub fn new() -> Self {
        Self { types: Vec::new(), next_raw: 1 }
    }

    pub fn register(
        &mut self,
        name: String,
        category: UnitCategory,
        base_strength: u16,
        base_morale: u16,
        movement_speed: u8,
    ) -> UnitTypeId {
        let id = UnitTypeId(NonZeroU32::new(self.next_raw).unwrap());
        self.next_raw += 1;
        self.types.push(UnitTypeDef {
            id,
            name,
            category,
            base_strength,
            base_morale,
            movement_speed,
            custom_stats: HashMap::new(),
        });
        id
    }

    pub fn get(&self, id: UnitTypeId) -> Option<&UnitTypeDef> {
        self.types.iter().find(|t| t.id == id)
    }
}

// ---------------------------------------------------------------------------
// Combat result (logged for war score, history)
// ---------------------------------------------------------------------------

/// Which side won.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BattleSide {
    Attacker,
    Defender,
    Draw,
}

/// Result of a completed battle.
#[derive(Clone, Serialize, Deserialize)]
pub struct CombatResult {
    pub location: ProvinceId,
    pub date: GameDate,
    pub winner: BattleSide,
    pub attacker_casualties: u32,
    pub defender_casualties: u32,
    pub attacker_nations: Vec<NationId>,
    pub defender_nations: Vec<NationId>,
}

/// Log of all combat results (for war score calculation and history).
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct CombatResultLog {
    pub results: Vec<CombatResult>,
}

impl CombatResultLog {
    pub fn new() -> Self {
        Self { results: Vec::new() }
    }

    pub fn push(&mut self, result: CombatResult) {
        self.results.push(result);
    }
}

// ---------------------------------------------------------------------------
// Combat model selection
// ---------------------------------------------------------------------------

/// The active combat model. Determines which systems run.
/// Set once at world setup; changing mid-game is not recommended.
#[derive(Resource, Clone, Serialize, Deserialize)]
pub enum CombatModel {
    /// Paradox-style: armies stack in provinces, abstract multi-day fire/shock phases.
    StackBased(stack::StackCombatConfig),
    /// Civ 5-style: one unit per tile, instant strength comparison.
    OneUnitPerTile(tile::TileCombatConfig),
    /// Humankind-style: armies deploy units to surrounding tiles, multi-round battles.
    Deployment(deployment::DeploymentConfig),
    /// Total War-style: separate tactical grid with formations and flanking.
    TacticalGrid(tactical::TacticalGridConfig),
}

impl Default for CombatModel {
    fn default() -> Self {
        CombatModel::StackBased(stack::StackCombatConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::ScopeId;

    #[test]
    fn unit_type_registry() {
        let mut reg = UnitTypeRegistry::new();
        let inf = reg.register("Infantry".into(), UnitCategory::Infantry, 10, 100, 1);
        let cav = reg.register("Cavalry".into(), UnitCategory::Cavalry, 15, 80, 3);
        assert_eq!(reg.types.len(), 2);
        assert_eq!(reg.get(inf).unwrap().base_strength, 10);
        assert_eq!(reg.get(cav).unwrap().movement_speed, 3);
    }

    #[test]
    fn combat_result_log() {
        let mut log = CombatResultLog::new();
        log.push(CombatResult {
            location: ProvinceId::from_raw(1),
            date: GameDate::new(1444, 6, 15),
            winner: BattleSide::Attacker,
            attacker_casualties: 500,
            defender_casualties: 1200,
            attacker_nations: vec![NationId::from_raw(1)],
            defender_nations: vec![NationId::from_raw(2)],
        });
        assert_eq!(log.results.len(), 1);
    }
}
