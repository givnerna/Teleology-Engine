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

