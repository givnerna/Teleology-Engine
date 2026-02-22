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
    /// Occupier during wartime (different from owner). None if not occupied.
    pub occupation: Option<NationId>,
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
            occupation: None,
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
    /// War exhaustion: 0.0 = fresh, 100.0 = exhausted.
    pub war_exhaustion: f32,
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
            war_exhaustion: 0.0,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::ScopeId;

    #[test]
    fn province_default_for() {
        let pid = ProvinceId::from_raw(5);
        let p = Province::default_for(pid);
        assert_eq!(p.id, pid);
        assert!(p.owner.is_none());
        assert!(p.occupation.is_none());
        assert_eq!(p.terrain, TERRAIN_LAND);
        assert_eq!(p.development, [1, 1, 1]);
        assert_eq!(p.population, 0);
    }

    #[test]
    fn province_is_land() {
        let mut p = Province::default_for(ProvinceId::from_raw(1));
        assert!(p.is_land());
        p.terrain = TERRAIN_SEA;
        assert!(!p.is_land());
    }

    #[test]
    fn nation_default_for() {
        let nid = NationId::from_raw(3);
        let n = Nation::default_for(nid);
        assert_eq!(n.id, nid);
        assert_eq!(n.name_id, 0);
        assert_eq!(n.prestige, 0);
        assert_eq!(n.stability, 0);
        assert_eq!(n.treasury, 0);
        assert_eq!(n.manpower, 0);
        assert_eq!(n.war_exhaustion, 0.0);
    }

    #[test]
    fn province_ownership() {
        let mut p = Province::default_for(ProvinceId::from_raw(1));
        assert!(p.owner.is_none());
        p.owner = Some(NationId::from_raw(2));
        assert_eq!(p.owner.unwrap(), NationId::from_raw(2));
    }

    #[test]
    fn province_occupation() {
        let mut p = Province::default_for(ProvinceId::from_raw(1));
        p.owner = Some(NationId::from_raw(1));
        p.occupation = Some(NationId::from_raw(2));
        assert_ne!(p.owner, p.occupation);
    }
}
