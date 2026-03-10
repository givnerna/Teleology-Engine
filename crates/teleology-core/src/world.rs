//! Game world: ECS World + SoA-style bulk data for grand strategy.
//!
//! Provinces and nations use dense, cache-friendly storage so we can
//! iterate over thousands of entities without pointer chasing.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use std::num::NonZeroU32;

use crate::archetypes::{Nation, Province, ScopeEntity};

/// Trait for scope ids (ProvinceId, NationId) so generic systems can work with either.
pub trait ScopeId: Clone + Copy + PartialEq + Eq + std::hash::Hash + Send + Sync + 'static {
    /// Dense 0-based index for array access.
    fn index(self) -> usize;
    /// Raw 1-based id value.
    fn raw(self) -> u32;
    /// Construct from a raw 1-based id. Panics if raw == 0.
    fn from_raw(raw: u32) -> Self;
}

// NOTE: Province and Nation scopes are hand-written reference implementations.
// For custom scopes, use `register_scope!` which generates all of this automatically.

/// Stable id for a province (map slot). Dense index into province arrays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProvinceId(pub NonZeroU32);

impl ProvinceId {
    #[inline]
    pub fn index(self) -> usize {
        (self.0.get() - 1) as usize
    }
}

impl ScopeId for ProvinceId {
    #[inline]
    fn index(self) -> usize { (self.0.get() - 1) as usize }
    #[inline]
    fn raw(self) -> u32 { self.0.get() }
    #[inline]
    fn from_raw(raw: u32) -> Self { ProvinceId(NonZeroU32::new(raw).unwrap()) }
}

/// Stable id for a nation/tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NationId(pub NonZeroU32);

impl NationId {
    #[inline]
    pub fn index(self) -> usize {
        (self.0.get() - 1) as usize
    }
}

impl ScopeId for NationId {
    #[inline]
    fn index(self) -> usize { (self.0.get() - 1) as usize }
    #[inline]
    fn raw(self) -> u32 { self.0.get() }
    #[inline]
    fn from_raw(raw: u32) -> Self { NationId(NonZeroU32::new(raw).unwrap()) }
}

/// Entity handle for units (can be many; use ECS or secondary SoA).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UnitId(pub u64);

/// Total counts fixed at world init (grand strategy: province count is static).
#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct WorldBounds {
    pub province_count: u32,
    pub nation_count: u32,
}

/// 2D map layout for the editor: each tile (x, y) has a province index (1-based; 0 = no province).
#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct MapLayout {
    pub width: u32,
    pub height: u32,
    /// Index = y * width + x; value = province raw id (0 = empty).
    pub tiles: Vec<u32>,
}

impl MapLayout {
    pub fn new(width: u32, height: u32) -> Self {
        let len = (width as usize).saturating_mul(height as usize);
        Self {
            width,
            height,
            tiles: vec![0; len],
        }
    }

    #[inline]
    pub fn index(&self, x: u32, y: u32) -> usize {
        (y as usize) * (self.width as usize) + (x as usize)
    }

    pub fn get(&self, x: u32, y: u32) -> u32 {
        let i = self.index(x, y);
        self.tiles.get(i).copied().unwrap_or(0)
    }

    pub fn set(&mut self, x: u32, y: u32, province_raw: u32) {
        let i = self.index(x, y);
        if i < self.tiles.len() {
            self.tiles[i] = province_raw;
        }
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }
}

/// Hex grid layout: axial coordinates (q, r). Index = r * width + q; value = province raw id (0 = empty).
#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct HexMapLayout {
    pub width: u32,
    pub height: u32,
    /// Index = r * width + q; value = province raw id (0 = empty).
    pub tiles: Vec<u32>,
}

impl HexMapLayout {
    pub fn new(width: u32, height: u32) -> Self {
        let len = (width as usize).saturating_mul(height as usize);
        Self {
            width,
            height,
            tiles: vec![0; len],
        }
    }

    #[inline]
    pub fn index(&self, q: u32, r: u32) -> usize {
        (r as usize) * (self.width as usize) + (q as usize)
    }

    pub fn get(&self, q: u32, r: u32) -> u32 {
        let i = self.index(q, r);
        self.tiles.get(i).copied().unwrap_or(0)
    }

    pub fn set(&mut self, q: u32, r: u32, province_raw: u32) {
        let i = self.index(q, r);
        if i < self.tiles.len() {
            self.tiles[i] = province_raw;
        }
    }

    pub fn hex_count(&self) -> usize {
        self.tiles.len()
    }

    /// Six axial neighbors: (q+1,r), (q+1,r-1), (q,r-1), (q-1,r), (q-1,r+1), (q,r+1).
    pub fn neighbors(&self, q: u32, r: u32) -> [(u32, u32); 6] {
        [
            (q + 1, r),
            (q + 1, r.saturating_sub(1)),
            (q, r.saturating_sub(1)),
            (q.saturating_sub(1), r),
            (q.saturating_sub(1), r + 1),
            (q, r + 1),
        ]
    }
}

/// One province as a closed polygon (vertices in order; first and last implicitly connected).
#[derive(Clone, Serialize, Deserialize)]
pub struct ProvincePolygon {
    pub province_id: u32,
    /// Vertices in world coordinates (e.g. longitude/latitude or arbitrary 2D).
    pub vertices: Vec<[f64; 2]>,
}

/// Irregular, vector-based map: provinces as polygons. No grid; adjacency from shared edges.
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct VectorMapLayout {
    pub polygons: Vec<ProvincePolygon>,
}

impl VectorMapLayout {
    pub fn new() -> Self {
        Self {
            polygons: Vec::new(),
        }
    }

    pub fn add(&mut self, province_id: u32, vertices: Vec<[f64; 2]>) {
        if province_id != 0 && !vertices.is_empty() {
            self.polygons.push(ProvincePolygon {
                province_id,
                vertices,
            });
        }
    }
}

/// Map layout kind: square grid, hex grid, or irregular vector provinces.
#[derive(Resource, Clone, Serialize, Deserialize)]
pub enum MapKind {
    Square(MapLayout),
    Hex(HexMapLayout),
    Irregular(VectorMapLayout),
}

impl MapKind {
    pub fn square(width: u32, height: u32) -> Self {
        MapKind::Square(MapLayout::new(width, height))
    }

    pub fn hex(width: u32, height: u32) -> Self {
        MapKind::Hex(HexMapLayout::new(width, height))
    }

    pub fn irregular() -> Self {
        MapKind::Irregular(VectorMapLayout::new())
    }
}

/// Province adjacency (Paradox-style): for each province index, list of adjacent province ids.
/// Used for movement, borders, and pathfinding.
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct ProvinceAdjacency {
    /// adjacent[i] = list of ProvinceId (raw) that border province index i (0-based).
    pub adjacent: Vec<Vec<u32>>,
}

impl ProvinceAdjacency {
    pub fn new(province_count: usize) -> Self {
        Self {
            adjacent: vec![Vec::new(); province_count],
        }
    }

    #[inline]
    pub fn get(&self, id: ProvinceId) -> &[u32] {
        self.adjacent
            .get(id.index())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn set(&mut self, id: ProvinceId, neighbors: Vec<u32>) {
        if id.index() < self.adjacent.len() {
            self.adjacent[id.index()] = neighbors;
        }
    }

    pub fn add_neighbor(&mut self, id: ProvinceId, neighbor_raw: u32) {
        if neighbor_raw == 0 {
            return;
        }
        if id.index() < self.adjacent.len() {
            let v = &mut self.adjacent[id.index()];
            if !v.contains(&neighbor_raw) {
                v.push(neighbor_raw);
            }
        }
    }
}

/// Generic dense store for any scope entity (provinces, nations, etc.).
/// Backed by a `Vec<T>` indexed via `Id::index()` for cache-friendly access.
#[derive(Clone, Serialize, Deserialize)]
pub struct ScopedStore<Id: ScopeId, T: Clone> {
    pub items: Vec<T>,
    #[serde(skip)]
    _phantom: PhantomData<Id>,
}

impl<Id: ScopeId, T: ScopeEntity<Id = Id>> ScopedStore<Id, T> {
    /// Create a store pre-filled with `count` default entities (ids 1..=count).
    pub fn new(count: u32) -> Self {
        ScopedStore {
            items: (0..count)
                .map(|i| T::default_for(Id::from_raw(i + 1)))
                .collect(),
            _phantom: PhantomData,
        }
    }
}

impl<Id: ScopeId, T: Clone> ScopedStore<Id, T> {
    /// Wrap an existing Vec as a store.
    pub fn from_vec(items: Vec<T>) -> Self {
        ScopedStore { items, _phantom: PhantomData }
    }

    #[inline]
    pub fn get(&self, id: Id) -> Option<&T> {
        self.items.get(id.index())
    }

    #[inline]
    pub fn get_mut(&mut self, id: Id) -> Option<&mut T> {
        self.items.get_mut(id.index())
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (Id, &T)> {
        self.items.iter().enumerate().map(|(i, item)| {
            (Id::from_raw((i + 1) as u32), item)
        })
    }

    #[inline]
    pub fn push(&mut self, item: T) {
        self.items.push(item);
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Bulk province data.
pub type ProvinceStore = ScopedStore<ProvinceId, Province>;
impl Resource for ScopedStore<ProvinceId, Province> {}

/// Bulk nation data.
pub type NationStore = ScopedStore<NationId, Nation>;
impl Resource for ScopedStore<NationId, Nation> {}

/// What unit of time a single simulation tick represents.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum TickUnit {
    Second,
    Minute,
    Hour,
    Day,
    Week,
    Month,
    Year,
}

impl Default for TickUnit {
    fn default() -> Self { TickUnit::Day }
}

/// Configuration for the simulation time system. Determines what a tick means
/// and when the three schedule tiers (primary/secondary/tertiary) fire.
///
/// ## Presets
/// - `TimeConfig::grand_strategy()` — tick=Day, secondary=Month, tertiary=Year (EU4/CK3)
/// - `TimeConfig::tactical()` — tick=Hour, secondary=Day, tertiary=Month (HoI4-style)
/// - `TimeConfig::realtime()` — tick=Second, secondary=Minute, tertiary=Hour (RTS)
/// - `TimeConfig::civilization()` — tick=Year, secondary=10 years, tertiary=100 years (Civ)
///
/// ## Custom
/// Use `TimeConfig::custom()` to set arbitrary thresholds.
#[derive(Resource, Clone, Debug, Serialize, Deserialize)]
pub struct TimeConfig {
    /// What one simulation tick represents.
    pub tick_unit: TickUnit,
    /// How many ticks until the secondary schedule fires (e.g. 30 days = 1 month).
    pub secondary_every: u32,
    /// How many ticks until the tertiary schedule fires (e.g. 365 days = 1 year).
    pub tertiary_every: u32,
    /// Labels for the three schedule tiers (for UI display).
    pub primary_label: String,
    pub secondary_label: String,
    pub tertiary_label: String,
}

impl Default for TimeConfig {
    fn default() -> Self {
        Self::grand_strategy()
    }
}

impl TimeConfig {
    /// EU4/CK3 style: tick=Day, secondary=Month (~30 days), tertiary=Year (~365 days).
    pub fn grand_strategy() -> Self {
        Self {
            tick_unit: TickUnit::Day,
            secondary_every: 30,
            tertiary_every: 365,
            primary_label: "Daily".into(),
            secondary_label: "Monthly".into(),
            tertiary_label: "Yearly".into(),
        }
    }

    /// HoI4 style: tick=Hour, secondary=Day (24h), tertiary=Month (720h).
    pub fn tactical() -> Self {
        Self {
            tick_unit: TickUnit::Hour,
            secondary_every: 24,
            tertiary_every: 24 * 30,
            primary_label: "Hourly".into(),
            secondary_label: "Daily".into(),
            tertiary_label: "Monthly".into(),
        }
    }

    /// RTS / real-time style: tick=Second, secondary=Minute (60s), tertiary=Hour (3600s).
    pub fn realtime() -> Self {
        Self {
            tick_unit: TickUnit::Second,
            secondary_every: 60,
            tertiary_every: 3600,
            primary_label: "Per Second".into(),
            secondary_label: "Per Minute".into(),
            tertiary_label: "Per Hour".into(),
        }
    }

    /// Civilization style: tick=Year, secondary=Decade (10), tertiary=Century (100).
    pub fn civilization() -> Self {
        Self {
            tick_unit: TickUnit::Year,
            secondary_every: 10,
            tertiary_every: 100,
            primary_label: "Yearly".into(),
            secondary_label: "Per Decade".into(),
            tertiary_label: "Per Century".into(),
        }
    }

    /// Fully custom configuration.
    pub fn custom(
        tick_unit: TickUnit,
        secondary_every: u32,
        tertiary_every: u32,
        labels: [&str; 3],
    ) -> Self {
        Self {
            tick_unit,
            secondary_every,
            tertiary_every,
            primary_label: labels[0].into(),
            secondary_label: labels[1].into(),
            tertiary_label: labels[2].into(),
        }
    }
}

/// Full-precision game time. Tracks time down to the second regardless of tick unit.
/// The tick counter drives schedule thresholds; the calendar fields provide human-readable display.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GameTime {
    pub second: u8,
    pub minute: u8,
    pub hour: u8,
    pub day: u16,
    pub month: u8,
    pub year: i32,
    /// Total ticks elapsed since simulation start. Used for schedule threshold checks.
    pub tick: u64,
}

impl Default for GameTime {
    fn default() -> Self {
        Self { second: 0, minute: 0, hour: 0, day: 1, month: 1, year: 1444, tick: 0 }
    }
}

impl GameTime {
    pub fn new(year: i32, month: u8, day: u16) -> Self {
        Self { second: 0, minute: 0, hour: 0, day, month, year, tick: 0 }
    }

    pub fn with_time(year: i32, month: u8, day: u16, hour: u8, minute: u8, second: u8) -> Self {
        Self { second, minute, hour, day, month, year, tick: 0 }
    }

    /// Total days since epoch for ordering and delta math (backward compatible).
    #[inline]
    pub fn to_days_since_epoch(self) -> i64 {
        let y = self.year as i64;
        let m = self.month as i64;
        let d = self.day as i64;
        (y * 365) + (m * 31) + d
    }

    /// Total seconds since midnight for sub-day comparison.
    #[inline]
    pub fn to_seconds_today(self) -> u32 {
        self.hour as u32 * 3600 + self.minute as u32 * 60 + self.second as u32
    }

    /// Convert to a GameDate (drops sub-day precision) for backward compatibility.
    #[inline]
    pub fn to_date(self) -> GameDate {
        GameDate { day: self.day, month: self.month, year: self.year }
    }
}

/// Current game date (grand strategy: day/month/year).
/// Kept for backward compatibility. Derived from `GameTime` at each tick.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GameDate {
    pub day: u16,
    pub month: u8,
    pub year: i32,
}

impl Default for GameDate {
    fn default() -> Self {
        Self { day: 1, month: 1, year: 1444 }
    }
}

impl GameDate {
    pub fn new(year: i32, month: u8, day: u16) -> Self {
        Self { day, month, year }
    }

    /// Total days since epoch for ordering and delta math.
    #[inline]
    pub fn to_days_since_epoch(self) -> i64 {
        let y = self.year as i64;
        let m = self.month as i64;
        let d = self.day as i64;
        (y * 365) + (m * 31) + d
    }
}

/// Builder for a new game world (province count, nations, etc.).
pub struct WorldBuilder {
    province_count: u32,
    nation_count: u32,
    map_kind: Option<MapKind>,
    time_config: Option<TimeConfig>,
    start_time: Option<GameTime>,
    enable_tags: bool,
    enable_character_gen: bool,
    enable_modifiers: bool,
    enable_events: bool,
    enable_event_bus: bool,
    enable_progress_trees: bool,
    enable_armies: bool,
    enable_economy: bool,
    enable_diplomacy: bool,
    combat_model: Option<crate::combat::CombatModel>,
    enable_population: bool,
    terrain_registry: Option<crate::archetypes::TerrainRegistry>,
}

impl WorldBuilder {
    pub fn new() -> Self {
        Self {
            province_count: 0,
            nation_count: 0,
            map_kind: None,
            time_config: None,
            start_time: None,
            enable_tags: false,
            enable_character_gen: false,
            enable_modifiers: false,
            enable_events: false,
            enable_event_bus: false,
            enable_progress_trees: false,
            enable_armies: false,
            enable_economy: false,
            enable_diplomacy: false,
            combat_model: None,
            enable_population: false,
            terrain_registry: None,
        }
    }

    pub fn provinces(mut self, n: u32) -> Self {
        self.province_count = n;
        self
    }

    pub fn nations(mut self, n: u32) -> Self {
        self.nation_count = n;
        self
    }

    /// Square grid map (pre-filled with contiguous rectangular province blocks).
    pub fn map_size(mut self, width: u32, height: u32) -> Self {
        let mut map = MapLayout::new(width, height);
        let n = self.province_count;
        if n > 0 {
            // Divide the map into a grid of contiguous rectangular blocks,
            // one per province.  This keeps each province's tiles adjacent
            // so painting ownership behaves intuitively.
            let ratio = width as f64 / height as f64;
            let grid_rows = ((n as f64 / ratio).sqrt()).round().max(1.0) as u32;
            let grid_cols = ((n as f64 / grid_rows as f64).ceil()).max(1.0) as u32;
            for y in 0..height {
                for x in 0..width {
                    let col = (x * grid_cols / width).min(grid_cols - 1);
                    let row = (y * grid_rows / height).min(grid_rows - 1);
                    let idx = row * grid_cols + col;
                    map.set(x, y, idx.min(n - 1) + 1);
                }
            }
        }
        self.map_kind = Some(MapKind::Square(map));
        self
    }

    /// Square grid map with all tiles empty (0). Use this to draw provinces on the map first, then assign nations.
    pub fn map_size_empty(mut self, width: u32, height: u32) -> Self {
        self.map_kind = Some(MapKind::Square(MapLayout::new(width, height)));
        self
    }

    /// Hex grid map (axial q,r; pre-filled with contiguous province blocks).
    pub fn map_hex(mut self, width: u32, height: u32) -> Self {
        let mut hex = HexMapLayout::new(width, height);
        let n = self.province_count;
        if n > 0 {
            let ratio = width as f64 / height as f64;
            let grid_rows = ((n as f64 / ratio).sqrt()).round().max(1.0) as u32;
            let grid_cols = ((n as f64 / grid_rows as f64).ceil()).max(1.0) as u32;
            for r in 0..height {
                for q in 0..width {
                    let col = (q * grid_cols / width).min(grid_cols - 1);
                    let row = (r * grid_rows / height).min(grid_rows - 1);
                    let idx = row * grid_cols + col;
                    hex.set(q, r, idx.min(n - 1) + 1);
                }
            }
        }
        self.map_kind = Some(MapKind::Hex(hex));
        self
    }

    /// Hex grid map with all tiles empty (0). Use this to draw provinces on the map first, then assign nations.
    pub fn map_hex_empty(mut self, width: u32, height: u32) -> Self {
        self.map_kind = Some(MapKind::Hex(HexMapLayout::new(width, height)));
        self
    }

    /// Irregular vector map (empty; load from file or add polygons in editor).
    pub fn map_irregular(mut self) -> Self {
        self.map_kind = Some(MapKind::Irregular(VectorMapLayout::new()));
        self
    }

    /// Enable the tag system (TagRegistry + per-province/per-nation tag maps).
    pub fn with_tags(mut self) -> Self {
        self.enable_tags = true;
        self
    }

    /// Enable built-in character generator config (CharacterGenConfig resource).
    pub fn with_character_generator(mut self) -> Self {
        self.enable_character_gen = true;
        self
    }

    /// Enable modifiers (ProvinceModifiers + NationModifiers resources).
    pub fn with_modifiers(mut self) -> Self {
        self.enable_modifiers = true;
        self
    }

    /// Enable pop-up events (EventRegistry + EventQueue + ActiveEvent resources).
    pub fn with_events(mut self) -> Self {
        self.enable_events = true;
        self
    }

    /// Enable the EventBus (publish/poll dev-facing messaging).
    pub fn with_event_bus(mut self) -> Self {
        self.enable_event_bus = true;
        self
    }

    /// Enable progress trees (definitions + per-scope ProgressState).
    pub fn with_progress_trees(mut self) -> Self {
        self.enable_progress_trees = true;
        self
    }

    /// Enable the army system (ArmyRegistry resource).
    pub fn with_armies(mut self) -> Self {
        self.enable_armies = true;
        self
    }

    /// Enable the economy system (EconomyConfig, NationBudgets, GoodsRegistry, ProvinceEconomy).
    pub fn with_economy(mut self) -> Self {
        self.enable_economy = true;
        self
    }

    /// Enable the diplomacy system (DiplomaticRelations, WarRegistry, DiplomacyConfig).
    pub fn with_diplomacy(mut self) -> Self {
        self.enable_diplomacy = true;
        self
    }

    /// Enable combat with the specified model (UnitTypeRegistry, CombatResultLog, model-specific resources).
    pub fn with_combat(mut self, model: crate::combat::CombatModel) -> Self {
        self.combat_model = Some(model);
        self.enable_armies = true; // Combat requires armies.
        self
    }

    /// Enable the population system (ProvincePops, PopulationConfig).
    pub fn with_population(mut self) -> Self {
        self.enable_population = true;
        self
    }

    /// Set the time configuration (tick granularity and schedule thresholds).
    /// If not called, defaults to grand strategy (tick=Day, month/year thresholds).
    pub fn time_config(mut self, config: TimeConfig) -> Self {
        self.time_config = Some(config);
        self
    }

    /// Set the simulation start time. Defaults to 1444-01-01 00:00:00.
    pub fn start_time(mut self, time: GameTime) -> Self {
        self.start_time = Some(time);
        self
    }

    /// Set custom terrain types. If not called, a default registry with Land+Sea is inserted.
    pub fn terrain_registry(mut self, reg: crate::archetypes::TerrainRegistry) -> Self {
        self.terrain_registry = Some(reg);
        self
    }

    pub fn build(self, world: &mut World) {
        world.insert_resource(WorldBounds {
            province_count: self.province_count,
            nation_count: self.nation_count,
        });
        let time = self.start_time.unwrap_or_default();
        world.insert_resource(time);
        world.insert_resource(time.to_date());
        world.insert_resource(self.time_config.unwrap_or_default());
        world.insert_resource(ProvinceStore::new(self.province_count));
        world.insert_resource(NationStore::new(self.nation_count));
        if let Some(mk) = self.map_kind {
            world.insert_resource(mk);
        }
        world.insert_resource(ProvinceAdjacency::new(self.province_count as usize));
        world.insert_resource(self.terrain_registry.unwrap_or_default());

        // Optional, modular systems.
        if self.enable_tags {
            world.insert_resource(crate::tags::TagRegistry::new());
            world.insert_resource(crate::tags::ProvinceTags::default());
            world.insert_resource(crate::tags::NationTags::default());
        }
        if self.enable_character_gen {
            world.insert_resource(crate::character_gen::CharacterGenConfig::default());
        }
        if self.enable_modifiers {
            world.insert_resource(crate::modifiers::ProvinceModifiers::new(
                self.province_count as usize,
            ));
            world.insert_resource(crate::modifiers::NationModifiers::new(
                self.nation_count as usize,
            ));
        }
        if self.enable_events {
            world.insert_resource(crate::events::EventRegistry::new());
            world.insert_resource(crate::events::EventQueue::default());
            world.insert_resource(crate::events::ActiveEvent::default());
        }
        if self.enable_event_bus {
            world.insert_resource(crate::event_bus::EventBus::new());
        }
        if self.enable_progress_trees {
            world.insert_resource(crate::progress_trees::ProgressTrees::new());
            world.insert_resource(crate::progress_trees::ProgressState::new(
                self.nation_count as usize,
                self.province_count as usize,
            ));
        }
        if self.enable_armies {
            world.insert_resource(crate::armies::ArmyRegistry::new());
        }
        if self.enable_economy {
            world.insert_resource(crate::economy::EconomyConfig::default());
            world.insert_resource(crate::economy::NationBudgets::new(self.nation_count as usize));
            world.insert_resource(crate::economy::GoodsRegistry::new());
            world.insert_resource(crate::economy::ProvinceEconomy::new(self.province_count as usize));
            world.insert_resource(crate::economy::TradeNetwork::new());
        }
        if self.enable_diplomacy {
            world.insert_resource(crate::diplomacy::DiplomacyConfig::default());
            world.insert_resource(crate::diplomacy::DiplomaticRelations::new(self.nation_count));
            world.insert_resource(crate::diplomacy::WarRegistry::new());
        }
        if let Some(model) = self.combat_model {
            // Insert model-specific resources based on the selected combat model.
            match &model {
                crate::combat::CombatModel::StackBased(config) => {
                    world.insert_resource(config.clone());
                    world.insert_resource(crate::combat::stack::ActiveStackBattles::default());
                    world.insert_resource(crate::combat::stack::ActiveSieges::default());
                }
                crate::combat::CombatModel::OneUnitPerTile(config) => {
                    world.insert_resource(config.clone());
                }
                crate::combat::CombatModel::Deployment(config) => {
                    world.insert_resource(config.clone());
                    world.insert_resource(crate::combat::deployment::ActiveDeploymentBattles::default());
                }
                crate::combat::CombatModel::TacticalGrid(config) => {
                    world.insert_resource(config.clone());
                    world.insert_resource(crate::combat::tactical::ActiveTacticalBattles::default());
                }
            }
            world.insert_resource(model);
            world.insert_resource(crate::combat::UnitTypeRegistry::new());
            world.insert_resource(crate::combat::CombatResultLog::new());
        }
        if self.enable_population {
            world.insert_resource(crate::population::PopulationConfig::default());
            world.insert_resource(crate::population::ProvincePops::new(self.province_count as usize));
        }
    }
}

impl Default for WorldBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Add one new province to an existing world (e.g. from the map editor).
/// Extends WorldBounds, ProvinceStore, ProvinceAdjacency, and optionally ProvinceModifiers and ProgressState.
/// Returns the new province raw id (1-based).
pub fn add_province_to_world(world: &mut World) -> Option<u32> {
    let mut bounds = world.get_resource_mut::<WorldBounds>()?;
    let new_raw = bounds.province_count + 1;
    bounds.province_count = new_raw;

    let mut store = world.get_resource_mut::<ProvinceStore>()?;
    store.push(Province::default_for(ProvinceId(
        NonZeroU32::new(new_raw).unwrap(),
    )));

    if let Some(mut adj) = world.get_resource_mut::<ProvinceAdjacency>() {
        adj.adjacent.push(Vec::new());
    }
    if let Some(mut pm) = world.get_resource_mut::<crate::modifiers::ProvinceModifiers>() {
        pm.per_scope.push(Vec::new());
    }
    if let Some(mut ps) = world.get_resource_mut::<crate::progress_trees::ProgressState>() {
        ps.per_province.push(std::collections::HashMap::new());
    }

    Some(new_raw)
}

/// Add one new nation to an existing world (e.g. from the map editor).
/// Extends WorldBounds, NationStore, and optionally NationModifiers, DiplomaticRelations, NationBudgets, and ProgressState.
/// Returns the new nation raw id (1-based).
pub fn add_nation_to_world(world: &mut World) -> Option<u32> {
    let mut bounds = world.get_resource_mut::<WorldBounds>()?;
    let new_raw = bounds.nation_count + 1;
    bounds.nation_count = new_raw;

    let mut store = world.get_resource_mut::<NationStore>()?;
    store.push(Nation::default_for(NationId(
        NonZeroU32::new(new_raw).unwrap(),
    )));

    if let Some(mut nm) = world.get_resource_mut::<crate::modifiers::NationModifiers>() {
        nm.per_scope.push(Vec::new());
    }
    if let Some(mut nb) = world.get_resource_mut::<crate::economy::NationBudgets>() {
        nb.budgets.push(crate::economy::BudgetEntry::default());
    }
    if let Some(mut ps) = world.get_resource_mut::<crate::progress_trees::ProgressState>() {
        ps.per_nation.push(std::collections::HashMap::new());
    }

    Some(new_raw)
}

// ---------------------------------------------------------------------------
// Province auto-generation (inspired by opengs-maptool's jittered seeds + BFS)
// ---------------------------------------------------------------------------

/// Simple LCG random number generator (no external dependency needed).
struct SimpleRng(u64);

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_add(1))
    }
    fn next_u32(&mut self) -> u32 {
        // LCG parameters from Numerical Recipes
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    /// Random u32 in [0, max).
    fn next_range(&mut self, max: u32) -> u32 {
        if max == 0 { return 0; }
        self.next_u32() % max
    }
}

/// Auto-generate provinces on a square map using jittered seeds + BFS flood fill.
///
/// Divides the map into a grid, places one seed per cell (jittered), then
/// runs a multi-seed BFS so every tile is assigned to the nearest province.
/// Clears and rebuilds `store` and updates `bounds.province_count`.
pub fn generate_provinces_square(
    map: &mut MapLayout,
    count: u32,
    store: &mut ProvinceStore,
    bounds: &mut WorldBounds,
) {
    use std::collections::VecDeque;

    let w = map.width;
    let h = map.height;
    if count == 0 || w == 0 || h == 0 {
        return;
    }

    // 1. Jittered seeds: divide map into grid
    let cols = ((count as f64).sqrt()).ceil() as u32;
    let rows = ((count as f64 / cols as f64).ceil()) as u32;
    let cell_w = w as f64 / cols as f64;
    let cell_h = h as f64 / rows as f64;

    let mut rng = SimpleRng::new(w as u64 * 31 + h as u64 * 17 + count as u64);
    let mut seeds: Vec<(u32, u32)> = Vec::new();

    for row in 0..rows {
        for col in 0..cols {
            if seeds.len() as u32 >= count {
                break;
            }
            let cx = (col as f64 + 0.5) * cell_w;
            let cy = (row as f64 + 0.5) * cell_h;
            // Jitter within 60% of half-cell
            let jx = (rng.next_range(1000) as f64 / 1000.0 - 0.5) * cell_w * 0.6;
            let jy = (rng.next_range(1000) as f64 / 1000.0 - 0.5) * cell_h * 0.6;
            let sx = ((cx + jx) as u32).min(w - 1);
            let sy = ((cy + jy) as u32).min(h - 1);
            seeds.push((sx, sy));
        }
    }

    let actual_count = seeds.len() as u32;

    // Clear map
    for t in map.tiles.iter_mut() {
        *t = 0;
    }

    // 2. Multi-seed BFS flood fill
    let mut queue: VecDeque<(u32, u32, u32)> = VecDeque::new();
    for (i, &(sx, sy)) in seeds.iter().enumerate() {
        let prov_id = (i as u32) + 1;
        map.set(sx, sy, prov_id);
        queue.push_back((sx, sy, prov_id));
    }

    while let Some((x, y, prov_id)) = queue.pop_front() {
        let neighbors: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
        for (dx, dy) in neighbors {
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                continue;
            }
            let (nx, ny) = (nx as u32, ny as u32);
            if map.get(nx, ny) == 0 {
                map.set(nx, ny, prov_id);
                queue.push_back((nx, ny, prov_id));
            }
        }
    }

    // 3. Rebuild store
    store.items.clear();
    for i in 0..actual_count {
        let id = ProvinceId(NonZeroU32::new(i + 1).unwrap());
        store.items.push(Province::default_for(id));
    }
    bounds.province_count = actual_count;
}

/// Auto-generate provinces on a hex map using jittered seeds + BFS flood fill.
///
/// Same algorithm as square but uses 6-connected hex neighbors.
pub fn generate_provinces_hex(
    map: &mut HexMapLayout,
    count: u32,
    store: &mut ProvinceStore,
    bounds: &mut WorldBounds,
) {
    use std::collections::VecDeque;

    let w = map.width;
    let h = map.height;
    if count == 0 || w == 0 || h == 0 {
        return;
    }

    let cols = ((count as f64).sqrt()).ceil() as u32;
    let rows = ((count as f64 / cols as f64).ceil()) as u32;
    let cell_w = w as f64 / cols as f64;
    let cell_h = h as f64 / rows as f64;

    let mut rng = SimpleRng::new(w as u64 * 31 + h as u64 * 17 + count as u64);
    let mut seeds: Vec<(u32, u32)> = Vec::new();

    for row in 0..rows {
        for col in 0..cols {
            if seeds.len() as u32 >= count {
                break;
            }
            let cx = (col as f64 + 0.5) * cell_w;
            let cy = (row as f64 + 0.5) * cell_h;
            let jx = (rng.next_range(1000) as f64 / 1000.0 - 0.5) * cell_w * 0.6;
            let jy = (rng.next_range(1000) as f64 / 1000.0 - 0.5) * cell_h * 0.6;
            let sq = ((cx + jx) as u32).min(w - 1);
            let sr = ((cy + jy) as u32).min(h - 1);
            seeds.push((sq, sr));
        }
    }

    let actual_count = seeds.len() as u32;

    for t in map.tiles.iter_mut() {
        *t = 0;
    }

    let mut queue: VecDeque<(u32, u32, u32)> = VecDeque::new();
    for (i, &(sq, sr)) in seeds.iter().enumerate() {
        let prov_id = (i as u32) + 1;
        map.set(sq, sr, prov_id);
        queue.push_back((sq, sr, prov_id));
    }

    while let Some((q, r, prov_id)) = queue.pop_front() {
        for (nq, nr) in map.neighbors(q, r) {
            if map.get(nq, nr) == 0 {
                map.set(nq, nr, prov_id);
                queue.push_back((nq, nr, prov_id));
            }
        }
    }

    store.items.clear();
    for i in 0..actual_count {
        let id = ProvinceId(NonZeroU32::new(i + 1).unwrap());
        store.items.push(Province::default_for(id));
    }
    bounds.province_count = actual_count;
}

/// Convenience type for the full game world (ECS + resources).
pub type GameWorld = World;

/// Register a custom scope, generating all required trait and resource implementations.
///
/// # Two-argument form (recommended for SoA-style scopes)
///
/// `register_scope!(FooId, Foo)` generates:
/// - [`ScopeId`] impl for `FooId` (must be a newtype over `NonZeroU32`)
/// - [`Resource`](bevy_ecs::prelude::Resource) impl for `ScopedStore<FooId, Foo>`
/// - [`Resource`](bevy_ecs::prelude::Resource) impl for `ScopedModifiers<FooId>`
/// - [`Resource`](bevy_ecs::prelude::Resource) impl for `ScopedTags<FooId>`
/// - [`Resource`](bevy_ecs::prelude::Resource) impl for `ScopedProgress<FooId>`
///
/// # One-argument form (for ECS-entity scopes without a store)
///
/// `register_scope!(FooId)` generates everything above **except** the
/// `ScopedStore` resource impl. Use this when entities are stored as Bevy ECS
/// entities with components rather than in a dense `ScopedStore`.
///
/// # What you still need to write manually
///
/// Implement [`ScopeEntity`](crate::archetypes::ScopeEntity) for your entity
/// type — each entity has custom fields, so the macro cannot generate this.
///
/// # Full example: defining a City scope
///
/// ```ignore
/// use std::num::NonZeroU32;
/// use serde::{Serialize, Deserialize};
/// use teleology_core::{ScopeEntity, ScopeId};
///
/// // 1. Define your id type (newtype over NonZeroU32).
/// #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// pub struct CityId(pub NonZeroU32);
///
/// // 2. Define your entity struct.
/// #[derive(Clone, Serialize, Deserialize)]
/// pub struct City {
///     pub id: CityId,
///     pub name_id: u32,
///     pub population: u32,
/// }
///
/// // 3. Implement ScopeEntity (manual — your fields, your defaults).
/// impl ScopeEntity for City {
///     type Id = CityId;
///     fn id(&self) -> CityId { self.id }
///     fn default_for(id: CityId) -> Self {
///         Self { id, name_id: 0, population: 0 }
///     }
/// }
///
/// // 4. One macro call generates everything.
/// teleology_core::register_scope!(CityId, City);
///
/// // Now you can use:
/// //   ScopedStore<CityId, City>::new(count)   — dense entity storage
/// //   ScopedModifiers<CityId>::new(count)     — per-city modifiers
/// //   ScopedTags<CityId>::default()           — per-city tags
/// //   ScopedProgress<CityId>::new(count)      — per-city progress trees
/// //
/// // All of these are Bevy Resources — insert into a World:
/// //   world.insert_resource(ScopedStore::<CityId, City>::new(city_count));
/// ```
#[macro_export]
macro_rules! register_scope {
    // Full form: ScopeId + ScopedStore + all subsystem Resources.
    ($id_type:ty, $entity_type:ty) => {
        impl $crate::world::ScopeId for $id_type {
            #[inline]
            fn index(self) -> usize {
                (self.0.get() - 1) as usize
            }
            #[inline]
            fn raw(self) -> u32 {
                self.0.get()
            }
            #[inline]
            fn from_raw(raw: u32) -> Self {
                Self(::std::num::NonZeroU32::new(raw).unwrap())
            }
        }

        impl $crate::Resource for $crate::world::ScopedStore<$id_type, $entity_type> {}
        impl $crate::Resource for $crate::modifiers::ScopedModifiers<$id_type> {}
        impl $crate::Resource for $crate::tags::ScopedTags<$id_type> {}
        impl $crate::Resource for $crate::progress_trees::ScopedProgress<$id_type> {}
    };
    // Minimal form: ScopeId + subsystem Resources only (no ScopedStore).
    // Use for scopes stored as ECS entities rather than in a dense store.
    ($id_type:ty) => {
        impl $crate::world::ScopeId for $id_type {
            #[inline]
            fn index(self) -> usize {
                (self.0.get() - 1) as usize
            }
            #[inline]
            fn raw(self) -> u32 {
                self.0.get()
            }
            #[inline]
            fn from_raw(raw: u32) -> Self {
                Self(::std::num::NonZeroU32::new(raw).unwrap())
            }
        }

        impl $crate::Resource for $crate::modifiers::ScopedModifiers<$id_type> {}
        impl $crate::Resource for $crate::tags::ScopedTags<$id_type> {}
        impl $crate::Resource for $crate::progress_trees::ScopedProgress<$id_type> {}
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archetypes::ScopeEntity;

    // --- Custom scope defined at module level (required for orphan rules). ---
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct CityId(pub NonZeroU32);

    #[derive(Clone, Serialize, Deserialize)]
    pub struct City {
        pub id: CityId,
        pub name_id: u32,
        pub population: u32,
    }

    impl ScopeEntity for City {
        type Id = CityId;
        fn id(&self) -> CityId { self.id }
        fn default_for(id: CityId) -> Self {
            Self { id, name_id: 0, population: 0 }
        }
    }

    // One macro call generates ScopeId + all Resource impls.
    crate::register_scope!(CityId, City);

    #[test]
    fn custom_scope_via_macro() {
        use crate::modifiers::{Modifier, ModifierId, ModifierTypeId, ModifierValue, ScopedModifiers};
        use crate::tags::{ScopedTags, TagId, TagTypeId};
        use crate::progress_trees::{ScopedProgress, TreeId, NodeId};

        // ScopedStore: create, read, mutate.
        let mut store = ScopedStore::<CityId, City>::new(3);
        assert_eq!(store.len(), 3);
        let c1 = CityId::from_raw(1);
        assert_eq!(store.get(c1).unwrap().population, 0);
        store.get_mut(c1).unwrap().population = 5000;
        assert_eq!(store.get(c1).unwrap().population, 5000);

        // Iterate.
        let ids: Vec<u32> = store.iter().map(|(id, _)| id.raw()).collect();
        assert_eq!(ids, vec![1, 2, 3]);

        // ScopedModifiers: add, list, remove.
        let mut mods = ScopedModifiers::<CityId>::new(3);
        let m = Modifier {
            id: ModifierId(NonZeroU32::new(1).unwrap()),
            ty: ModifierTypeId(NonZeroU32::new(1).unwrap()),
            value: ModifierValue::Additive(10.0),
            source_id: 0,
            expires_on: None,
        };
        let mid = mods.add(c1, m);
        assert_eq!(mods.list(c1).len(), 1);
        assert!(mods.remove(c1, mid));
        assert_eq!(mods.list(c1).len(), 0);

        // ScopedTags: set, get.
        let mut tags = ScopedTags::<CityId>::default();
        let ty = TagTypeId(NonZeroU32::new(1).unwrap());
        let tag = TagId(NonZeroU32::new(42).unwrap());
        tags.set(c1, ty, tag);
        assert_eq!(tags.get(c1, ty), Some(tag));

        // ScopedProgress: add progress, unlock.
        let mut progress = ScopedProgress::<CityId>::new(3);
        let tree = TreeId(NonZeroU32::new(1).unwrap());
        let node = NodeId(NonZeroU32::new(1).unwrap());
        progress.add_progress(c1, tree, node, 50.0);
        assert!(!progress.is_unlocked(c1, tree, node));
        progress.unlock(c1, tree, node);
        assert!(progress.is_unlocked(c1, tree, node));

        // All four types work as Bevy Resources.
        let mut world = World::new();
        world.insert_resource(store);
        world.insert_resource(mods);
        world.insert_resource(tags);
        world.insert_resource(progress);
        assert_eq!(
            world.get_resource::<ScopedStore<CityId, City>>().unwrap().len(),
            3
        );
    }

    #[test]
    fn province_id_index() {
        let id = ProvinceId(NonZeroU32::new(1).unwrap());
        assert_eq!(id.index(), 0);
        let id = ProvinceId(NonZeroU32::new(100).unwrap());
        assert_eq!(id.index(), 99);
    }

    #[test]
    fn nation_id_index() {
        let id = NationId(NonZeroU32::new(1).unwrap());
        assert_eq!(id.index(), 0);
    }

    #[test]
    fn game_date_default() {
        let d = GameDate::default();
        assert_eq!(d.day, 1);
        assert_eq!(d.month, 1);
        assert_eq!(d.year, 1444);
    }

    #[test]
    fn map_layout_get_set() {
        let mut map = MapLayout::new(10, 5);
        assert_eq!(map.tile_count(), 50);
        assert_eq!(map.get(0, 0), 0);
        map.set(2, 1, 7);
        assert_eq!(map.get(2, 1), 7);
        assert_eq!(map.index(2, 1), 12);
    }

    #[test]
    fn world_builder_inserts_resources() {
        let mut world = World::new();
        WorldBuilder::new().provinces(5).nations(3).build(&mut world);
        let bounds = world.get_resource::<WorldBounds>().unwrap();
        assert_eq!(bounds.province_count, 5);
        assert_eq!(bounds.nation_count, 3);
        let date = world.get_resource::<GameDate>().unwrap();
        assert_eq!(date.year, 1444);
        let store = world.get_resource::<ProvinceStore>().unwrap();
        assert_eq!(store.len(), 5);
        let nations = world.get_resource::<NationStore>().unwrap();
        assert_eq!(nations.len(), 3);
    }

    #[test]
    fn world_builder_with_map_layout() {
        let mut world = World::new();
        WorldBuilder::new().provinces(2).nations(1).map_size(4, 3).build(&mut world);
        let map_kind = world.get_resource::<MapKind>().unwrap();
        let map = match map_kind {
            MapKind::Square(m) => m,
            _ => panic!("expected Square map"),
        };
        assert_eq!(map.width, 4);
        assert_eq!(map.height, 3);
        assert_eq!(map.tile_count(), 12);
    }
}
