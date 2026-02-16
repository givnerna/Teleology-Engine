//! Core archetypes for grand strategy: Province, Nation, Unit.
//!
//! Data-oriented: fields are laid out for bulk access. Scripts (C++) read/write
//! via the script API using indices/opaque handles.

use serde::{Deserialize, Serialize};

use crate::world::{NationId, ProvinceId};

/// Terrain type for a province (Paradox-style: land vs sea).
pub const TERRAIN_LAND: u8 = 0;
pub const TERRAIN_SEA: u8 = 1;

/// Province: one map tile/region. SoA-friendly: if you need max perf,
/// split into e.g. `owner: Vec<NationId>`, `terrain: Vec<u8>`, etc.
#[derive(Clone, Serialize, Deserialize)]
pub struct Province {
    pub id: ProvinceId,
    pub owner: Option<NationId>,
    /// Terrain: TERRAIN_LAND (0), TERRAIN_SEA (1), or custom.
    pub terrain: u8,
    pub development: [u16; 3], // tax, production, manpower
    pub population: u32,
}

impl Province {
    pub fn default_for(id: ProvinceId) -> Self {
        Self {
            id,
            owner: None,
            terrain: TERRAIN_LAND,
            development: [1, 1, 1],
            population: 0,
        }
    }

    #[inline]
    pub fn is_land(&self) -> bool {
        self.terrain == TERRAIN_LAND
    }
}

/// Nation/tag: one country.
#[derive(Clone, Serialize, Deserialize)]
pub struct Nation {
    pub id: NationId,
    pub name_id: u32,
    pub prestige: i32,
    pub stability: i8,
    pub treasury: i64,
    pub manpower: u32,
}

impl Nation {
    pub fn default_for(id: NationId) -> Self {
        Self {
            id,
            name_id: 0,
            prestige: 0,
            stability: 0,
            treasury: 0,
            manpower: 0,
        }
    }
}

/// Unit: army or fleet. Stored as ECS entities for variable count.
#[derive(Clone, Serialize, Deserialize)]
pub struct Unit {
    pub province: ProvinceId,
    pub owner: NationId,
    pub strength: u16,
    pub kind: u8, // 0 = army, 1 = fleet, etc.
}
