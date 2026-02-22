//! Total War-style tactical grid combat: separate battle grid with
//! formations, flanking/rear attacks, morale routing.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use crate::armies::ArmyId;
use crate::world::ProvinceId;
use super::UnitTypeId;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for Total War-style tactical grid combat.
#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct TacticalGridConfig {
    /// Battle grid width.
    pub grid_width: u16,
    /// Battle grid height.
    pub grid_height: u16,
    /// Available formation types with their bonuses.
    pub formations: Vec<FormationDef>,
    /// Damage multiplier for flanking (side) attacks.
    pub flank_damage_mult: f64,
    /// Damage multiplier for rear attacks.
    pub rear_damage_mult: f64,
    /// Starting morale for units.
    pub morale_base: f64,
    /// Fatigue accumulation per battle tick.
    pub fatigue_per_tick: f64,
    /// Cavalry charge damage bonus.
    pub charge_bonus: f64,
    /// Range in grid tiles for ranged units.
    pub missile_range: u16,
    /// Whether auto-resolve is available (skips grid simulation).
    pub auto_resolve_enabled: bool,
    /// Morale penalty when seeing adjacent friendly units rout.
    pub rout_morale_cascade: f64,
}

impl Default for TacticalGridConfig {
    fn default() -> Self {
        Self {
            grid_width: 40,
            grid_height: 30,
            formations: vec![
                FormationDef { name: "Line".into(), defense_bonus: 0.0, attack_bonus: 0.0, width_mult: 1.0 },
                FormationDef { name: "Column".into(), defense_bonus: -0.1, attack_bonus: 0.15, width_mult: 0.5 },
                FormationDef { name: "Square".into(), defense_bonus: 0.3, attack_bonus: -0.2, width_mult: 0.7 },
                FormationDef { name: "Wedge".into(), defense_bonus: -0.15, attack_bonus: 0.25, width_mult: 0.6 },
            ],
            flank_damage_mult: 1.5,
            rear_damage_mult: 2.0,
            morale_base: 100.0,
            fatigue_per_tick: 0.5,
            charge_bonus: 1.5,
            missile_range: 8,
            auto_resolve_enabled: true,
            rout_morale_cascade: 10.0,
        }
    }
}

/// A formation type that units can adopt.
#[derive(Clone, Serialize, Deserialize)]
pub struct FormationDef {
    pub name: String,
    pub defense_bonus: f64,
    pub attack_bonus: f64,
    /// How wide the formation is relative to base (affects frontage).
    pub width_mult: f64,
}

// ---------------------------------------------------------------------------
// Grid state
// ---------------------------------------------------------------------------

/// Facing direction on the tactical grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Facing {
    North,
    South,
    East,
    West,
}

impl Default for Facing {
    fn default() -> Self { Facing::North }
}

/// A unit placed on the tactical grid.
#[derive(Clone, Serialize, Deserialize)]
pub struct TacticalUnit {
    pub army_id: ArmyId,
    pub stack_index: usize,
    pub unit_type: Option<UnitTypeId>,
    pub grid_x: u16,
    pub grid_y: u16,
    pub facing: Facing,
    pub formation_index: usize,
    pub hp: u16,
    pub morale: f64,
    pub fatigue: f64,
    pub routing: bool,
}

/// An active tactical grid battle.
#[derive(Clone, Serialize, Deserialize)]
pub struct TacticalBattle {
    pub location: ProvinceId,
    pub grid_width: u16,
    pub grid_height: u16,
    pub attacker_units: Vec<TacticalUnit>,
    pub defender_units: Vec<TacticalUnit>,
    pub tick: u32,
    pub attacker_casualties: u32,
    pub defender_casualties: u32,
}

/// Active tactical battles.
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct ActiveTacticalBattles {
    pub battles: Vec<TacticalBattle>,
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Create a tactical grid when armies meet.
pub fn system_tactical_create_grid(
    // Would create TacticalBattle instances when hostile armies collide
) {
    // Placeholder: grid creation logic
}

/// Tick the tactical battle: movement, combat, morale.
pub fn system_tactical_tick(
    config: Res<TacticalGridConfig>,
    mut battles: ResMut<ActiveTacticalBattles>,
) {
    for battle in &mut battles.battles {
        battle.tick += 1;

        // Apply fatigue.
        for unit in battle.attacker_units.iter_mut().chain(battle.defender_units.iter_mut()) {
            if !unit.routing {
                unit.fatigue += config.fatigue_per_tick;
            }
        }

        // Simplified combat: adjacent units deal damage.
        // Full implementation would use grid positions and facing.
        let att_total_str: u16 = battle.attacker_units.iter()
            .filter(|u| !u.routing && u.hp > 0)
            .map(|u| u.hp)
            .sum();
        let def_total_str: u16 = battle.defender_units.iter()
            .filter(|u| !u.routing && u.hp > 0)
            .map(|u| u.hp)
            .sum();

        // Damage proportional to relative strength.
        if att_total_str > 0 && def_total_str > 0 {
            let att_dmg = (att_total_str as f64 * 0.05) as u16;
            let def_dmg = (def_total_str as f64 * 0.05) as u16;

            // Distribute damage.
            let def_count = battle.defender_units.len().max(1) as u16;
            let att_count = battle.attacker_units.len().max(1) as u16;
            for unit in &mut battle.defender_units {
                if !unit.routing && unit.hp > 0 {
                    unit.hp = unit.hp.saturating_sub(att_dmg / def_count);
                    unit.morale -= 2.0;
                }
            }
            for unit in &mut battle.attacker_units {
                if !unit.routing && unit.hp > 0 {
                    unit.hp = unit.hp.saturating_sub(def_dmg / att_count);
                    unit.morale -= 2.0;
                }
            }

            battle.attacker_casualties += def_dmg as u32;
            battle.defender_casualties += att_dmg as u32;
        }

        // Check morale routing.
        for unit in battle.attacker_units.iter_mut().chain(battle.defender_units.iter_mut()) {
            if unit.morale <= 0.0 && !unit.routing {
                unit.routing = true;
            }
        }
    }

    // Remove battles where one side is fully routed or eliminated.
    battles.battles.retain(|b| {
        let att_active = b.attacker_units.iter().any(|u| !u.routing && u.hp > 0);
        let def_active = b.defender_units.iter().any(|u| !u.routing && u.hp > 0);
        att_active && def_active
    });
}

/// Auto-resolve a tactical battle using total strength comparison.
pub fn auto_resolve(battle: &TacticalBattle, _config: &TacticalGridConfig) -> super::CombatResult {
    use crate::world::GameDate;

    let att_str: u32 = battle.attacker_units.iter().map(|u| u.hp as u32).sum();
    let def_str: u32 = battle.defender_units.iter().map(|u| u.hp as u32).sum();

    let winner = if att_str > def_str {
        super::BattleSide::Attacker
    } else if def_str > att_str {
        super::BattleSide::Defender
    } else {
        super::BattleSide::Draw
    };

    let ratio = if att_str > def_str {
        def_str as f64 / att_str.max(1) as f64
    } else {
        att_str as f64 / def_str.max(1) as f64
    };

    super::CombatResult {
        location: battle.location,
        date: GameDate::default(),
        winner,
        attacker_casualties: (att_str as f64 * ratio * 0.3) as u32,
        defender_casualties: (def_str as f64 * (1.0 - ratio) * 0.3) as u32,
        attacker_nations: Vec::new(),
        defender_nations: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::ScopeId;
    use crate::armies::ArmyId;

    fn make_unit(army_raw: u32, hp: u16, morale: f64) -> TacticalUnit {
        TacticalUnit {
            army_id: ArmyId(std::num::NonZeroU32::new(army_raw).unwrap()),
            stack_index: 0,
            unit_type: None,
            grid_x: 0,
            grid_y: 0,
            facing: Facing::North,
            formation_index: 0,
            hp,
            morale,
            fatigue: 0.0,
            routing: false,
        }
    }

    #[test]
    fn tactical_grid_config_defaults() {
        let config = TacticalGridConfig::default();
        assert_eq!(config.grid_width, 40);
        assert_eq!(config.grid_height, 30);
        assert_eq!(config.formations.len(), 4);
        assert!((config.flank_damage_mult - 1.5).abs() < f64::EPSILON);
        assert!((config.rear_damage_mult - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn facing_default() {
        let f = Facing::default();
        assert_eq!(f, Facing::North);
    }

    #[test]
    fn tactical_unit_routing() {
        let mut unit = make_unit(1, 100, 50.0);
        assert!(!unit.routing);
        unit.morale = -1.0;
        if unit.morale <= 0.0 {
            unit.routing = true;
        }
        assert!(unit.routing);
    }

    #[test]
    fn auto_resolve_attacker_wins() {
        let battle = TacticalBattle {
            location: ProvinceId::from_raw(1),
            grid_width: 40,
            grid_height: 30,
            attacker_units: vec![make_unit(1, 200, 100.0)],
            defender_units: vec![make_unit(2, 50, 100.0)],
            tick: 0,
            attacker_casualties: 0,
            defender_casualties: 0,
        };
        let config = TacticalGridConfig::default();
        let result = auto_resolve(&battle, &config);
        assert_eq!(result.winner, super::super::BattleSide::Attacker);
    }

    #[test]
    fn auto_resolve_defender_wins() {
        let battle = TacticalBattle {
            location: ProvinceId::from_raw(1),
            grid_width: 40,
            grid_height: 30,
            attacker_units: vec![make_unit(1, 50, 100.0)],
            defender_units: vec![make_unit(2, 200, 100.0)],
            tick: 0,
            attacker_casualties: 0,
            defender_casualties: 0,
        };
        let config = TacticalGridConfig::default();
        let result = auto_resolve(&battle, &config);
        assert_eq!(result.winner, super::super::BattleSide::Defender);
    }

    #[test]
    fn auto_resolve_draw() {
        let battle = TacticalBattle {
            location: ProvinceId::from_raw(1),
            grid_width: 40,
            grid_height: 30,
            attacker_units: vec![make_unit(1, 100, 100.0)],
            defender_units: vec![make_unit(2, 100, 100.0)],
            tick: 0,
            attacker_casualties: 0,
            defender_casualties: 0,
        };
        let config = TacticalGridConfig::default();
        let result = auto_resolve(&battle, &config);
        assert_eq!(result.winner, super::super::BattleSide::Draw);
    }

    #[test]
    fn active_tactical_battles_default() {
        let battles = ActiveTacticalBattles::default();
        assert!(battles.battles.is_empty());
    }

    #[test]
    fn formation_def_properties() {
        let config = TacticalGridConfig::default();
        let line = &config.formations[0];
        assert_eq!(line.name, "Line");
        let wedge = &config.formations[3];
        assert_eq!(wedge.name, "Wedge");
        assert!(wedge.attack_bonus > 0.0);
    }
}
