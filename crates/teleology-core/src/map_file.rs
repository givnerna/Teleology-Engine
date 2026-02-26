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
    GameDate, GameTime, HexMapLayout, MapKind, MapLayout, NationStore, ProvinceAdjacency,
    ProvinceId, ProvinceStore, TimeConfig, VectorMapLayout, WorldBounds,
};

const MAP_FILE_VERSION: u32 = 3;

/// Serializable map + world snapshot for upload/save/load.
#[derive(Clone, Serialize, Deserialize)]
pub struct MapFile {
    pub version: u32,
    pub bounds: WorldBounds,
    pub date: GameDate,
    pub time: Option<GameTime>,
    pub time_config: Option<TimeConfig>,
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

    // Tier 1 gameplay systems (present only if enabled).
    pub economy_config: Option<crate::economy::EconomyConfig>,
    pub nation_budgets: Option<crate::economy::NationBudgets>,
    pub goods_registry: Option<crate::economy::GoodsRegistry>,
    pub province_economy: Option<crate::economy::ProvinceEconomy>,
    pub trade_network: Option<crate::economy::TradeNetwork>,

    pub diplomacy_config: Option<crate::diplomacy::DiplomacyConfig>,
    pub diplomatic_relations: Option<crate::diplomacy::DiplomaticRelations>,
    pub war_registry: Option<crate::diplomacy::WarRegistry>,

    pub combat_model: Option<crate::combat::CombatModel>,
    pub unit_type_registry: Option<crate::combat::UnitTypeRegistry>,
    pub combat_result_log: Option<crate::combat::CombatResultLog>,

    pub population_config: Option<crate::population::PopulationConfig>,
    pub province_pops: Option<crate::population::ProvincePops>,
}

impl MapFile {
    /// Build a map file snapshot from the current world. Returns None if required resources are missing.
    ///
    /// Note: this takes `&mut World` because Bevy ECS queries are created from a mutable World.
    pub fn from_world(world: &mut bevy_ecs::world::World) -> Option<Self> {
        let bounds = world.get_resource::<WorldBounds>()?.clone();
        let date = *world.get_resource::<GameDate>()?;
        let time = world.get_resource::<GameTime>().copied();
        let time_config = world.get_resource::<TimeConfig>().cloned();
        let map_kind = world.get_resource::<MapKind>()?.clone();
        let adjacency = world
            .get_resource::<ProvinceAdjacency>()
            .cloned()
            .unwrap_or_else(|| compute_adjacency(&map_kind, bounds.province_count));
        let provinces = world.get_resource::<ProvinceStore>()?.items.clone();
        let nations = world.get_resource::<NationStore>()?.items.clone();

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

        // Tier 1 gameplay systems.
        let economy_config = world.get_resource::<crate::economy::EconomyConfig>().cloned();
        let nation_budgets = world.get_resource::<crate::economy::NationBudgets>().cloned();
        let goods_registry = world.get_resource::<crate::economy::GoodsRegistry>().cloned();
        let province_economy = world.get_resource::<crate::economy::ProvinceEconomy>().cloned();
        let trade_network = world.get_resource::<crate::economy::TradeNetwork>().cloned();

        let diplomacy_config = world.get_resource::<crate::diplomacy::DiplomacyConfig>().cloned();
        let diplomatic_relations = world.get_resource::<crate::diplomacy::DiplomaticRelations>().cloned();
        let war_registry = world.get_resource::<crate::diplomacy::WarRegistry>().cloned();

        let combat_model = world.get_resource::<crate::combat::CombatModel>().cloned();
        let unit_type_registry = world.get_resource::<crate::combat::UnitTypeRegistry>().cloned();
        let combat_result_log = world.get_resource::<crate::combat::CombatResultLog>().cloned();

        let population_config = world.get_resource::<crate::population::PopulationConfig>().cloned();
        let province_pops = world.get_resource::<crate::population::ProvincePops>().cloned();

        Some(Self {
            version: MAP_FILE_VERSION,
            bounds,
            date,
            time,
            time_config,
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
            economy_config,
            nation_budgets,
            goods_registry,
            province_economy,
            trade_network,
            diplomacy_config,
            diplomatic_relations,
            war_registry,
            combat_model,
            unit_type_registry,
            combat_result_log,
            population_config,
            province_pops,
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
        if let Some(time) = self.time {
            world.insert_resource(time);
        }
        if let Some(ref config) = self.time_config {
            world.insert_resource(config.clone());
        }
        world.insert_resource(self.map_kind.clone());
        world.insert_resource(self.adjacency.clone());
        world.insert_resource(ProvinceStore::from_vec(self.provinces.clone()));
        world.insert_resource(NationStore::from_vec(self.nations.clone()));

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

        // Tier 1 gameplay systems.
        if let Some(ref ec) = self.economy_config {
            world.insert_resource(ec.clone());
        }
        if let Some(ref nb) = self.nation_budgets {
            world.insert_resource(nb.clone());
        }
        if let Some(ref gr) = self.goods_registry {
            world.insert_resource(gr.clone());
        }
        if let Some(ref pe) = self.province_economy {
            world.insert_resource(pe.clone());
        }
        if let Some(ref tn) = self.trade_network {
            world.insert_resource(tn.clone());
        }
        if let Some(ref dc) = self.diplomacy_config {
            world.insert_resource(dc.clone());
        }
        if let Some(ref dr) = self.diplomatic_relations {
            world.insert_resource(dr.clone());
        }
        if let Some(ref wr) = self.war_registry {
            world.insert_resource(wr.clone());
        }
        if let Some(ref cm) = self.combat_model {
            world.insert_resource(cm.clone());
        }
        if let Some(ref utr) = self.unit_type_registry {
            world.insert_resource(utr.clone());
        }
        if let Some(ref crl) = self.combat_result_log {
            world.insert_resource(crl.clone());
        }
        if let Some(ref pc) = self.population_config {
            world.insert_resource(pc.clone());
        }
        if let Some(ref pp) = self.province_pops {
            world.insert_resource(pp.clone());
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::world::World;
    use crate::world::{MapLayout, ScopeId, WorldBuilder};

    #[test]
    fn map_file_roundtrip_bincode() {
        let mut world = World::new();
        WorldBuilder::new()
            .provinces(4)
            .nations(2)
            .map_size(2, 2)
            .build(&mut world);

        let mf = MapFile::from_world(&mut world).unwrap();
        assert_eq!(mf.version, 3);
        assert_eq!(mf.provinces.len(), 4);
        assert_eq!(mf.nations.len(), 2);

        let mut buf = Vec::new();
        mf.write(&mut buf).unwrap();
        let mf2 = MapFile::read(&mut &buf[..]).unwrap();
        assert_eq!(mf2.provinces.len(), 4);
        assert_eq!(mf2.nations.len(), 2);
    }

    #[test]
    fn map_file_roundtrip_json() {
        let mut world = World::new();
        WorldBuilder::new()
            .provinces(3)
            .nations(1)
            .map_size(3, 1)
            .build(&mut world);

        let mf = MapFile::from_world(&mut world).unwrap();
        let mut buf = Vec::new();
        mf.write_json(&mut buf).unwrap();
        let mf2 = MapFile::read_json(&mut &buf[..]).unwrap();
        assert_eq!(mf2.provinces.len(), 3);
    }

    #[test]
    fn map_file_apply_to_world() {
        let mut world = World::new();
        WorldBuilder::new()
            .provinces(4)
            .nations(2)
            .map_size(2, 2)
            .build(&mut world);

        let mf = MapFile::from_world(&mut world).unwrap();

        let mut world2 = World::new();
        mf.apply_to_world(&mut world2);
        let store = world2.get_resource::<ProvinceStore>().unwrap();
        assert_eq!(store.len(), 4);
        let nstore = world2.get_resource::<NationStore>().unwrap();
        assert_eq!(nstore.len(), 2);
    }

    #[test]
    fn compute_adjacency_from_square_layout() {
        // 2x2 grid: provinces 1,2,3,4
        let layout = MapLayout {
            width: 2,
            height: 2,
            tiles: vec![1, 2, 3, 4],
        };
        let adj = compute_adjacency_from_layout(&layout, 4);

        // Province 1 (0,0) should be adjacent to 2 (1,0) and 3 (0,1)
        let n1 = adj.get(ProvinceId::from_raw(1));
        assert!(n1.contains(&2));
        assert!(n1.contains(&3));
        assert!(!n1.contains(&4));
    }

    #[test]
    fn map_file_from_world_requires_resources() {
        let mut w = World::new();
        let result = MapFile::from_world(&mut w);
        assert!(result.is_none());
    }
}
