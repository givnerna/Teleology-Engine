//! Teleology — data-oriented engine core for grand strategy and 4X games.
//!
//! Design principles:
//! - **SoA where it matters**: Province/Nation/Unit data in struct-of-arrays for cache-friendly iteration.
//! - **Sparse updates**: Systems only touch entities that need this tick (e.g. monthly vs daily).
//! - **Tick granularity**: Configurable from seconds to years; batch work by frequency.
//! - **Script-friendly**: Hot data exposed via stable C API for C++ scripting.
//! - **Data-driven**: All gameplay formulas exposed via config resources.

pub mod archetypes;
pub mod batch;
pub mod characters;
pub mod character_gen;
pub mod combat;
pub mod diplomacy;
pub mod economy;
pub mod map_file;
pub mod modifiers;
pub mod events;
pub mod event_bus;
pub mod game_ui;
pub mod raycast;
pub mod population;
pub mod progress_trees;
pub mod armies;
pub mod simulation;
pub mod tags;
pub mod world;

pub use archetypes::{Nation, Province, ScopeEntity, Unit, TERRAIN_LAND, TERRAIN_SEA};
pub use batch::par_provinces_mut;
pub use character_gen::{
    CharacterGenConfig, CharacterGenerator, DefaultCharacterGenerator, GenContext,
};
pub use characters::{spawn_character, Character, CharacterRole, CharacterStats};
pub use combat::{
    BattleSide, CombatModel, CombatResult, CombatResultLog, UnitCategory, UnitTypeDef,
    UnitTypeId, UnitTypeRegistry,
};
pub use diplomacy::{
    Alliance, DiplomacyConfig, DiplomaticRelations, Relations, Truce, War, WarGoal, WarId,
    WarRegistry,
};
pub use economy::{
    BudgetEntry, EconomyConfig, GoodTypeDef, GoodTypeId, GoodsRegistry, NationBudgets,
    ProvinceEconomy, TradeNetwork, TradeNode, TradeNodeId,
};
pub use events::{
    queue_event, pull_next_event, register_builtin_templates, ActiveEvent, EventChoice,
    EventDefinition, EventId, EventInstance, EventPopupStyle, EventQueue, EventRegistry,
    EventScope, EventTemplate, KeywordEntry, KeywordRegistry, PopupAnchor,
};
pub use event_bus::{
    publish_event, EntityScopeRef, EventBus, EventEnvelope, EventPayload, EventScopeRef,
    EventTopicId, scope_types,
};
pub use game_ui::{UiCommand, UiCommandBuffer, UiPrefab, UiPrefabRegistry};
pub use raycast::{
    point_in_polygon, point_to_province_irregular, raycast, screen_to_tile_hex,
    screen_to_tile_square, screen_to_world, tile_distance_hex, tile_distance_square,
    tile_to_world_hex, tile_to_world_square, world_to_screen, RaycastHit, Viewport,
};
pub use population::{check_revolts, PopGroup, PopulationConfig, ProvincePops};
pub use progress_trees::{
    NationProgress, NodeId, ProgressNode, ProgressState, ProgressTreeDefinition, ProgressTrees,
    ProvinceProgress, ScopedProgress, TreeId, TreeProgressState,
};
pub use armies::{
    spawn_army, Army, ArmyCommander, ArmyComposition, ArmyId, ArmyRegistry, ArmyStatus, UnitStack,
};
pub use modifiers::{
    apply_modifiers, ArmyModifiers, CharacterModifiers, Modifier, ModifierCalculator, ModifierId,
    ModifierTypeId, ModifierValue, NationModifiers, ProvinceModifiers, ScopedModifiers,
};
pub use map_file::{
    compute_adjacency, compute_adjacency_from_hex, compute_adjacency_from_layout,
    compute_adjacency_from_vector, MapFile,
};
pub use simulation::{advance_time_in_place, SimulationSchedule, TickRate, WorldSimulation};
pub use tags::{
    NationTags, ProvinceTags, ScopedTags, TagDef, TagId, TagRegistry, TagTypeDef, TagTypeId,
};
pub use world::GameWorld;
pub use world::{
    add_province_to_world, GameDate, GameTime, HexMapLayout, MapKind, MapLayout, NationId,
    NationStore, ProvinceAdjacency, ProvinceId, ProvincePolygon, ProvinceStore, ScopeId,
    ScopedStore, TickUnit, TimeConfig, VectorMapLayout, WorldBounds, WorldBuilder,
};
