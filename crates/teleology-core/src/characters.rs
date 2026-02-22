//! Character system: leaders, generals, advisors, and other people.
//!
//! Implementation notes:
//! - ECS-based: characters are entities with components.
//! - Modular: worlds may have zero characters; systems should handle absence.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::world::NationId;

/// Base character data (identity + life dates).
#[derive(Component, Clone, Serialize, Deserialize)]
pub struct Character {
    /// Name reference into a game-defined string table.
    pub name_id: u32,
    /// Optional unique id for external references (e.g. save migration, debug).
    pub persistent_id: u64,
    /// Birth year (optional; games may use different calendars).
    pub birth_year: Option<i32>,
    /// Death year (optional).
    pub death_year: Option<i32>,
}

impl Default for Character {
    fn default() -> Self {
        Self {
            name_id: 0,
            persistent_id: 0,
            birth_year: None,
            death_year: None,
        }
    }
}

/// Character role within the game world.
#[derive(Component, Clone, Serialize, Deserialize)]
pub enum CharacterRole {
    /// Head of state / ruler of a nation.
    Leader(NationId),
    /// Military commander; army linkage is handled by the army system.
    General { nation: NationId, army_raw: u32 },
    /// Advisor/court position.
    Advisor(NationId),
    /// Game-defined custom role id.
    Custom(u32),
}

/// Common stats for grand-strategy characters, plus a custom stat map.
#[derive(Component, Clone, Default, Serialize, Deserialize)]
pub struct CharacterStats {
    pub military: i16,
    pub diplomacy: i16,
    pub administration: i16,
    /// Game-defined stats keyed by id (e.g. intrigue, legitimacy, charisma).
    pub custom: HashMap<u32, i32>,
}

/// Simple helper: spawn a character entity with baseline components.
pub fn spawn_character(world: &mut World, character: Character) -> Entity {
    world.spawn((character, CharacterStats::default())).id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::world::World;
    use crate::world::ScopeId;

    #[test]
    fn character_default() {
        let c = Character::default();
        assert_eq!(c.name_id, 0);
        assert_eq!(c.persistent_id, 0);
        assert!(c.birth_year.is_none());
        assert!(c.death_year.is_none());
    }

    #[test]
    fn character_stats_default() {
        let s = CharacterStats::default();
        assert_eq!(s.military, 0);
        assert_eq!(s.diplomacy, 0);
        assert_eq!(s.administration, 0);
        assert!(s.custom.is_empty());
    }

    #[test]
    fn spawn_character_creates_entity() {
        let mut world = World::new();
        let c = Character {
            name_id: 42,
            persistent_id: 999,
            birth_year: Some(1400),
            death_year: None,
        };
        let entity = spawn_character(&mut world, c);
        let ch = world.get::<Character>(entity).unwrap();
        assert_eq!(ch.name_id, 42);
        assert_eq!(ch.persistent_id, 999);
        assert_eq!(ch.birth_year, Some(1400));

        let stats = world.get::<CharacterStats>(entity).unwrap();
        assert_eq!(stats.military, 0);
    }

    #[test]
    fn character_role_variants() {
        let nid = NationId::from_raw(1);
        let leader = CharacterRole::Leader(nid);
        let general = CharacterRole::General { nation: nid, army_raw: 5 };
        let advisor = CharacterRole::Advisor(nid);
        let custom = CharacterRole::Custom(99);

        match leader {
            CharacterRole::Leader(n) => assert_eq!(n, nid),
            _ => panic!("expected Leader"),
        }
        match general {
            CharacterRole::General { nation, army_raw } => {
                assert_eq!(nation, nid);
                assert_eq!(army_raw, 5);
            }
            _ => panic!("expected General"),
        }
        match advisor {
            CharacterRole::Advisor(n) => assert_eq!(n, nid),
            _ => panic!("expected Advisor"),
        }
        match custom {
            CharacterRole::Custom(id) => assert_eq!(id, 99),
            _ => panic!("expected Custom"),
        }
    }

    #[test]
    fn character_stats_custom() {
        let mut stats = CharacterStats::default();
        stats.custom.insert(1, 50);
        stats.custom.insert(2, -10);
        assert_eq!(stats.custom[&1], 50);
        assert_eq!(stats.custom[&2], -10);
    }
}

