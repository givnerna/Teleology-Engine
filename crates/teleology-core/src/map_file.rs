//! Map file format for save/load and upload. Paradox-style: layout, adjacency, terrain.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::num::NonZeroU32;

use crate::archetypes::{Nation, Province};
use crate::armies::{Army, ArmyCommander, ArmyComposition, ArmyRegistry, ArmyStatus};
use crate::modifiers::ArmyModifiers;
use crate::characters::{Character, CharacterRole, CharacterStats};
use crate::modifiers::{CharacterModifiers, NationModifiers, ProvinceModifiers};
use crate::progress_trees::{ProgressState, ProgressTrees};
use crate::tags::{NationTags, ProvinceTags, TagRegistry};
use crate::world::{
    GameDate, HexMapLayout, MapKind, MapLayout, NationStore, ProvinceAdjacency, ProvinceId,
    ProvinceStore, VectorMapLayout, WorldBounds,
};

const MAP_FILE_VERSION: u32 = 3;

/// Serializable map + world snapshot for upload/save/load.
#[derive(Clone, Serialize, Deserialize)]
pub struct MapFile {
    pub version: u32,
    pub bounds: WorldBounds,
    pub date: GameDate,
    pub map_kind: MapKind,
    pub adjacency: ProvinceAdjacency,
    pub provinces: Vec<Province>,
    pub nations: Vec<Nation>,

    // Optional modular systems (present only if enabled).
    pub tag_registry: Option<TagRegistry>,
    pub province_tags: Option<ProvinceTags>,
    pub nation_tags: Option<NationTags>,

    pub province_modifiers: Option<ProvinceModifiers>,
    pub nation_modifiers: Option<NationModifiers>,

    pub event_registry: Option<crate::events::EventRegistry>,
    pub event_queue: Option<crate::events::EventQueue>,
    pub active_event: Option<crate::events::ActiveEvent>,

    pub event_bus: Option<crate::event_bus::EventBus>,

    pub progress_trees: Option<ProgressTrees>,
    pub progress_state: Option<ProgressState>,

    pub characters: Vec<CharacterSave>,
    pub armies: Vec<ArmySave>,
}

impl MapFile {
    /// Build a map file snapshot from the current world. Returns None if required resources are missing.
    ///
    /// Note: this takes `&mut World` because Bevy ECS queries are created from a mutable World.
    pub fn from_world(world: &mut bevy_ecs::world::World) -> Option<Self> {
        let bounds = world.get_resource::<WorldBounds>()?.clone();
        let date = *world.get_resource::<GameDate>()?;
        let map_kind = world.get_resource::<MapKind>()?.clone();
        let adjacency = world
            .get_resource::<ProvinceAdjacency>()
            .cloned()
            .unwrap_or_else(|| compute_adjacency(&map_kind, bounds.province_count));
        let provinces = world.get_resource::<ProvinceStore>()?.provinces.clone();
        let nations = world.get_resource::<NationStore>()?.nations.clone();

        // Optional modules (resources).
        let tag_registry = world.get_resource::<TagRegistry>().cloned();
        let province_tags = world.get_resource::<ProvinceTags>().cloned();
        let nation_tags = world.get_resource::<NationTags>().cloned();

        let province_modifiers = world.get_resource::<ProvinceModifiers>().cloned();
        let nation_modifiers = world.get_resource::<NationModifiers>().cloned();

        let event_registry = world.get_resource::<crate::events::EventRegistry>().cloned();
        let event_queue = world.get_resource::<crate::events::EventQueue>().cloned();
        let active_event = world.get_resource::<crate::events::ActiveEvent>().cloned();

        let event_bus = world.get_resource::<crate::event_bus::EventBus>().cloned();

        let progress_trees = world.get_resource::<ProgressTrees>().cloned();
        let progress_state = world.get_resource::<ProgressState>().cloned();

        // ECS modules (characters, armies) saved as explicit lists.
        let mut characters: Vec<CharacterSave> = Vec::new();
        {
            let mut q = world.query::<(
                &Character,
                &CharacterStats,
                Option<&CharacterRole>,
                Option<&CharacterModifiers>,
            )>();
            for (_e, (c, s, role, mods)) in q.iter(world).enumerate() {
                let _ = _e;
                characters.push(CharacterSave {
                    character: c.clone(),
                    stats: s.clone(),
                    role: role.cloned(),
                    modifiers: mods.cloned(),
                });
            }
        }

        let mut armies: Vec<ArmySave> = Vec::new();
        {
            let mut q = world.query::<(
                &Army,
                &ArmyComposition,
                &ArmyCommander,
                &ArmyStatus,
                Option<&ArmyModifiers>,
            )>();
            for (_e, (a, comp, cmd, st, mods)) in q.iter(world).enumerate() {
                let _ = _e;
                armies.push(ArmySave {
                    army: a.clone(),
                    composition: comp.clone(),
                    commander: *cmd,
                    status: *st,
                    modifiers: mods.cloned(),
                });
            }
        }

        Some(Self {
            version: MAP_FILE_VERSION,
            bounds,
            date,
            map_kind,
            adjacency,
            provinces,
            nations,
            tag_registry,
            province_tags,
            nation_tags,
            province_modifiers,
            nation_modifiers,
            event_registry,
            event_queue,
            active_event,
            event_bus,
            progress_trees,
            progress_state,
            characters,
            armies,
        })
    }

    /// Write to a binary stream (bincode).
    pub fn write<W: Write>(&self, w: &mut W) -> Result<(), bincode::Error> {
        bincode::serialize_into(w, self)
    }

    /// Read from a binary stream (bincode). Version 2 uses map_kind (square/hex/irregular).
    pub fn read<R: Read>(r: &mut R) -> Result<Self, bincode::Error> {
        bincode::deserialize_from(r)
    }

    /// Write to a stream as JSON.
    pub fn write_json<W: Write>(&self, w: &mut W) -> Result<(), serde_json::Error> {
        serde_json::to_writer(w, self)
    }

    /// Write to a stream as pretty-printed JSON.
    pub fn write_json_pretty<W: Write>(&self, w: &mut W) -> Result<(), serde_json::Error> {
        serde_json::to_writer_pretty(w, self)
    }

    /// Read from a JSON stream.
    pub fn read_json<R: Read>(r: &mut R) -> Result<Self, serde_json::Error> {
        serde_json::from_reader(r)
    }

    /// Apply this map file to an existing world (replaces map-related resources).
    pub fn apply_to_world(&self, world: &mut bevy_ecs::world::World) {
        world.insert_resource(self.bounds.clone());
        world.insert_resource(self.date);
        world.insert_resource(self.map_kind.clone());
        world.insert_resource(self.adjacency.clone());
        world.insert_resource(ProvinceStore {
            provinces: self.provinces.clone(),
        });
        world.insert_resource(NationStore {
            nations: self.nations.clone(),
        });

        // Optional resources.
        if let Some(reg) = &self.tag_registry {
            world.insert_resource(reg.clone());
        }
        if let Some(pt) = &self.province_tags {
            world.insert_resource(pt.clone());
        }
        if let Some(nt) = &self.nation_tags {
            world.insert_resource(nt.clone());
        }
        if let Some(pm) = &self.province_modifiers {
            world.insert_resource(pm.clone());
        }
        if let Some(nm) = &self.nation_modifiers {
            world.insert_resource(nm.clone());
        }
        if let Some(er) = &self.event_registry {
            world.insert_resource(er.clone());
        }
        if let Some(eq) = &self.event_queue {
            world.insert_resource(eq.clone());
        }
        if let Some(ae) = &self.active_event {
            world.insert_resource(ae.clone());
        }
        if let Some(bus) = &self.event_bus {
            world.insert_resource(bus.clone());
        }
        if let Some(trees) = &self.progress_trees {
            world.insert_resource(trees.clone());
        }
        if let Some(state) = &self.progress_state {
            world.insert_resource(state.clone());
        }

        // Rebuild ECS entities for characters.
        for cs in &self.characters {
            let mut ent = world.spawn((cs.character.clone(), cs.stats.clone()));
            if let Some(role) = &cs.role {
                ent.insert(role.clone());
            }
            if let Some(mods) = &cs.modifiers {
                ent.insert(mods.clone());
            }
        }

        // Armies require an ArmyRegistry to map stable ids.
        if !self.armies.is_empty() && world.get_resource::<ArmyRegistry>().is_none() {
            world.insert_resource(ArmyRegistry::new());
        }
        for a in &self.armies {
            let e = world
                .spawn((
                    a.army.clone(),
                    a.composition.clone(),
                    a.commander,
                    a.status,
                ))
                .id();
            if let Some(mods) = &a.modifiers {
                world.entity_mut(e).insert(mods.clone());
            }
            if let Some(mut reg) = world.get_resource_mut::<ArmyRegistry>() {
                reg.entity_by_raw.insert(a.army.id.raw(), e);
                reg.next_raw = reg.next_raw.max(a.army.id.raw().saturating_add(1));
            }
        }
    }
}

/// Saved character entry for MapFile.
#[derive(Clone, Serialize, Deserialize)]
pub struct CharacterSave {
    pub character: Character,
    pub stats: CharacterStats,
    pub role: Option<CharacterRole>,
    pub modifiers: Option<CharacterModifiers>,
}

/// Saved army entry for MapFile.
#[derive(Clone, Serialize, Deserialize)]
pub struct ArmySave {
    pub army: Army,
    pub composition: ArmyComposition,
    pub commander: ArmyCommander,
    pub status: ArmyStatus,
    pub modifiers: Option<ArmyModifiers>,
}

/// Compute province adjacency from a square grid layout (4-connected).
pub fn compute_adjacency_from_layout(layout: &MapLayout, province_count: u32) -> ProvinceAdjacency {
    let mut adj = ProvinceAdjacency::new(province_count as usize);
    let w = layout.width;
    let h = layout.height;
    let mut add = |x1: u32, y1: u32, x2: u32, y2: u32| {
        let a = layout.get(x1, y1);
        let b = layout.get(x2, y2);
        if a != 0 && b != 0 && a != b {
            let pid = ProvinceId(NonZeroU32::new(a).unwrap());
            adj.add_neighbor(pid, b);
            let pid2 = ProvinceId(NonZeroU32::new(b).unwrap());
            adj.add_neighbor(pid2, a);
        }
    };
    for y in 0..h {
        for x in 0..w {
            if x + 1 < w {
                add(x, y, x + 1, y);
            }
            if y + 1 < h {
                add(x, y, x, y + 1);
            }
        }
    }
    adj
}

/// Compute province adjacency from a hex grid (6 neighbors per hex).
pub fn compute_adjacency_from_hex(layout: &HexMapLayout, province_count: u32) -> ProvinceAdjacency {
    let mut adj = ProvinceAdjacency::new(province_count as usize);
    let w = layout.width;
    let h = layout.height;
    for r in 0..h {
        for q in 0..w {
            let a = layout.get(q, r);
            if a == 0 {
                continue;
            }
            for (nq, nr) in layout.neighbors(q, r) {
                if nq < w && nr < h {
                    let b = layout.get(nq, nr);
                    if b != 0 && b != a {
                        let pid = ProvinceId(NonZeroU32::new(a).unwrap());
                        adj.add_neighbor(pid, b);
                    }
                }
            }
        }
    }
    adj
}

/// Normalize edge for shared-edge detection (order so (a,b) and (b,a) match).
fn edge_key(a: [f64; 2], b: [f64; 2], tol: f64) -> ((i64, i64), (i64, i64)) {
    let to_int = |p: [f64; 2]| ((p[0] / tol).round() as i64, (p[1] / tol).round() as i64);
    let pa = to_int(a);
    let pb = to_int(b);
    if pa <= pb {
        (pa, pb)
    } else {
        (pb, pa)
    }
}

/// Compute province adjacency from vector polygons (shared edges).
pub fn compute_adjacency_from_vector(
    layout: &VectorMapLayout,
    province_count: u32,
) -> ProvinceAdjacency {
    let mut adj = ProvinceAdjacency::new(province_count as usize);
    const TOL: f64 = 1e-6;
    let mut edge_to_provinces: HashMap<((i64, i64), (i64, i64)), Vec<u32>> = HashMap::new();
    for poly in &layout.polygons {
        let id = poly.province_id;
        if id == 0 {
            continue;
        }
        let v = &poly.vertices;
        for i in 0..v.len() {
            let a = v[i];
            let b = v[(i + 1) % v.len()];
            let key = edge_key(a, b, TOL);
            edge_to_provinces.entry(key).or_default().push(id);
        }
    }
    for (_edge, provs) in edge_to_provinces {
        let uniq: Vec<u32> = provs.into_iter().collect::<std::collections::HashSet<_>>().into_iter().collect();
        for i in 0..uniq.len() {
            for j in (i + 1)..uniq.len() {
                let pid_a = ProvinceId(NonZeroU32::new(uniq[i]).unwrap());
                let pid_b = ProvinceId(NonZeroU32::new(uniq[j]).unwrap());
                adj.add_neighbor(pid_a, uniq[j]);
                adj.add_neighbor(pid_b, uniq[i]);
            }
        }
    }
    adj
}

/// Compute province adjacency from any map kind.
pub fn compute_adjacency(map_kind: &MapKind, province_count: u32) -> ProvinceAdjacency {
    match map_kind {
        MapKind::Square(layout) => compute_adjacency_from_layout(layout, province_count),
        MapKind::Hex(layout) => compute_adjacency_from_hex(layout, province_count),
        MapKind::Irregular(layout) => compute_adjacency_from_vector(layout, province_count),
    }
}
