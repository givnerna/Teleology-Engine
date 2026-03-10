//! Core archetypes for grand strategy: Province, Nation, Unit.
//!
//! Data-oriented: fields are laid out for bulk access. Scripts (C++) read/write
//! via the script API using indices/opaque handles.

use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};

use crate::world::{NationId, ProvinceId, ScopeId};

/// Terrain type for a province (Paradox-style: land vs sea).
pub const TERRAIN_LAND: u8 = 0;
pub const TERRAIN_SEA: u8 = 1;

/// A terrain type definition. Developers register these to define their game's terrain palette.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TerrainType {
    /// Terrain id (matches Province.terrain u8 value).
    pub id: u8,
    /// Display name (e.g. "Grassland", "Ocean", "Desert").
    pub name: String,
    /// Display color as [R, G, B, A].
    pub color: [u8; 4],
    /// Whether this terrain is passable by land units.
    pub is_land: bool,
}

/// Registry of all terrain types. Stored as a Bevy Resource.
/// Developers populate this via WorldBuilder or at runtime.
/// Ships with two defaults: Land (0) and Sea (1).
#[derive(Clone, Debug, Resource, Serialize, Deserialize)]
pub struct TerrainRegistry {
    pub types: Vec<TerrainType>,
}

impl Default for TerrainRegistry {
    fn default() -> Self {
        Self {
            types: vec![
                TerrainType { id: 0, name: "Land".into(), color: [0x22, 0x8B, 0x22, 0xFF], is_land: true },
                TerrainType { id: 1, name: "Sea".into(), color: [0x1E, 0x3A, 0x5F, 0xFF], is_land: false },
            ],
        }
    }
}

impl TerrainRegistry {
    /// Look up a terrain type by id.
    pub fn get(&self, id: u8) -> Option<&TerrainType> {
        self.types.iter().find(|t| t.id == id)
    }

    /// Register a new terrain type (or replace existing with same id).
    pub fn register(&mut self, t: TerrainType) {
        if let Some(existing) = self.types.iter_mut().find(|e| e.id == t.id) {
            *existing = t;
        } else {
            self.types.push(t);
        }
    }

    /// Get display color for a terrain id. Returns dark gray if not found.
    pub fn color(&self, id: u8) -> [u8; 4] {
        self.get(id).map(|t| t.color).unwrap_or([0x40, 0x40, 0x40, 0xFF])
    }

    /// Get display name for a terrain id. Returns "Unknown" if not found.
    pub fn name(&self, id: u8) -> &str {
        self.get(id).map(|t| t.name.as_str()).unwrap_or("Unknown")
    }
}

/// Shared interface for scope entities (Province, Nation) so generic stores
/// and systems can construct and identify them without knowing concrete types.
pub trait ScopeEntity: Clone + Send + Sync + 'static {
    type Id: ScopeId;
    fn id(&self) -> Self::Id;
    fn default_for(id: Self::Id) -> Self;
}

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
    #[inline]
    pub fn is_land(&self) -> bool {
        self.terrain == TERRAIN_LAND
    }
}

impl ScopeEntity for Province {
    type Id = ProvinceId;

    #[inline]
    fn id(&self) -> ProvinceId { self.id }

    fn default_for(id: ProvinceId) -> Self {
        Self {
            id,
            owner: None,
            occupation: None,
            terrain: TERRAIN_LAND,
            development: [1, 1, 1],
            population: 0,
        }
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

impl ScopeEntity for Nation {
    type Id = NationId;

    #[inline]
    fn id(&self) -> NationId { self.id }

    fn default_for(id: NationId) -> Self {
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
        let p = <Province as ScopeEntity>::default_for(pid);
        assert_eq!(p.id, pid);
        assert!(p.owner.is_none());
        assert!(p.occupation.is_none());
        assert_eq!(p.terrain, TERRAIN_LAND);
        assert_eq!(p.development, [1, 1, 1]);
        assert_eq!(p.population, 0);
    }

    #[test]
    fn province_is_land() {
        let mut p = <Province as ScopeEntity>::default_for(ProvinceId::from_raw(1));
        assert!(p.is_land());
        p.terrain = TERRAIN_SEA;
        assert!(!p.is_land());
    }

    #[test]
    fn nation_default_for() {
        let nid = NationId::from_raw(3);
        let n = <Nation as ScopeEntity>::default_for(nid);
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
        let mut p = <Province as ScopeEntity>::default_for(ProvinceId::from_raw(1));
        assert!(p.owner.is_none());
        p.owner = Some(NationId::from_raw(2));
        assert_eq!(p.owner.unwrap(), NationId::from_raw(2));
    }

    #[test]
    fn province_occupation() {
        let mut p = <Province as ScopeEntity>::default_for(ProvinceId::from_raw(1));
        p.owner = Some(NationId::from_raw(1));
        p.occupation = Some(NationId::from_raw(2));
        assert_ne!(p.owner, p.occupation);
    }

    #[test]
    fn terrain_registry_default() {
        let reg = TerrainRegistry::default();
        assert_eq!(reg.types.len(), 2);
        assert_eq!(reg.name(0), "Land");
        assert_eq!(reg.name(1), "Sea");
        assert!(reg.get(0).unwrap().is_land);
        assert!(!reg.get(1).unwrap().is_land);
    }

    #[test]
    fn terrain_registry_register() {
        let mut reg = TerrainRegistry::default();
        reg.register(TerrainType {
            id: 2,
            name: "Desert".into(),
            color: [0xED, 0xC9, 0x67, 0xFF],
            is_land: true,
        });
        assert_eq!(reg.types.len(), 3);
        assert_eq!(reg.name(2), "Desert");
        // Replace existing
        reg.register(TerrainType {
            id: 0,
            name: "Grassland".into(),
            color: [0x32, 0xCD, 0x32, 0xFF],
            is_land: true,
        });
        assert_eq!(reg.types.len(), 3);
        assert_eq!(reg.name(0), "Grassland");
    }

    #[test]
    fn terrain_registry_unknown() {
        let reg = TerrainRegistry::default();
        assert_eq!(reg.name(99), "Unknown");
        assert_eq!(reg.color(99), [0x40, 0x40, 0x40, 0xFF]);
    }
}
