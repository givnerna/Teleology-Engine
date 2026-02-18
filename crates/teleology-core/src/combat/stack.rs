//! Paradox-style stack-based combat: armies stack in provinces,
//! abstract multi-day fire/shock phase resolution, morale routing.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::armies::{Army, ArmyId, ArmyStatus};
use crate::diplomacy::WarRegistry;
use crate::world::{GameDate, NationId, ProvinceAdjacency, ProvinceId, ProvinceStore, ScopeId};
use super::{BattleSide, CombatResult, CombatResultLog};

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for Paradox-style stack combat.
#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct StackCombatConfig {
    /// Max regiments in the frontline at once.
    pub combat_width: u16,
    /// Base damage per fire phase day.
    pub base_fire_damage: f64,
    /// Base damage per shock phase day.
    pub base_shock_damage: f64,
    /// Days in fire phase.
    pub fire_phase_days: u16,
    /// Days in shock phase.
    pub shock_phase_days: u16,
    /// Days of pursuit after one side routs.
    pub pursuit_days: u16,
    /// Starting morale for armies.
    pub morale_base: f64,
    /// Multiplier for morale damage.
    pub morale_damage_mult: f64,
    /// Dice range: 0 to N-1 (e.g. 10 for EU4-style 0-9).
    pub dice_range: u8,
    /// How much commander military skill affects damage.
    pub commander_pip_weight: f64,
    /// Terrain defense bonus per terrain type.
    pub terrain_defense: HashMap<u8, f64>,
    /// Troops per province before attrition kicks in.
    pub attrition_supply_limit: u16,
    /// Attrition damage per tick as fraction of strength.
    pub attrition_damage_rate: f64,
    /// Siege progress per tick (0.0 to 1.0 scale).
    pub siege_progress_per_tick: f64,
    /// Base garrison strength for sieges.
    pub siege_garrison_base: u16,
    /// Organization recovery per tick when idle.
    pub org_recovery_per_tick: u16,
    /// Movement cost in ticks per province.
    pub movement_ticks_per_province: u8,
}

impl Default for StackCombatConfig {
    fn default() -> Self {
        Self {
            combat_width: 27,
            base_fire_damage: 0.5,
            base_shock_damage: 0.8,
            fire_phase_days: 3,
            shock_phase_days: 3,
            pursuit_days: 2,
            morale_base: 3.0,
            morale_damage_mult: 1.0,
            dice_range: 10,
            commander_pip_weight: 0.1,
            terrain_defense: HashMap::new(),
            attrition_supply_limit: 25,
            attrition_damage_rate: 0.01,
            siege_progress_per_tick: 0.03,
            siege_garrison_base: 1000,
            org_recovery_per_tick: 5,
            movement_ticks_per_province: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Battle state
// ---------------------------------------------------------------------------

/// Phase of a stack-based battle.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StackBattlePhase {
    Fire,
    Shock,
    Pursuit,
    Resolved,
}

/// An active stack-based battle.
#[derive(Clone, Serialize, Deserialize)]
pub struct StackBattle {
    pub location: ProvinceId,
    pub attacker_armies: Vec<ArmyId>,
    pub defender_armies: Vec<ArmyId>,
    pub phase: StackBattlePhase,
    pub phase_day: u16,
    pub attacker_morale: f64,
    pub defender_morale: f64,
    pub attacker_casualties: u32,
    pub defender_casualties: u32,
}

/// Active stack battles.
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct ActiveStackBattles {
    pub battles: Vec<StackBattle>,
}

/// Siege state for a province.
#[derive(Clone, Serialize, Deserialize)]
pub struct SiegeState {
    pub province: ProvinceId,
    pub besieging_army: ArmyId,
    pub progress: f64,
    pub garrison: u16,
}

/// Active sieges.
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct ActiveSieges {
    pub sieges: Vec<SiegeState>,
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Detect battles: find provinces where hostile armies coexist.
pub fn system_stack_detect_battles(
    config: Res<StackCombatConfig>,
    war_reg: Res<WarRegistry>,
    army_query: Query<(&Army, Entity)>,
    mut battles: ResMut<ActiveStackBattles>,
) {
    // Group armies by province.
    let mut by_province: HashMap<u32, Vec<(ArmyId, NationId)>> = HashMap::new();
    for (army, _e) in army_query.iter() {
        by_province
            .entry(army.location.raw())
            .or_default()
            .push((army.id, army.owner));
    }

    // Check each province for hostile coexistence.
    for (_prov_raw, armies) in &by_province {
        for i in 0..armies.len() {
            for j in (i + 1)..armies.len() {
                let (aid_a, nation_a) = armies[i];
                let (aid_b, nation_b) = armies[j];

                if war_reg.are_at_war(nation_a, nation_b) {
                    // Check not already in a battle at this location.
                    let prov = ProvinceId::from_raw(*_prov_raw);
                    let already = battles.battles.iter().any(|b| b.location == prov);
                    if !already {
                        battles.battles.push(StackBattle {
                            location: prov,
                            attacker_armies: vec![aid_a],
                            defender_armies: vec![aid_b],
                            phase: StackBattlePhase::Fire,
                            phase_day: 0,
                            attacker_morale: config.morale_base,
                            defender_morale: config.morale_base,
                            attacker_casualties: 0,
                            defender_casualties: 0,
                        });
                    }
                }
            }
        }
    }
}

/// Resolve battle phases.
pub fn system_stack_resolve_battles(
    config: Res<StackCombatConfig>,
    date: Res<GameDate>,
    mut battles: ResMut<ActiveStackBattles>,
    mut army_query: Query<&mut Army>,
    mut result_log: ResMut<CombatResultLog>,
) {
    let mut resolved_indices = Vec::new();

    for (idx, battle) in battles.battles.iter_mut().enumerate() {
        match battle.phase {
            StackBattlePhase::Fire => {
                battle.phase_day += 1;
                // Apply fire damage.
                let att_dmg = (config.base_fire_damage * 100.0) as u32;
                let def_dmg = (config.base_fire_damage * 100.0) as u32;
                battle.defender_casualties += att_dmg;
                battle.attacker_casualties += def_dmg;
                battle.defender_morale -= config.base_fire_damage * config.morale_damage_mult;
                battle.attacker_morale -= config.base_fire_damage * config.morale_damage_mult * 0.8;

                if battle.phase_day >= config.fire_phase_days {
                    battle.phase = StackBattlePhase::Shock;
                    battle.phase_day = 0;
                }
            }
            StackBattlePhase::Shock => {
                battle.phase_day += 1;
                let att_dmg = (config.base_shock_damage * 100.0) as u32;
                let def_dmg = (config.base_shock_damage * 100.0) as u32;
                battle.defender_casualties += att_dmg;
                battle.attacker_casualties += def_dmg;
                battle.defender_morale -= config.base_shock_damage * config.morale_damage_mult;
                battle.attacker_morale -= config.base_shock_damage * config.morale_damage_mult * 0.8;

                if battle.phase_day >= config.shock_phase_days {
                    // Check morale.
                    if battle.attacker_morale <= 0.0 || battle.defender_morale <= 0.0 {
                        battle.phase = StackBattlePhase::Pursuit;
                        battle.phase_day = 0;
                    } else {
                        // Another round of fire/shock.
                        battle.phase = StackBattlePhase::Fire;
                        battle.phase_day = 0;
                    }
                }
            }
            StackBattlePhase::Pursuit => {
                battle.phase_day += 1;
                // Extra casualties on the loser.
                if battle.attacker_morale <= 0.0 {
                    battle.attacker_casualties += 50;
                } else {
                    battle.defender_casualties += 50;
                }
                if battle.phase_day >= config.pursuit_days {
                    battle.phase = StackBattlePhase::Resolved;
                }
            }
            StackBattlePhase::Resolved => {
                // Apply casualties to armies.
                let winner = if battle.attacker_morale > battle.defender_morale {
                    BattleSide::Attacker
                } else {
                    BattleSide::Defender
                };

                // Reduce army strength.
                for &aid in &battle.attacker_armies {
                    for mut army in army_query.iter_mut() {
                        if army.id == aid {
                            army.strength = army.strength.saturating_sub(
                                (battle.attacker_casualties / battle.attacker_armies.len() as u32) as u16,
                            );
                        }
                    }
                }
                for &aid in &battle.defender_armies {
                    for mut army in army_query.iter_mut() {
                        if army.id == aid {
                            army.strength = army.strength.saturating_sub(
                                (battle.defender_casualties / battle.defender_armies.len() as u32) as u16,
                            );
                        }
                    }
                }

                result_log.push(CombatResult {
                    location: battle.location,
                    date: *date,
                    winner,
                    attacker_casualties: battle.attacker_casualties,
                    defender_casualties: battle.defender_casualties,
                    attacker_nations: Vec::new(), // Would be populated from army owners
                    defender_nations: Vec::new(),
                });

                resolved_indices.push(idx);
            }
        }
    }

    // Remove resolved battles (reverse order to preserve indices).
    for &idx in resolved_indices.iter().rev() {
        battles.battles.swap_remove(idx);
    }
}

/// Siege progression.
pub fn system_stack_siege_tick(
    config: Res<StackCombatConfig>,
    mut sieges: ResMut<ActiveSieges>,
    _provinces: Res<ProvinceStore>,
) {
    for siege in &mut sieges.sieges {
        siege.progress += config.siege_progress_per_tick;
        if siege.progress >= 1.0 {
            siege.progress = 1.0;
            // Province captured — occupation handled elsewhere.
        }
    }
}

/// Army movement: armies with Marching status move one province per tick.
pub fn system_stack_army_movement(
    adjacency: Res<ProvinceAdjacency>,
    mut army_query: Query<(&mut Army, &mut ArmyStatus)>,
) {
    for (mut army, mut status) in army_query.iter_mut() {
        if let ArmyStatus::Marching { to } = *status {
            // Check if target is adjacent.
            let neighbors = adjacency.get(army.location);
            if neighbors.contains(&to.raw()) {
                army.location = to;
                *status = ArmyStatus::Idle;
            }
            // If not adjacent, would need pathfinding — for now, just stay.
        }
    }
}

/// Organization recovery for idle armies.
pub fn system_stack_org_recovery(
    config: Res<StackCombatConfig>,
    mut army_query: Query<(&mut Army, &ArmyStatus)>,
) {
    for (mut army, status) in army_query.iter_mut() {
        if *status == ArmyStatus::Idle {
            army.organization = army.organization.saturating_add(config.org_recovery_per_tick);
        }
    }
}
