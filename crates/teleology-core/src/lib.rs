//! Teleology — data-oriented engine core for grand strategy games.
//!
//! Design principles:
//! - **SoA where it matters**: Province/Nation/Unit data in struct-of-arrays for cache-friendly iteration.
//! - **Sparse updates**: Systems only touch entities that need this tick (e.g. monthly vs daily).
//! - **Tick granularity**: Day/month/year ticks; batch work by frequency.
//! - **Script-friendly**: Hot data exposed via stable C API for C++ scripting.

pub mod archetypes;
pub mod batch;
pub mod characters;
pub mod character_gen;
pub mod map_file;
pub mod modifiers;
pub mod events;
pub mod event_bus;
pub mod progress_trees;
pub mod armies;
pub mod simulation;
pub mod tags;
pub mod world;

pub use archetypes::{Nation, Province, Unit, TERRAIN_LAND, TERRAIN_SEA};
pub use batch::par_provinces_mut;
pub use character_gen::{
    CharacterGenConfig, CharacterGenerator, DefaultCharacterGenerator, GenContext,
};
pub use characters::{spawn_character, Character, CharacterRole, CharacterStats};
pub use events::{
    queue_event, pull_next_event, ActiveEvent, EventChoice, EventDefinition, EventId, EventInstance,
    EventQueue, EventRegistry, EventScope,
};
pub use event_bus::{
    publish_event, EventBus, EventEnvelope, EventPayload, EventScopeRef, EventTopicId,
};
pub use progress_trees::{
    NodeId, ProgressNode, ProgressState, ProgressTreeDefinition, ProgressTrees, TreeId,
    TreeProgressState,
};
pub use armies::{
    spawn_army, Army, ArmyCommander, ArmyComposition, ArmyId, ArmyRegistry, ArmyStatus, UnitStack,
};
pub use modifiers::{
    apply_modifiers, ArmyModifiers, CharacterModifiers, Modifier, ModifierCalculator, ModifierId,
    ModifierTypeId, ModifierValue, NationModifiers, ProvinceModifiers,
};
pub use map_file::{
    compute_adjacency, compute_adjacency_from_hex, compute_adjacency_from_layout,
    compute_adjacency_from_vector, MapFile,
};
pub use simulation::{SimulationSchedule, TickRate, WorldSimulation};
pub use tags::{NationTags, ProvinceTags, TagDef, TagId, TagRegistry, TagTypeDef, TagTypeId};
pub use world::GameWorld;
pub use world::{
    add_province_to_world, GameDate, HexMapLayout, MapKind, MapLayout, NationId, NationStore,
    ProvinceAdjacency, ProvinceId, ProvincePolygon, ProvinceStore, VectorMapLayout, WorldBounds,
    WorldBuilder,
};
