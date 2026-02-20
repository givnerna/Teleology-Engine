//! Humankind-style deployment combat: armies deploy units onto surrounding
//! world map tiles, multi-round tactical battle, reinforcement from nearby.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::armies::ArmyId;
use crate::world::ProvinceId;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for Humankind-style deployment combat.
#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct DeploymentConfig {
    /// How many tiles from the battle center units can deploy to.
    pub deployment_radius: u8,
    /// Max rounds before the battle auto-resolves.
    pub max_rounds: u16,
    /// Max deployed units per tile.
    pub units_per_tile: u8,
    /// Nearby armies within this range (in provinces) can reinforce mid-battle.
    pub reinforcement_range: u8,
    /// Defense bonus per terrain type.
    pub terrain_defense: HashMap<u8, f64>,
    /// Bonus for holding higher elevation (if applicable).
    pub elevation_bonus: f64,
    /// Allow instant auto-resolve without playing out rounds.
    pub auto_resolve_available: bool,
    /// Can armies disengage after a minimum number of rounds?
    pub retreat_after_rounds: u16,
}

impl Default for DeploymentConfig {
    fn default() -> Self {
        Self {
            deployment_radius: 2,
            max_rounds: 3,
            units_per_tile: 1,
            reinforcement_range: 3,
            terrain_defense: HashMap::new(),
            elevation_bonus: 0.15,
            auto_resolve_available: true,
            retreat_after_rounds: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Battle state
// ---------------------------------------------------------------------------

/// A deployed unit on the battlefield (references an army's unit stack).
#[derive(Clone, Serialize, Deserialize)]
pub struct DeployedUnit {
    pub army_id: ArmyId,
    /// Index into the army's composition stacks.
    pub stack_index: usize,
    pub hp: u16,
    pub tile: ProvinceId,
}

/// An active deployment battle.
#[derive(Clone, Serialize, Deserialize)]
pub struct DeploymentBattle {
    pub center: ProvinceId,
    pub round: u16,
    pub attacker_units: Vec<DeployedUnit>,
    pub defender_units: Vec<DeployedUnit>,
    pub attacker_casualties: u32,
    pub defender_casualties: u32,
}

/// Active deployment battles.
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct ActiveDeploymentBattles {
    pub battles: Vec<DeploymentBattle>,
}

// ---------------------------------------------------------------------------
// Systems (stubs — full implementation requires game-specific tile adjacency)
// ---------------------------------------------------------------------------

/// Initiate deployment battle when hostile armies meet.
pub fn system_deployment_initiate(
    // Would detect collisions similar to stack model, then create DeploymentBattle
) {
    // Placeholder: battle initiation logic
}

/// Resolve one round of deployment combat.
pub fn system_deployment_resolve_round(
    config: Res<DeploymentConfig>,
    mut battles: ResMut<ActiveDeploymentBattles>,
) {
    for battle in &mut battles.battles {
        if battle.round < config.max_rounds {
            battle.round += 1;

            // Simplified: each attacker unit attacks adjacent defender units.
            // Full implementation would use adjacency for deployed tile positions.
            let att_damage = battle.attacker_units.len() as u32 * 10;
            let def_damage = battle.defender_units.len() as u32 * 10;
            battle.defender_casualties += att_damage;
            battle.attacker_casualties += def_damage;

            // Remove destroyed units.
            let att_count = battle.attacker_units.len().max(1) as u32;
            let def_count = battle.defender_units.len().max(1) as u32;
            for unit in &mut battle.attacker_units {
                unit.hp = unit.hp.saturating_sub((def_damage / att_count) as u16);
            }
            for unit in &mut battle.defender_units {
                unit.hp = unit.hp.saturating_sub((att_damage / def_count) as u16);
            }
            battle.attacker_units.retain(|u| u.hp > 0);
            battle.defender_units.retain(|u| u.hp > 0);
        }
    }

    // Remove battles where one side is eliminated or max rounds reached.
    battles.battles.retain(|b| {
        !b.attacker_units.is_empty()
            && !b.defender_units.is_empty()
            && b.round < config.max_rounds
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::ScopeId;

    #[test]
    fn deployment_config_defaults() {
        let config = DeploymentConfig::default();
        assert_eq!(config.deployment_radius, 2);
        assert_eq!(config.max_rounds, 3);
        assert_eq!(config.units_per_tile, 1);
        assert_eq!(config.reinforcement_range, 3);
        assert!(config.auto_resolve_available);
        assert_eq!(config.retreat_after_rounds, 1);
    }

    #[test]
    fn deployment_battle_initial_state() {
        let battle = DeploymentBattle {
            center: ProvinceId::from_raw(5),
            round: 0,
            attacker_units: vec![DeployedUnit {
                army_id: ArmyId(std::num::NonZeroU32::new(1).unwrap()),
                stack_index: 0,
                hp: 100,
                tile: ProvinceId::from_raw(5),
            }],
            defender_units: vec![DeployedUnit {
                army_id: ArmyId(std::num::NonZeroU32::new(2).unwrap()),
                stack_index: 0,
                hp: 100,
                tile: ProvinceId::from_raw(6),
            }],
            attacker_casualties: 0,
            defender_casualties: 0,
        };
        assert_eq!(battle.round, 0);
        assert_eq!(battle.attacker_units.len(), 1);
        assert_eq!(battle.defender_units.len(), 1);
    }

    #[test]
    fn active_deployment_battles_default() {
        let battles = ActiveDeploymentBattles::default();
        assert!(battles.battles.is_empty());
    }

    #[test]
    fn deployed_unit_hp() {
        let mut unit = DeployedUnit {
            army_id: ArmyId(std::num::NonZeroU32::new(1).unwrap()),
            stack_index: 0,
            hp: 100,
            tile: ProvinceId::from_raw(1),
        };
        unit.hp = unit.hp.saturating_sub(30);
        assert_eq!(unit.hp, 70);
    }
}
