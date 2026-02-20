//! Army system: higher-level military units with composition and commanders.
//!
//! Armies are ECS entities, but also have stable `ArmyId` handles for save/load and scripts.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::num::NonZeroU32;

use crate::world::{NationId, ProvinceId};

/// Stable id for an army.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArmyId(pub NonZeroU32);

impl ArmyId {
    #[inline]
    pub fn raw(self) -> u32 {
        self.0.get()
    }
}

/// Army component (core state).
#[derive(Component, Clone, Serialize, Deserialize)]
pub struct Army {
    pub id: ArmyId,
    pub owner: NationId,
    pub location: ProvinceId,
    pub strength: u16,
    pub organization: u16,
}

/// Unit stack inside an army (game-defined unit_type ids).
#[derive(Clone, Serialize, Deserialize)]
pub struct UnitStack {
    pub unit_type: u32,
    pub count: u32,
}

/// Army composition (modular unit types for different genres).
#[derive(Component, Clone, Default, Serialize, Deserialize)]
pub struct ArmyComposition {
    pub stacks: Vec<UnitStack>,
}

/// Commander link (character system is modular, so we use a raw persistent id).
#[derive(Component, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ArmyCommander {
    pub character_persistent_id: Option<u64>,
}

/// Simple state machine for armies.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArmyStatus {
    Idle,
    Marching { to: ProvinceId },
    Sieging { target: ProvinceId },
}

impl Default for ArmyStatus {
    fn default() -> Self {
        ArmyStatus::Idle
    }
}

/// Registry mapping stable ArmyId to ECS entity.
#[derive(Resource, Default, Serialize, Deserialize)]
pub struct ArmyRegistry {
    pub next_raw: u32,
    #[serde(skip)]
    pub entity_by_raw: HashMap<u32, Entity>,
}

impl ArmyRegistry {
    pub fn new() -> Self {
        Self {
            next_raw: 1,
            entity_by_raw: HashMap::new(),
        }
    }

    fn alloc_id(&mut self) -> ArmyId {
        let raw = self.next_raw.max(1);
        self.next_raw = raw.saturating_add(1);
        ArmyId(NonZeroU32::new(raw).unwrap())
    }

    pub fn get_entity(&self, id: ArmyId) -> Option<Entity> {
        self.entity_by_raw.get(&id.raw()).copied()
    }
}

/// Spawn a new army with basic components. Requires `ArmyRegistry` resource.
pub fn spawn_army(
    world: &mut World,
    owner: NationId,
    location: ProvinceId,
    composition: ArmyComposition,
) -> ArmyId {
    let mut reg = world
        .get_resource_mut::<ArmyRegistry>()
        .expect("ArmyRegistry must be inserted to spawn armies");
    let id = reg.alloc_id();
    drop(reg);

    let entity = world
        .spawn((
            Army {
                id,
                owner,
                location,
                strength: 1000,
                organization: 100,
            },
            composition,
            ArmyCommander::default(),
            ArmyStatus::default(),
        ))
        .id();

    if let Some(mut reg) = world.get_resource_mut::<ArmyRegistry>() {
        reg.entity_by_raw.insert(id.raw(), entity);
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::world::World;
    use crate::world::ScopeId;

    #[test]
    fn army_registry_alloc_ids() {
        let mut reg = ArmyRegistry::new();
        let id1 = reg.alloc_id();
        let id2 = reg.alloc_id();
        assert_eq!(id1.raw(), 1);
        assert_eq!(id2.raw(), 2);
        assert_ne!(id1, id2);
    }

    #[test]
    fn army_registry_get_entity() {
        let mut world = World::new();
        let entity = world.spawn_empty().id();
        let mut reg = ArmyRegistry::new();
        let id = reg.alloc_id();
        reg.entity_by_raw.insert(id.raw(), entity);
        assert_eq!(reg.get_entity(id), Some(entity));
    }

    #[test]
    fn spawn_army_creates_entity() {
        let mut world = World::new();
        world.insert_resource(ArmyRegistry::new());

        let owner = NationId::from_raw(1);
        let loc = ProvinceId::from_raw(3);
        let comp = ArmyComposition {
            stacks: vec![UnitStack { unit_type: 1, count: 10 }],
        };
        let id = spawn_army(&mut world, owner, loc, comp);
        assert_eq!(id.raw(), 1);

        let reg = world.get_resource::<ArmyRegistry>().unwrap();
        let entity = reg.get_entity(id).unwrap();
        let army = world.get::<Army>(entity).unwrap();
        assert_eq!(army.owner, owner);
        assert_eq!(army.location, loc);
        assert_eq!(army.strength, 1000);
        assert_eq!(army.organization, 100);
    }

    #[test]
    fn army_status_default_is_idle() {
        assert_eq!(ArmyStatus::default(), ArmyStatus::Idle);
    }

    #[test]
    fn army_composition_stacks() {
        let comp = ArmyComposition {
            stacks: vec![
                UnitStack { unit_type: 1, count: 5 },
                UnitStack { unit_type: 2, count: 3 },
            ],
        };
        assert_eq!(comp.stacks.len(), 2);
        assert_eq!(comp.stacks[0].count, 5);
    }

    #[test]
    fn army_commander_default() {
        let cmd = ArmyCommander::default();
        assert!(cmd.character_persistent_id.is_none());
    }
}

