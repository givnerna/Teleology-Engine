//! Runtime: world setup, script loading, hot reload, and engine C API implementation.
//! On WebGL, script loading and hot reload are no-ops; simulation runs without C++ scripts.

mod audio;
mod video;

use bevy_ecs::entity::Entity;
use std::cell::UnsafeCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use teleology_core::{
    publish_event, ArmyComposition, ArmyRegistry, EventBus, EventScopeRef,
    ActiveEvent, EventChoice, EventDefinition, EventId, EventPopupStyle, EventQueue,
    EventRegistry, EventScope, EventTemplate, KeywordEntry, KeywordRegistry,
    PopupAnchor, queue_event, register_builtin_templates,
    GameDate, GameWorld, NationId, NationStore, NationTags, ProvinceId, ProvinceStore, ProvinceTags,
    ProgressState, ProgressTrees, SimulationSchedule, TagId, TagRegistry, TagTypeId, WorldBuilder,
    WorldBounds, WorldSimulation,
    UiCommand, UiCommandBuffer, UiPrefabRegistry, Viewport,
    raycast, screen_to_world, world_to_screen, screen_to_tile_square, screen_to_tile_hex,
    tile_distance_square, tile_distance_hex, RaycastHit, MapKind,
    // Diplomacy
    DiplomaticRelations, WarRegistry, DiplomacyConfig, WarGoal, WarId,
    // Economy
    EconomyConfig, NationBudgets, GoodsRegistry, ProvinceEconomy, TradeNetwork,
    // Population
    PopulationConfig, ProvincePops, check_revolts,
    // Modifiers
    ProvinceModifiers, NationModifiers, Modifier, ModifierValue, ModifierId, ModifierTypeId,
    apply_modifiers,
    // Characters
    Character, CharacterRole, CharacterStats, spawn_character,
    // Combat
    CombatModel, CombatResultLog, UnitTypeRegistry, UnitCategory, BattleSide,
};
use teleology_script_api::{
    load_script_api, CArmyId, CGameDate, CGameTime, CNodeId, CNationId, CProvinceId, CTagId,
    CTagTypeId, CTreeId, TeleologyEngine, TeleologyScriptApi, ScriptHandle,
};
use std::num::NonZeroU32;
use std::ffi::CStr;

#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use notify_debouncer_mini::{new_debouncer, DebounceEventResult};
#[cfg(not(target_arch = "wasm32"))]
use notify::RecursiveMode;

/// Input state: last click and key state for script polling / callbacks.
#[derive(Default)]
pub struct InputState {
    /// Last click position (screen/logical coords). Cleared on next click.
    pub last_click: Option<(f32, f32)>,
    /// Keys currently held down.
    pub keys_down: HashSet<u32>,
    /// Keys that went down this frame (for OnKeyDown callbacks). Cleared after deliver.
    pub keys_just_pressed: HashSet<u32>,
    /// Keys that went up this frame (for OnKeyUp callbacks). Cleared after deliver.
    pub keys_just_released: HashSet<u32>,
}

/// Engine context: world + script API. Passed as TeleologyEngine* to scripts.
/// On WebGL, script fields are unused; simulation runs without C++ scripts.
pub struct EngineContext {
    pub world: UnsafeCell<GameWorld>,
    script_lib: Option<ScriptHandle>,
    script_api: Option<TeleologyScriptApi>,
    script_path: Option<PathBuf>,
    /// Input state: fed by host (editor), polled by scripts or delivered as callbacks.
    pub input: InputState,
    #[cfg(not(target_arch = "wasm32"))]
    audio: audio::AudioSystem,
    video: video::VideoPlayer,
    #[cfg(not(target_arch = "wasm32"))]
    hot_reload_enabled: bool,
    #[cfg(not(target_arch = "wasm32"))]
    reload_pending: Arc<AtomicBool>,
    #[cfg(not(target_arch = "wasm32"))]
    _watcher: Option<notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>>,
}

impl EngineContext {
    pub fn new() -> Self {
        let mut world = GameWorld::new();
        WorldBuilder::new()
            .provinces(100)
            .nations(20)
            .map_size(20, 10)
            .build(&mut world);
        SimulationSchedule::build(&mut world);
        world.insert_resource(UiCommandBuffer::new());
        world.insert_resource(Viewport {
            base_cell: 14.0,
            zoom: 1.0,
            ..Viewport::default()
        });
        Self {
            world: UnsafeCell::new(world),
            script_lib: None,
            script_api: None,
            script_path: None,
            input: InputState::default(),
            #[cfg(not(target_arch = "wasm32"))]
            audio: audio::AudioSystem::new(),
            video: video::VideoPlayer::new(),
            #[cfg(not(target_arch = "wasm32"))]
            hot_reload_enabled: false,
            #[cfg(not(target_arch = "wasm32"))]
            reload_pending: Arc::new(AtomicBool::new(false)),
            #[cfg(not(target_arch = "wasm32"))]
            _watcher: None,
        }
    }

    pub fn load_script(&mut self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let path = path.to_path_buf();
        let (lib, api) = load_script_api(&path)?;
        self.script_lib = Some(lib);
        self.script_api = Some(api);
        self.script_path = Some(path);
        #[cfg(not(target_arch = "wasm32"))]
        self.start_watcher_if_enabled();
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_hot_reload(&mut self, enabled: bool) {
        self.hot_reload_enabled = enabled;
        if enabled {
            self.start_watcher_if_enabled();
        } else {
            self._watcher = None;
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn set_hot_reload(&mut self, _enabled: bool) {}

    #[cfg(not(target_arch = "wasm32"))]
    pub fn hot_reload_enabled(&self) -> bool {
        self.hot_reload_enabled
    }

    #[cfg(target_arch = "wasm32")]
    pub fn hot_reload_enabled(&self) -> bool {
        false
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start_watcher_if_enabled(&mut self) {
        if !self.hot_reload_enabled {
            return;
        }
        let path = match &self.script_path {
            Some(p) => p.clone(),
            None => return,
        };
        let reload_pending = Arc::clone(&self.reload_pending);
        if let Ok(mut debouncer) = new_debouncer(
            Duration::from_millis(400),
            move |res: DebounceEventResult| {
                if res.is_ok() {
                    reload_pending.store(true, Ordering::Relaxed);
                }
            },
        ) {
            let _ = debouncer.watcher().watch(&path, RecursiveMode::NonRecursive);
            self._watcher = Some(debouncer);
        }
    }

    /// Call once per frame (e.g. from editor). If hot reload detected, reloads the script.
    /// No-op on WebGL.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn try_reload_script(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        if !self.reload_pending.swap(false, Ordering::Relaxed) {
            return Ok(false);
        }
        let path = match self.script_path.as_deref() {
            Some(p) => p,
            None => return Ok(false),
        };
        let (lib, api) = load_script_api(path)?;
        self.script_lib = Some(lib);
        self.script_api = Some(api);
        Ok(true)
    }

    #[cfg(target_arch = "wasm32")]
    pub fn try_reload_script(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(false)
    }

    pub fn tick(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.try_reload_script();
        let world = unsafe { &mut *self.world.get() };
        WorldSimulation::tick(world);

        if let Some(api) = self.script_api {
            let engine = self as *mut EngineContext as *mut TeleologyEngine;
            (api.on_daily_tick)(engine);
        }
    }

    pub fn world(&self) -> &GameWorld {
        unsafe { &*self.world.get() }
    }

    pub fn world_mut(&mut self) -> &mut GameWorld {
        unsafe { &mut *self.world.get() }
    }

    // --- Audio API (native only; no-op on wasm) ---
    pub fn audio_available(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return self.audio.is_available();
        #[cfg(target_arch = "wasm32")]
        return false;
    }

    pub fn audio_play_file(&mut self, path: &Path, looping: bool, volume: f32) -> u32 {
        #[cfg(not(target_arch = "wasm32"))]
        return self.audio.play_file(path, looping, volume);
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (path, looping, volume);
            0
        }
    }

    pub fn audio_set_master_volume(&mut self, volume: f32) {
        #[cfg(not(target_arch = "wasm32"))]
        self.audio.set_master_volume(volume);
        #[cfg(target_arch = "wasm32")]
        {
            let _ = volume;
        }
    }

    // --- Video API (feature-gated) ---
    pub fn video_open(&mut self, path: &Path) -> bool {
        self.video.open(path)
    }

    pub fn video_poll_frame(&mut self) -> Option<video::VideoFrame> {
        self.video.poll_frame()
    }

    // --- Input (fed by host; delivered to script callbacks; polled via C API) ---

    /// Feed a click at (x, y). Call from host (e.g. editor) each frame when primary click occurs.
    pub fn feed_click(&mut self, x: f32, y: f32) {
        self.input.last_click = Some((x, y));
    }

    /// Feed key down. Call from host when a key is pressed.
    pub fn feed_key_down(&mut self, key_code: u32) {
        if self.input.keys_down.insert(key_code) {
            self.input.keys_just_pressed.insert(key_code);
        }
    }

    /// Feed key up. Call from host when a key is released.
    pub fn feed_key_up(&mut self, key_code: u32) {
        if self.input.keys_down.remove(&key_code) {
            self.input.keys_just_released.insert(key_code);
        }
    }

    /// Deliver input events to script callbacks (OnClick, OnKeyDown, OnKeyUp). Call once per frame after feeding.
    pub fn deliver_input_events(&mut self) {
        let api = match self.script_api {
            Some(a) => a,
            None => return,
        };
        let engine = self as *mut EngineContext as *mut TeleologyEngine;

        if let Some((x, y)) = self.input.last_click {
            if let Some(cb) = api.on_click {
                cb(engine, x, y);
            }
        }

        for &key_code in &self.input.keys_just_pressed {
            if let Some(cb) = api.on_key_down {
                cb(engine, key_code);
            }
        }
        for &key_code in &self.input.keys_just_released {
            if let Some(cb) = api.on_key_up {
                cb(engine, key_code);
            }
        }

        self.input.keys_just_pressed.clear();
        self.input.keys_just_released.clear();
    }
}

impl Default for EngineContext {
    fn default() -> Self {
        Self::new()
    }
}

// --- Engine C API (symbols for script DLLs; no-op when called from wasm) ---

fn context_from_engine(engine: *mut TeleologyEngine) -> Option<&'static mut EngineContext> {
    if engine.is_null() {
        return None;
    }
    Some(unsafe { &mut *(engine as *mut EngineContext) })
}

#[no_mangle]
pub extern "C" fn teleology_get_date(engine: *mut TeleologyEngine) -> CGameDate {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return CGameDate::default(),
    };
    let world = unsafe { &*ctx.world.get() };
    world
        .get_resource::<GameDate>()
        .map(|d| CGameDate {
            day: d.day,
            month: d.month,
            year: d.year,
        })
        .unwrap_or_default()
}

#[no_mangle]
pub extern "C" fn teleology_get_time(engine: *mut TeleologyEngine) -> CGameTime {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return CGameTime::default(),
    };
    let world = unsafe { &*ctx.world.get() };
    world
        .get_resource::<teleology_core::GameTime>()
        .map(|t| CGameTime {
            second: t.second,
            minute: t.minute,
            hour: t.hour,
            day: t.day,
            month: t.month,
            year: t.year,
            tick: t.tick,
        })
        .unwrap_or_default()
}

#[no_mangle]
pub extern "C" fn teleology_get_province_count(engine: *mut TeleologyEngine) -> u32 {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return 0,
    };
    let world = unsafe { &*ctx.world.get() };
    world
        .get_resource::<WorldBounds>()
        .map(|b| b.province_count)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn teleology_get_province_owner(engine: *mut TeleologyEngine, province: CProvinceId) -> CNationId {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return CNationId { raw: 0 },
    };
    let world = unsafe { &*ctx.world.get() };
    let Some(pid) = NonZeroU32::new(province.raw) else { return CNationId { raw: 0 } };
    let id = ProvinceId(pid);
    world
        .get_resource::<ProvinceStore>()
        .and_then(|s| s.get(id))
        .and_then(|p| p.owner)
        .map(|n| CNationId { raw: n.0.get() })
        .unwrap_or(CNationId { raw: 0 })
}

#[no_mangle]
pub extern "C" fn teleology_set_province_owner(engine: *mut TeleologyEngine, province: CProvinceId, nation: CNationId) {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return,
    };
    let world = unsafe { &mut *ctx.world.get() };
    let Some(mut store) = world.get_resource_mut::<ProvinceStore>() else { return };
    let Some(pid) = NonZeroU32::new(province.raw) else { return };
    let pid = ProvinceId(pid);
    let owner = NonZeroU32::new(nation.raw).map(NationId);
    if let Some(p) = store.get_mut(pid) {
        p.owner = owner;
    }
}

// --- Tags (optional; lazy-init on first API use) ---

fn ensure_tags(world: &mut GameWorld) {
    if world.get_resource::<TagRegistry>().is_none() {
        world.insert_resource(TagRegistry::new());
        world.insert_resource(ProvinceTags::default());
        world.insert_resource(NationTags::default());
    }
}

#[no_mangle]
pub extern "C" fn teleology_tags_register_type(engine: *mut TeleologyEngine, name: *const std::ffi::c_char) -> CTagTypeId {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return CTagTypeId { raw: 0 } };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_tags(world);
    let Some(mut reg) = world.get_resource_mut::<TagRegistry>() else { return CTagTypeId { raw: 0 } };
    if name.is_null() { return CTagTypeId { raw: 0 } }
    let s = unsafe { CStr::from_ptr(name) }.to_string_lossy().to_string();
    let id = reg.register_type(s);
    CTagTypeId { raw: id.raw() }
}

#[no_mangle]
pub extern "C" fn teleology_tags_register_tag(engine: *mut TeleologyEngine, ty: CTagTypeId, name: *const std::ffi::c_char) -> CTagId {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return CTagId { raw: 0 } };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_tags(world);
    let Some(mut reg) = world.get_resource_mut::<TagRegistry>() else { return CTagId { raw: 0 } };
    let Some(ty_nz) = NonZeroU32::new(ty.raw) else { return CTagId { raw: 0 } };
    if name.is_null() { return CTagId { raw: 0 } }
    let s = unsafe { CStr::from_ptr(name) }.to_string_lossy().to_string();
    let id = reg.register_tag(TagTypeId(ty_nz), s);
    CTagId { raw: id.raw() }
}

#[no_mangle]
pub extern "C" fn teleology_province_get_tag(engine: *mut TeleologyEngine, province: CProvinceId, ty: CTagTypeId) -> CTagId {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return CTagId { raw: 0 } };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_tags(world);
    let Some(pid) = NonZeroU32::new(province.raw) else { return CTagId { raw: 0 } };
    let Some(ty_nz) = NonZeroU32::new(ty.raw) else { return CTagId { raw: 0 } };
    let Some(tags) = world.get_resource::<ProvinceTags>() else { return CTagId { raw: 0 } };
    tags.get(ProvinceId(pid), TagTypeId(ty_nz))
        .map(|t| CTagId { raw: t.raw() })
        .unwrap_or(CTagId { raw: 0 })
}

#[no_mangle]
pub extern "C" fn teleology_province_set_tag(engine: *mut TeleologyEngine, province: CProvinceId, ty: CTagTypeId, tag: CTagId) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_tags(world);
    let Some(pid) = NonZeroU32::new(province.raw) else { return };
    let Some(ty_nz) = NonZeroU32::new(ty.raw) else { return };
    let Some(tag_nz) = NonZeroU32::new(tag.raw) else { return };
    let Some(mut tags) = world.get_resource_mut::<ProvinceTags>() else { return };
    tags.set(ProvinceId(pid), TagTypeId(ty_nz), TagId(tag_nz));
}

#[no_mangle]
pub extern "C" fn teleology_nation_get_tag(engine: *mut TeleologyEngine, nation: CNationId, ty: CTagTypeId) -> CTagId {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return CTagId { raw: 0 } };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_tags(world);
    let Some(nid) = NonZeroU32::new(nation.raw) else { return CTagId { raw: 0 } };
    let Some(ty_nz) = NonZeroU32::new(ty.raw) else { return CTagId { raw: 0 } };
    let Some(tags) = world.get_resource::<NationTags>() else { return CTagId { raw: 0 } };
    tags.get(NationId(nid), TagTypeId(ty_nz))
        .map(|t| CTagId { raw: t.raw() })
        .unwrap_or(CTagId { raw: 0 })
}

#[no_mangle]
pub extern "C" fn teleology_nation_set_tag(engine: *mut TeleologyEngine, nation: CNationId, ty: CTagTypeId, tag: CTagId) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_tags(world);
    let Some(nid) = NonZeroU32::new(nation.raw) else { return };
    let Some(ty_nz) = NonZeroU32::new(ty.raw) else { return };
    let Some(tag_nz) = NonZeroU32::new(tag.raw) else { return };
    let Some(mut tags) = world.get_resource_mut::<NationTags>() else { return };
    tags.set(NationId(nid), TagTypeId(ty_nz), TagId(tag_nz));
}

// --- EventBus (optional; lazy-init on first API use) ---

fn ensure_event_bus(world: &mut GameWorld) {
    if world.get_resource::<EventBus>().is_none() {
        world.insert_resource(EventBus::new());
    }
}

#[no_mangle]
pub extern "C" fn teleology_eventbus_publish(
    engine: *mut TeleologyEngine,
    topic: *const std::ffi::c_char,
    payload_type_id: u32,
    payload: *const u8,
    payload_len: u32,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_bus(world);
    let Some(topic) = (!topic.is_null()).then(|| unsafe { CStr::from_ptr(topic) }) else { return };
    let topic = topic.to_string_lossy();
    let bytes = if payload.is_null() || payload_len == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(payload, payload_len as usize) }.to_vec()
    };
    publish_event(world, &topic, EventScopeRef::global(), payload_type_id, bytes, 0);
}

/// Poll next eventbus message. Returns payload_len (0 if none).
#[no_mangle]
pub extern "C" fn teleology_eventbus_poll(
    engine: *mut TeleologyEngine,
    topic_raw_out: *mut u32,
    payload_type_out: *mut u32,
    payload_out: *mut u8,
    payload_cap: u32,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_bus(world);
    let Some(mut bus) = world.get_resource_mut::<EventBus>() else { return 0 };
    let Some(env) = bus.poll() else { return 0 };
    unsafe {
        if !topic_raw_out.is_null() { *topic_raw_out = env.topic.raw(); }
        if !payload_type_out.is_null() { *payload_type_out = env.payload.payload_type_id; }
    }
    let required = env.payload.bytes.len() as u32;
    if !payload_out.is_null() && payload_cap > 0 {
        let n = (payload_cap as usize).min(env.payload.bytes.len());
        unsafe { std::ptr::copy_nonoverlapping(env.payload.bytes.as_ptr(), payload_out, n); }
    }
    required
}

#[no_mangle]
pub extern "C" fn teleology_eventbus_topic_name(
    engine: *mut TeleologyEngine,
    topic_raw: u32,
    out: *mut std::ffi::c_char,
    out_cap: u32,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_bus(world);
    let Some(bus) = world.get_resource::<EventBus>() else { return 0 };
    let Some(nz) = NonZeroU32::new(topic_raw) else { return 0 };
    let name = bus.topic_name(teleology_core::EventTopicId(nz)).unwrap_or("");
    if out.is_null() || out_cap == 0 { return name.len() as u32; }
    let bytes = name.as_bytes();
    let n = (out_cap as usize).saturating_sub(1).min(bytes.len());
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out as *mut u8, n);
        *out.add(n) = 0;
    }
    name.len() as u32
}

// --- Pop-up events (define, queue, display, choose; lazy-init) ---

fn ensure_event_system(world: &mut GameWorld) {
    if world.get_resource::<EventRegistry>().is_none() {
        world.insert_resource(EventRegistry::new());
        world.insert_resource(EventQueue::default());
        world.insert_resource(ActiveEvent::default());
        world.insert_resource(EventPopupStyle::default());
        // Auto-load keywords.json from the working directory if present.
        let mut kw = KeywordRegistry::default();
        let path = std::path::Path::new("keywords.json");
        if path.exists() {
            let _ = kw.load_from_file(path);
        }
        world.insert_resource(kw);
    }
}

/// Define a new event. Returns the event_id (0 on failure).
#[no_mangle]
pub extern "C" fn teleology_event_define(
    engine: *mut TeleologyEngine,
    title: *const std::ffi::c_char,
    body: *const std::ffi::c_char,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let title = if title.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(title) }.to_string_lossy().into_owned()
    };
    let body = if body.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(body) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<EventRegistry>() else { return 0 };
    let def = EventDefinition {
        id: EventId(NonZeroU32::new(1).unwrap()), // placeholder; insert() reassigns
        title,
        body,
        choices: Vec::new(),
        image: String::new(),
        image_w: 0.0,
        image_h: 0.0,
    };
    reg.insert(def).raw()
}

/// Create a new event from a built-in template. Returns the event_id.
/// template: 0=Notification, 1=BinaryChoice, 2=ThreeWayChoice, 3=Narrative, 4=DiplomaticProposal.
#[no_mangle]
pub extern "C" fn teleology_event_from_template(
    engine: *mut TeleologyEngine,
    template: u32,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let tmpl = match template {
        0 => EventTemplate::Notification,
        1 => EventTemplate::BinaryChoice,
        2 => EventTemplate::ThreeWayChoice,
        3 => EventTemplate::Narrative,
        4 => EventTemplate::DiplomaticProposal,
        _ => return 0,
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<EventRegistry>() else { return 0 };
    reg.insert(tmpl.create()).raw()
}

/// Add a choice to an event. Returns the choice index (0-based), or -1 on failure.
#[no_mangle]
pub extern "C" fn teleology_event_add_choice(
    engine: *mut TeleologyEngine,
    event_id: u32,
    text: *const std::ffi::c_char,
    next_event_id: u32,
) -> i32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return -1 };
    let text = if text.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<EventRegistry>() else { return -1 };
    let Some(def) = reg.events.get_mut(&event_id) else { return -1 };
    let next = NonZeroU32::new(next_event_id).map(EventId);
    let idx = def.choices.len();
    def.choices.push(EventChoice {
        text,
        next_event: next,
        effects_payload: Vec::new(),
    });
    idx as i32
}

/// Set the text of an existing choice. Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn teleology_event_set_choice_text(
    engine: *mut TeleologyEngine,
    event_id: u32,
    choice_idx: u32,
    text: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let text = if text.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<EventRegistry>() else { return 0 };
    let Some(def) = reg.events.get_mut(&event_id) else { return 0 };
    let Some(ch) = def.choices.get_mut(choice_idx as usize) else { return 0 };
    ch.text = text;
    1
}

/// Set the title of an existing event. Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn teleology_event_set_title(
    engine: *mut TeleologyEngine,
    event_id: u32,
    title: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let title = if title.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(title) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<EventRegistry>() else { return 0 };
    let Some(def) = reg.events.get_mut(&event_id) else { return 0 };
    def.title = title;
    1
}

/// Set the body of an existing event. Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn teleology_event_set_body(
    engine: *mut TeleologyEngine,
    event_id: u32,
    body: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let body = if body.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(body) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<EventRegistry>() else { return 0 };
    let Some(def) = reg.events.get_mut(&event_id) else { return 0 };
    def.body = body;
    1
}

/// Set the image for an event definition. The path is relative to the project
/// resources directory. Pass w=0, h=0 to use the image's natural size.
/// Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn teleology_event_set_image(
    engine: *mut TeleologyEngine,
    event_id: u32,
    path: *const std::ffi::c_char,
    w: f32,
    h: f32,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let path = if path.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<EventRegistry>() else { return 0 };
    let Some(def) = reg.events.get_mut(&event_id) else { return 0 };
    def.image = path;
    def.image_w = w;
    def.image_h = h;
    1
}

/// Queue an event for display as a pop-up.
/// scope_type: 0=Global, 1=Province, 2=Nation, 3=Character, 4=Army.
/// scope_raw: entity id for scoped events (0 for global).
#[no_mangle]
pub extern "C" fn teleology_event_queue(
    engine: *mut TeleologyEngine,
    event_id: u32,
    scope_type: u32,
    scope_raw: u32,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let Some(eid) = NonZeroU32::new(event_id) else { return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let scope = EventScope { scope_type, raw: scope_raw, raw_hi: 0 };
    queue_event(world, EventId(eid), scope, Vec::new());
}

/// Get the active event. Returns event_id (0 if no active event).
/// Writes the choice count to choice_count_out.
#[no_mangle]
pub extern "C" fn teleology_event_get_active(
    engine: *mut TeleologyEngine,
    choice_count_out: *mut u32,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    let Some(active) = world.get_resource::<ActiveEvent>() else { return 0 };
    let Some(inst) = &active.current else { return 0 };
    let eid = inst.event_id.raw();
    if !choice_count_out.is_null() {
        let count = world.get_resource::<EventRegistry>()
            .and_then(|r| r.get(inst.event_id))
            .map(|d| d.choices.len() as u32)
            .unwrap_or(0);
        unsafe { *choice_count_out = count; }
    }
    eid
}

/// Get text of the active event (title or body).
/// field: 0=title, 1=body.
/// Writes NUL-terminated string to out, returns full length.
#[no_mangle]
pub extern "C" fn teleology_event_get_text(
    engine: *mut TeleologyEngine,
    field: u32,
    out: *mut std::ffi::c_char,
    out_cap: u32,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    let Some(active) = world.get_resource::<ActiveEvent>() else { return 0 };
    let Some(inst) = &active.current else { return 0 };
    let Some(reg) = world.get_resource::<EventRegistry>() else { return 0 };
    let Some(def) = reg.get(inst.event_id) else { return 0 };
    let text = match field {
        0 => &def.title,
        1 => &def.body,
        _ => return 0,
    };
    if out.is_null() || out_cap == 0 { return text.len() as u32; }
    let bytes = text.as_bytes();
    let n = (out_cap as usize).saturating_sub(1).min(bytes.len());
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out as *mut u8, n);
        *out.add(n) = 0;
    }
    text.len() as u32
}

/// Get choice text for the active event.
/// Writes NUL-terminated string to out, returns full length (0 if invalid).
#[no_mangle]
pub extern "C" fn teleology_event_get_choice_text(
    engine: *mut TeleologyEngine,
    choice_idx: u32,
    out: *mut std::ffi::c_char,
    out_cap: u32,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    let Some(active) = world.get_resource::<ActiveEvent>() else { return 0 };
    let Some(inst) = &active.current else { return 0 };
    let Some(reg) = world.get_resource::<EventRegistry>() else { return 0 };
    let Some(def) = reg.get(inst.event_id) else { return 0 };
    let Some(ch) = def.choices.get(choice_idx as usize) else { return 0 };
    let text = &ch.text;
    if out.is_null() || out_cap == 0 { return text.len() as u32; }
    let bytes = text.as_bytes();
    let n = (out_cap as usize).saturating_sub(1).min(bytes.len());
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out as *mut u8, n);
        *out.add(n) = 0;
    }
    text.len() as u32
}

/// Choose an option for the active event. Clears it and chains to next if set.
/// Returns 1 on success, 0 if no active event or invalid choice.
#[no_mangle]
pub extern "C" fn teleology_event_choose(
    engine: *mut TeleologyEngine,
    choice_idx: u32,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    // Get the active event + definition
    let (inst, next_event) = {
        let Some(active) = world.get_resource::<ActiveEvent>() else { return 0 };
        let Some(inst) = active.current.clone() else { return 0 };
        let Some(reg) = world.get_resource::<EventRegistry>() else { return 0 };
        let Some(def) = reg.get(inst.event_id) else { return 0 };
        let Some(ch) = def.choices.get(choice_idx as usize) else { return 0 };
        (inst, ch.next_event)
    };
    // Clear the active event
    if let Some(mut active) = world.get_resource_mut::<ActiveEvent>() {
        active.current = None;
    }
    // Chain to next event if set
    if let Some(next) = next_event {
        queue_event(world, next, inst.scope, inst.payload);
    }
    1
}

/// Set the pop-up style for the next event shown.
#[no_mangle]
pub extern "C" fn teleology_event_style_reset(engine: *mut TeleologyEngine) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    if let Some(mut style) = world.get_resource_mut::<EventPopupStyle>() {
        *style = EventPopupStyle::default();
    }
}

/// Set pop-up anchor: 0=Center, 1=Fixed(x,y).
#[no_mangle]
pub extern "C" fn teleology_event_style_set_anchor(
    engine: *mut TeleologyEngine,
    anchor: u32,
    x: f32,
    y: f32,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    if let Some(mut style) = world.get_resource_mut::<EventPopupStyle>() {
        style.anchor = match anchor {
            0 => PopupAnchor::Center,
            _ => PopupAnchor::Fixed { x, y },
        };
    }
}

/// Set pop-up background color.
#[no_mangle]
pub extern "C" fn teleology_event_style_set_colors(
    engine: *mut TeleologyEngine,
    bg_r: u8, bg_g: u8, bg_b: u8, bg_a: u8,
    title_r: u8, title_g: u8, title_b: u8, title_a: u8,
    body_r: u8, body_g: u8, body_b: u8, body_a: u8,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    if let Some(mut style) = world.get_resource_mut::<EventPopupStyle>() {
        style.bg_color = [bg_r, bg_g, bg_b, bg_a];
        style.title_color = [title_r, title_g, title_b, title_a];
        style.body_color = [body_r, body_g, body_b, body_a];
    }
}

/// Set pop-up image (shown above body text). Pass NULL/empty to clear.
#[no_mangle]
pub extern "C" fn teleology_event_style_set_image(
    engine: *mut TeleologyEngine,
    path: *const std::ffi::c_char,
    w: f32,
    h: f32,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let path = if path.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    if let Some(mut style) = world.get_resource_mut::<EventPopupStyle>() {
        style.image_path = path;
        style.image_w = w;
        style.image_h = h;
    }
}

/// Set pop-up width (0 = auto) and modal flag.
#[no_mangle]
pub extern "C" fn teleology_event_style_set_layout(
    engine: *mut TeleologyEngine,
    width: f32,
    modal: u8,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    if let Some(mut style) = world.get_resource_mut::<EventPopupStyle>() {
        style.width = width;
        style.modal = modal != 0;
    }
}

/// Register all built-in event templates. Returns 5 event IDs via out pointer.
/// Templates: [0]=Notification, [1]=BinaryChoice, [2]=ThreeWay, [3]=Narrative, [4]=Diplomatic.
#[no_mangle]
pub extern "C" fn teleology_event_register_templates(
    engine: *mut TeleologyEngine,
    ids_out: *mut u32,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<EventRegistry>() else { return };
    let ids = register_builtin_templates(&mut reg);
    if !ids_out.is_null() {
        for (i, id) in ids.iter().enumerate() {
            unsafe { *ids_out.add(i) = id.raw(); }
        }
    }
}

// --- Keyword tooltip system ---

/// Register a keyword with a title and description. When the keyword appears
/// in event text, it will be highlighted and show a tooltip on hover.
/// Returns the keyword index (for later removal), or 0xFFFFFFFF on failure.
#[no_mangle]
pub extern "C" fn teleology_keyword_add(
    engine: *mut TeleologyEngine,
    keyword: *const std::ffi::c_char,
    title: *const std::ffi::c_char,
    description: *const std::ffi::c_char,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return u32::MAX };
    let keyword = if keyword.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(keyword) }.to_string_lossy().into_owned()
    };
    let title = if title.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(title) }.to_string_lossy().into_owned()
    };
    let description = if description.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(description) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<KeywordRegistry>() else { return u32::MAX };
    reg.add(KeywordEntry {
        keyword,
        title,
        description,
        icon: String::new(),
        color: [0, 0, 0, 0],
    }) as u32
}

/// Set the icon (image path) for a keyword. Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn teleology_keyword_set_icon(
    engine: *mut TeleologyEngine,
    index: u32,
    path: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let path = if path.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<KeywordRegistry>() else { return 0 };
    let Some(entry) = reg.entries.get_mut(index as usize) else { return 0 };
    entry.icon = path;
    1
}

/// Set the highlight color (RGBA) for a keyword in text.
/// Pass r=0,g=0,b=0,a=0 to use the default highlight color.
/// Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn teleology_keyword_set_color(
    engine: *mut TeleologyEngine,
    index: u32,
    r: u8, g: u8, b: u8, a: u8,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<KeywordRegistry>() else { return 0 };
    let Some(entry) = reg.entries.get_mut(index as usize) else { return 0 };
    entry.color = [r, g, b, a];
    1
}

/// Remove a keyword by index. Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn teleology_keyword_remove(
    engine: *mut TeleologyEngine,
    index: u32,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<KeywordRegistry>() else { return 0 };
    if reg.remove(index as usize) { 1 } else { 0 }
}

/// Remove all keywords.
#[no_mangle]
pub extern "C" fn teleology_keyword_clear(engine: *mut TeleologyEngine) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    if let Some(mut reg) = world.get_resource_mut::<KeywordRegistry>() {
        reg.clear();
    }
}

/// Get the number of registered keywords.
#[no_mangle]
pub extern "C" fn teleology_keyword_count(engine: *mut TeleologyEngine) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(reg) = world.get_resource::<KeywordRegistry>() else { return 0 };
    reg.entries.len() as u32
}

/// Load keywords from a JSON file, appending to the registry.
/// Returns the number of keywords loaded, or 0xFFFFFFFF on error.
///
/// The JSON should be an array of keyword objects:
/// ```json
/// [
///   {
///     "keyword": "Prestige",
///     "title": "Prestige",
///     "description": "A measure of your realm's renown.",
///     "icon": "icons/prestige.png",
///     "color": [255, 215, 0, 255]
///   }
/// ]
/// ```
#[no_mangle]
pub extern "C" fn teleology_keyword_load_file(
    engine: *mut TeleologyEngine,
    path: *const std::ffi::c_char,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return u32::MAX };
    let path = if path.is_null() { return u32::MAX } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<KeywordRegistry>() else { return u32::MAX };
    match reg.load_from_file(std::path::Path::new(&path)) {
        Ok(n) => n as u32,
        Err(_) => u32::MAX,
    }
}

/// Save current keywords to a JSON file. Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn teleology_keyword_save_file(
    engine: *mut TeleologyEngine,
    path: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let path = if path.is_null() { return 0 } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(reg) = world.get_resource::<KeywordRegistry>() else { return 0 };
    match reg.save_to_file(std::path::Path::new(&path)) {
        Ok(()) => 1,
        Err(_) => 0,
    }
}

// --- Progress trees (nation scope; optional; lazy-init on first API use) ---

fn ensure_progress_trees(world: &mut GameWorld) {
    if world.get_resource::<ProgressTrees>().is_none() {
        if let Some(b) = world.get_resource::<WorldBounds>().cloned() {
            world.insert_resource(ProgressTrees::new());
            world.insert_resource(ProgressState::new(
                b.nation_count as usize,
                b.province_count as usize,
            ));
        }
    }
}

#[no_mangle]
pub extern "C" fn teleology_progress_unlock_nation(engine: *mut TeleologyEngine, nation: CNationId, tree: CTreeId, node: CNodeId) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_progress_trees(world);
    let Some(mut state) = world.get_resource_mut::<ProgressState>() else { return };
    let Some(nid) = NonZeroU32::new(nation.raw) else { return };
    let Some(tid) = NonZeroU32::new(tree.raw) else { return };
    let Some(nod) = NonZeroU32::new(node.raw) else { return };
    state.unlock_nation(NationId(nid), teleology_core::TreeId(tid), teleology_core::NodeId(nod));
}

#[no_mangle]
pub extern "C" fn teleology_progress_is_unlocked_nation(engine: *mut TeleologyEngine, nation: CNationId, tree: CTreeId, node: CNodeId) -> bool {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return false };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_progress_trees(world);
    let Some(state) = world.get_resource::<ProgressState>() else { return false };
    let Some(nid) = NonZeroU32::new(nation.raw) else { return false };
    let Some(tid) = NonZeroU32::new(tree.raw) else { return false };
    let Some(nod) = NonZeroU32::new(node.raw) else { return false };
    state.is_unlocked_nation(NationId(nid), teleology_core::TreeId(tid), teleology_core::NodeId(nod))
}

// --- Armies (minimal; optional) ---

#[no_mangle]
pub extern "C" fn teleology_spawn_army(engine: *mut TeleologyEngine, owner: CNationId, location: CProvinceId) -> CArmyId {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return CArmyId { raw: 0 } };
    let world = unsafe { &mut *ctx.world.get() };
    if world.get_resource::<ArmyRegistry>().is_none() {
        world.insert_resource(ArmyRegistry::new());
    }
    let Some(own) = NonZeroU32::new(owner.raw) else { return CArmyId { raw: 0 } };
    let Some(loc) = NonZeroU32::new(location.raw) else { return CArmyId { raw: 0 } };
    let id = teleology_core::spawn_army(world, NationId(own), ProvinceId(loc), ArmyComposition::default());
    CArmyId { raw: id.raw() }
}

#[no_mangle]
pub extern "C" fn teleology_set_army_location(engine: *mut TeleologyEngine, army: CArmyId, location: CProvinceId) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    let Some(reg) = world.get_resource_mut::<ArmyRegistry>() else { return };
    let Some(aid) = NonZeroU32::new(army.raw) else { return };
    let Some(loc) = NonZeroU32::new(location.raw) else { return };
    let id = teleology_core::ArmyId(aid);
    let Some(e) = reg.get_entity(id) else { return };
    if let Some(mut a) = world.get_mut::<teleology_core::Army>(e) {
        a.location = ProvinceId(loc);
    }
}

// --- Input (polling; callbacks are invoked by deliver_input_events) ---

/// Returns 1 if there was a click (and writes x, y to out); 0 otherwise.
#[no_mangle]
pub extern "C" fn teleology_input_last_click(
    engine: *mut TeleologyEngine,
    x_out: *mut f32,
    y_out: *mut f32,
) -> i32 {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return 0,
    };
    let Some((x, y)) = ctx.input.last_click else { return 0 };
    if !x_out.is_null() {
        unsafe { *x_out = x };
    }
    if !y_out.is_null() {
        unsafe { *y_out = y };
    }
    1
}

/// Returns 1 if the key is currently down; 0 otherwise.
#[no_mangle]
pub extern "C" fn teleology_input_key_down(engine: *mut TeleologyEngine, key_code: u32) -> i32 {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return 0,
    };
    if ctx.input.keys_down.contains(&key_code) {
        1
    } else {
        0
    }
}

// --- Game UI (immediate-mode command buffer; scripts push commands, host renders) ---

fn ensure_ui_buffer(world: &mut GameWorld) {
    if world.get_resource::<UiCommandBuffer>().is_none() {
        world.insert_resource(UiCommandBuffer::new());
    }
}

fn push_ui_cmd(engine: *mut TeleologyEngine, cmd: UiCommand) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_ui_buffer(world);
    if let Some(mut buf) = world.get_resource_mut::<UiCommandBuffer>() {
        buf.push(cmd);
    }
}

#[no_mangle]
pub extern "C" fn teleology_ui_begin_window(
    engine: *mut TeleologyEngine,
    title: *const std::ffi::c_char,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    let title = if title.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(title) }.to_string_lossy().into_owned()
    };
    push_ui_cmd(engine, UiCommand::BeginWindow { title, x, y, w, h });
}

#[no_mangle]
pub extern "C" fn teleology_ui_end_window(engine: *mut TeleologyEngine) {
    push_ui_cmd(engine, UiCommand::EndWindow);
}

#[no_mangle]
pub extern "C" fn teleology_ui_begin_horizontal(engine: *mut TeleologyEngine) {
    push_ui_cmd(engine, UiCommand::BeginHorizontal);
}

#[no_mangle]
pub extern "C" fn teleology_ui_end_horizontal(engine: *mut TeleologyEngine) {
    push_ui_cmd(engine, UiCommand::EndHorizontal);
}

#[no_mangle]
pub extern "C" fn teleology_ui_begin_vertical(engine: *mut TeleologyEngine) {
    push_ui_cmd(engine, UiCommand::BeginVertical);
}

#[no_mangle]
pub extern "C" fn teleology_ui_end_vertical(engine: *mut TeleologyEngine) {
    push_ui_cmd(engine, UiCommand::EndVertical);
}

#[no_mangle]
pub extern "C" fn teleology_ui_label(engine: *mut TeleologyEngine, text: *const std::ffi::c_char) {
    let text = if text.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
    };
    push_ui_cmd(engine, UiCommand::Label { text, font_size: 0.0 });
}

#[no_mangle]
pub extern "C" fn teleology_ui_label_sized(
    engine: *mut TeleologyEngine,
    text: *const std::ffi::c_char,
    font_size: f32,
) {
    let text = if text.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
    };
    push_ui_cmd(engine, UiCommand::Label { text, font_size });
}

#[no_mangle]
pub extern "C" fn teleology_ui_button(
    engine: *mut TeleologyEngine,
    id: u32,
    text: *const std::ffi::c_char,
) {
    let text = if text.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
    };
    push_ui_cmd(engine, UiCommand::Button { id, text });
}

#[no_mangle]
pub extern "C" fn teleology_ui_button_was_clicked(engine: *mut TeleologyEngine, id: u32) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    let Some(buf) = world.get_resource::<UiCommandBuffer>() else { return 0 };
    if buf.was_clicked(id) { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn teleology_ui_progress_bar(
    engine: *mut TeleologyEngine,
    fraction: f32,
    text: *const std::ffi::c_char,
    width: f32,
) {
    let text = if text.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
    };
    push_ui_cmd(engine, UiCommand::ProgressBar { fraction, text, w: width });
}

#[no_mangle]
pub extern "C" fn teleology_ui_image(
    engine: *mut TeleologyEngine,
    path: *const std::ffi::c_char,
    w: f32,
    h: f32,
) {
    let path = if path.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    push_ui_cmd(engine, UiCommand::Image { path, w, h });
}

#[no_mangle]
pub extern "C" fn teleology_ui_separator(engine: *mut TeleologyEngine) {
    push_ui_cmd(engine, UiCommand::Separator);
}

#[no_mangle]
pub extern "C" fn teleology_ui_spacing(engine: *mut TeleologyEngine, amount: f32) {
    push_ui_cmd(engine, UiCommand::Spacing { amount });
}

#[no_mangle]
pub extern "C" fn teleology_ui_set_color(
    engine: *mut TeleologyEngine,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) {
    push_ui_cmd(engine, UiCommand::SetColor { r, g, b, a });
}

#[no_mangle]
pub extern "C" fn teleology_ui_set_font_size(engine: *mut TeleologyEngine, size: f32) {
    push_ui_cmd(engine, UiCommand::SetFontSize { size });
}

// --- UI Prefabs (reusable templates; record, instantiate, save/load) ---

fn ensure_prefab_registry(world: &mut GameWorld) {
    if world.get_resource::<UiPrefabRegistry>().is_none() {
        world.insert_resource(UiPrefabRegistry::new());
    }
}

/// Begin recording UI commands into a named prefab. Subsequent teleology_ui_* calls
/// go into the recording buffer instead of the render buffer, until prefab_end is called.
#[no_mangle]
pub extern "C" fn teleology_ui_prefab_begin(
    engine: *mut TeleologyEngine,
    name: *const std::ffi::c_char,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let name = if name.is_null() {
        return;
    } else {
        unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_ui_buffer(world);
    if let Some(mut buf) = world.get_resource_mut::<UiCommandBuffer>() {
        buf.begin_recording(&name);
    }
}

/// End recording and store the prefab in the registry.
#[no_mangle]
pub extern "C" fn teleology_ui_prefab_end(engine: *mut TeleologyEngine) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_ui_buffer(world);
    ensure_prefab_registry(world);
    let prefab = {
        let Some(mut buf) = world.get_resource_mut::<UiCommandBuffer>() else { return };
        buf.end_recording()
    };
    if let Some(prefab) = prefab {
        if let Some(mut reg) = world.get_resource_mut::<UiPrefabRegistry>() {
            reg.insert(prefab);
        }
    }
}

/// Instantiate a prefab by name, expanding placeholder parameters into the render buffer.
/// params is a NUL-separated, double-NUL-terminated string: "Gold\0100\0\0"
/// Returns 1 on success, 0 if prefab not found.
#[no_mangle]
pub extern "C" fn teleology_ui_prefab_instantiate(
    engine: *mut TeleologyEngine,
    name: *const std::ffi::c_char,
    params: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let name = if name.is_null() {
        return 0;
    } else {
        unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned()
    };
    // Parse NUL-separated params
    let param_strs: Vec<String> = if params.is_null() {
        Vec::new()
    } else {
        let mut result = Vec::new();
        let mut ptr = params;
        loop {
            let s = unsafe { CStr::from_ptr(ptr) };
            let bytes = s.to_bytes();
            if bytes.is_empty() {
                break;
            }
            result.push(s.to_string_lossy().into_owned());
            ptr = unsafe { ptr.add(bytes.len() + 1) }; // skip past NUL
        }
        result
    };
    let param_refs: Vec<&str> = param_strs.iter().map(String::as_str).collect();

    let world = unsafe { &mut *ctx.world.get() };
    ensure_prefab_registry(world);
    ensure_ui_buffer(world);

    let expanded = {
        let Some(reg) = world.get_resource::<UiPrefabRegistry>() else { return 0 };
        let Some(prefab) = reg.get(&name) else { return 0 };
        prefab.instantiate(&param_refs)
    };
    if let Some(mut buf) = world.get_resource_mut::<UiCommandBuffer>() {
        for cmd in expanded {
            buf.push(cmd);
        }
    }
    1
}

/// Delete a prefab from the registry by name. Returns 1 if removed, 0 if not found.
#[no_mangle]
pub extern "C" fn teleology_ui_prefab_delete(
    engine: *mut TeleologyEngine,
    name: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let name = if name.is_null() {
        return 0;
    } else {
        unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_prefab_registry(world);
    let Some(mut reg) = world.get_resource_mut::<UiPrefabRegistry>() else { return 0 };
    if reg.remove(&name).is_some() { 1 } else { 0 }
}

/// Save a single prefab to a JSON file. Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn teleology_ui_prefab_save(
    engine: *mut TeleologyEngine,
    name: *const std::ffi::c_char,
    path: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let name = if name.is_null() { return 0 } else {
        unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned()
    };
    let path = if path.is_null() { return 0 } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_prefab_registry(world);
    let Some(reg) = world.get_resource::<UiPrefabRegistry>() else { return 0 };
    if reg.save_prefab(&name, std::path::Path::new(&path)).is_ok() { 1 } else { 0 }
}

/// Load a prefab from a JSON file into the registry. Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn teleology_ui_prefab_load(
    engine: *mut TeleologyEngine,
    path: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let path = if path.is_null() { return 0 } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_prefab_registry(world);
    let Some(mut reg) = world.get_resource_mut::<UiPrefabRegistry>() else { return 0 };
    if reg.load_prefab(std::path::Path::new(&path)).is_ok() { 1 } else { 0 }
}

/// Save all prefabs to a single JSON file. Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn teleology_ui_prefab_save_all(
    engine: *mut TeleologyEngine,
    path: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let path = if path.is_null() { return 0 } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_prefab_registry(world);
    let Some(reg) = world.get_resource::<UiPrefabRegistry>() else { return 0 };
    if reg.save_to_file(std::path::Path::new(&path)).is_ok() { 1 } else { 0 }
}

/// Load all prefabs from a JSON file (replaces registry). Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn teleology_ui_prefab_load_all(
    engine: *mut TeleologyEngine,
    path: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let path = if path.is_null() { return 0 } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    match UiPrefabRegistry::load_from_file(std::path::Path::new(&path)) {
        Ok(reg) => { world.insert_resource(reg); 1 }
        Err(_) => 0,
    }
}

/// Get the number of prefabs in the registry.
#[no_mangle]
pub extern "C" fn teleology_ui_prefab_count(engine: *mut TeleologyEngine) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    world.get_resource::<UiPrefabRegistry>()
        .map(|r| r.prefabs.len() as u32)
        .unwrap_or(0)
}

// --- Raycasting / coordinate conversion (screen ↔ world ↔ tile) ---

/// C-friendly raycast result.
#[repr(C)]
pub struct CRaycastHit {
    pub province_raw: u32,
    pub tile_x: i32,
    pub tile_y: i32,
    pub world_x: f32,
    pub world_y: f32,
}

impl Default for CRaycastHit {
    fn default() -> Self {
        Self { province_raw: 0, tile_x: -1, tile_y: -1, world_x: 0.0, world_y: 0.0 }
    }
}

impl From<RaycastHit> for CRaycastHit {
    fn from(h: RaycastHit) -> Self {
        Self {
            province_raw: h.province_raw,
            tile_x: h.tile_x,
            tile_y: h.tile_y,
            world_x: h.world_x,
            world_y: h.world_y,
        }
    }
}

/// Update the viewport state. Called by the host (editor) each frame.
#[no_mangle]
pub extern "C" fn teleology_viewport_set(
    engine: *mut TeleologyEngine,
    base_cell: f32,
    zoom: f32,
    pan_x: f32,
    pan_y: f32,
    canvas_x: f32,
    canvas_y: f32,
    canvas_w: f32,
    canvas_h: f32,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    if let Some(mut vp) = world.get_resource_mut::<Viewport>() {
        vp.base_cell = base_cell;
        vp.zoom = zoom;
        vp.pan_x = pan_x;
        vp.pan_y = pan_y;
        vp.canvas_x = canvas_x;
        vp.canvas_y = canvas_y;
        vp.canvas_w = canvas_w;
        vp.canvas_h = canvas_h;
    }
}

/// Perform a raycast: screen coordinates → province/tile/world.
#[no_mangle]
pub extern "C" fn teleology_raycast(
    engine: *mut TeleologyEngine,
    screen_x: f32,
    screen_y: f32,
) -> CRaycastHit {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return CRaycastHit::default() };
    let world = unsafe { &*ctx.world.get() };
    let Some(vp) = world.get_resource::<Viewport>() else { return CRaycastHit::default() };
    let Some(mk) = world.get_resource::<MapKind>() else { return CRaycastHit::default() };
    raycast(screen_x, screen_y, vp, mk).into()
}

/// Convert screen coordinates to world space. Writes to x_out, y_out.
#[no_mangle]
pub extern "C" fn teleology_screen_to_world(
    engine: *mut TeleologyEngine,
    screen_x: f32,
    screen_y: f32,
    x_out: *mut f32,
    y_out: *mut f32,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &*ctx.world.get() };
    let Some(vp) = world.get_resource::<Viewport>() else { return };
    let (wx, wy) = screen_to_world(screen_x, screen_y, vp);
    if !x_out.is_null() { unsafe { *x_out = wx }; }
    if !y_out.is_null() { unsafe { *y_out = wy }; }
}

/// Convert world coordinates to screen space. Writes to x_out, y_out.
#[no_mangle]
pub extern "C" fn teleology_world_to_screen(
    engine: *mut TeleologyEngine,
    world_x: f32,
    world_y: f32,
    x_out: *mut f32,
    y_out: *mut f32,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &*ctx.world.get() };
    let Some(vp) = world.get_resource::<Viewport>() else { return };
    let (sx, sy) = world_to_screen(world_x, world_y, vp);
    if !x_out.is_null() { unsafe { *x_out = sx }; }
    if !y_out.is_null() { unsafe { *y_out = sy }; }
}

/// Convert screen coordinates to tile coordinates. Returns 1 if valid, 0 if out of bounds.
/// Writes tile_x, tile_y to out pointers.
#[no_mangle]
pub extern "C" fn teleology_screen_to_tile(
    engine: *mut TeleologyEngine,
    screen_x: f32,
    screen_y: f32,
    tile_x_out: *mut i32,
    tile_y_out: *mut i32,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    let Some(vp) = world.get_resource::<Viewport>() else { return 0 };
    let Some(mk) = world.get_resource::<MapKind>() else { return 0 };
    let result = match mk {
        MapKind::Square(map) => screen_to_tile_square(screen_x, screen_y, vp, map.width, map.height)
            .map(|(x, y)| (x as i32, y as i32)),
        MapKind::Hex(map) => screen_to_tile_hex(screen_x, screen_y, vp, map.width, map.height)
            .map(|(q, r)| (q as i32, r as i32)),
        MapKind::Irregular(_) => {
            let (wx, wy) = screen_to_world(screen_x, screen_y, vp);
            Some((wx as i32, wy as i32))
        }
    };
    match result {
        Some((tx, ty)) => {
            if !tile_x_out.is_null() { unsafe { *tile_x_out = tx }; }
            if !tile_y_out.is_null() { unsafe { *tile_y_out = ty }; }
            1
        }
        None => 0,
    }
}

/// Compute tile distance between two tiles. Uses Chebyshev for square, axial for hex.
/// For irregular maps returns 0 (no grid distance concept).
#[no_mangle]
pub extern "C" fn teleology_tile_distance(
    engine: *mut TeleologyEngine,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    let Some(mk) = world.get_resource::<MapKind>() else { return 0 };
    match mk {
        MapKind::Square(_) => tile_distance_square(x0, y0, x1, y1),
        MapKind::Hex(_) => tile_distance_hex(x0, y0, x1, y1),
        MapKind::Irregular(_) => 0,
    }
}

// ===========================================================================
// Phase 1: Province & Nation extended field accessors
// ===========================================================================

#[no_mangle]
pub extern "C" fn teleology_get_province_terrain(engine: *mut TeleologyEngine, province: CProvinceId) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    let Some(pid) = NonZeroU32::new(province.raw) else { return 0 };
    world.get_resource::<ProvinceStore>()
        .and_then(|s| s.get(ProvinceId(pid)))
        .map(|p| p.terrain)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn teleology_set_province_terrain(engine: *mut TeleologyEngine, province: CProvinceId, terrain: u8) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    let Some(pid) = NonZeroU32::new(province.raw) else { return };
    let Some(mut store) = world.get_resource_mut::<ProvinceStore>() else { return };
    if let Some(p) = store.get_mut(ProvinceId(pid)) { p.terrain = terrain; }
}

#[no_mangle]
pub extern "C" fn teleology_get_province_development(engine: *mut TeleologyEngine, province: CProvinceId, index: u32) -> u16 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    let Some(pid) = NonZeroU32::new(province.raw) else { return 0 };
    world.get_resource::<ProvinceStore>()
        .and_then(|s| s.get(ProvinceId(pid)))
        .and_then(|p| p.development.get(index as usize).copied())
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn teleology_set_province_development(engine: *mut TeleologyEngine, province: CProvinceId, index: u32, value: u16) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    let Some(pid) = NonZeroU32::new(province.raw) else { return };
    let Some(mut store) = world.get_resource_mut::<ProvinceStore>() else { return };
    if let Some(p) = store.get_mut(ProvinceId(pid)) {
        if let Some(slot) = p.development.get_mut(index as usize) { *slot = value; }
    }
}

#[no_mangle]
pub extern "C" fn teleology_get_province_population(engine: *mut TeleologyEngine, province: CProvinceId) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    let Some(pid) = NonZeroU32::new(province.raw) else { return 0 };
    world.get_resource::<ProvinceStore>()
        .and_then(|s| s.get(ProvinceId(pid)))
        .map(|p| p.population)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn teleology_set_province_population(engine: *mut TeleologyEngine, province: CProvinceId, value: u32) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    let Some(pid) = NonZeroU32::new(province.raw) else { return };
    let Some(mut store) = world.get_resource_mut::<ProvinceStore>() else { return };
    if let Some(p) = store.get_mut(ProvinceId(pid)) { p.population = value; }
}

#[no_mangle]
pub extern "C" fn teleology_get_province_occupation(engine: *mut TeleologyEngine, province: CProvinceId) -> CNationId {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return CNationId { raw: 0 } };
    let world = unsafe { &*ctx.world.get() };
    let Some(pid) = NonZeroU32::new(province.raw) else { return CNationId { raw: 0 } };
    world.get_resource::<ProvinceStore>()
        .and_then(|s| s.get(ProvinceId(pid)))
        .and_then(|p| p.occupation)
        .map(|n| CNationId { raw: n.0.get() })
        .unwrap_or(CNationId { raw: 0 })
}

#[no_mangle]
pub extern "C" fn teleology_set_province_occupation(engine: *mut TeleologyEngine, province: CProvinceId, nation: CNationId) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    let Some(pid) = NonZeroU32::new(province.raw) else { return };
    let Some(mut store) = world.get_resource_mut::<ProvinceStore>() else { return };
    if let Some(p) = store.get_mut(ProvinceId(pid)) {
        p.occupation = NonZeroU32::new(nation.raw).map(NationId);
    }
}

#[no_mangle]
pub extern "C" fn teleology_get_nation_count(engine: *mut TeleologyEngine) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    world.get_resource::<WorldBounds>().map(|b| b.nation_count).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn teleology_get_nation_treasury(engine: *mut TeleologyEngine, nation: CNationId) -> i64 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    let Some(nid) = NonZeroU32::new(nation.raw) else { return 0 };
    world.get_resource::<NationStore>()
        .and_then(|s| s.get(NationId(nid)))
        .map(|n| n.treasury)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn teleology_set_nation_treasury(engine: *mut TeleologyEngine, nation: CNationId, value: i64) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    let Some(nid) = NonZeroU32::new(nation.raw) else { return };
    let Some(mut store) = world.get_resource_mut::<NationStore>() else { return };
    if let Some(n) = store.get_mut(NationId(nid)) { n.treasury = value; }
}

#[no_mangle]
pub extern "C" fn teleology_get_nation_stability(engine: *mut TeleologyEngine, nation: CNationId) -> i8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    let Some(nid) = NonZeroU32::new(nation.raw) else { return 0 };
    world.get_resource::<NationStore>()
        .and_then(|s| s.get(NationId(nid)))
        .map(|n| n.stability)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn teleology_set_nation_stability(engine: *mut TeleologyEngine, nation: CNationId, value: i8) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    let Some(nid) = NonZeroU32::new(nation.raw) else { return };
    let Some(mut store) = world.get_resource_mut::<NationStore>() else { return };
    if let Some(n) = store.get_mut(NationId(nid)) { n.stability = value; }
}

#[no_mangle]
pub extern "C" fn teleology_get_nation_prestige(engine: *mut TeleologyEngine, nation: CNationId) -> i32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    let Some(nid) = NonZeroU32::new(nation.raw) else { return 0 };
    world.get_resource::<NationStore>()
        .and_then(|s| s.get(NationId(nid)))
        .map(|n| n.prestige)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn teleology_set_nation_prestige(engine: *mut TeleologyEngine, nation: CNationId, value: i32) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    let Some(nid) = NonZeroU32::new(nation.raw) else { return };
    let Some(mut store) = world.get_resource_mut::<NationStore>() else { return };
    if let Some(n) = store.get_mut(NationId(nid)) { n.prestige = value; }
}

#[no_mangle]
pub extern "C" fn teleology_get_nation_manpower(engine: *mut TeleologyEngine, nation: CNationId) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    let Some(nid) = NonZeroU32::new(nation.raw) else { return 0 };
    world.get_resource::<NationStore>()
        .and_then(|s| s.get(NationId(nid)))
        .map(|n| n.manpower)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn teleology_set_nation_manpower(engine: *mut TeleologyEngine, nation: CNationId, value: u32) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    let Some(nid) = NonZeroU32::new(nation.raw) else { return };
    let Some(mut store) = world.get_resource_mut::<NationStore>() else { return };
    if let Some(n) = store.get_mut(NationId(nid)) { n.manpower = value; }
}

#[no_mangle]
pub extern "C" fn teleology_get_nation_war_exhaustion(engine: *mut TeleologyEngine, nation: CNationId) -> f32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0.0 };
    let world = unsafe { &*ctx.world.get() };
    let Some(nid) = NonZeroU32::new(nation.raw) else { return 0.0 };
    world.get_resource::<NationStore>()
        .and_then(|s| s.get(NationId(nid)))
        .map(|n| n.war_exhaustion)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn teleology_set_nation_war_exhaustion(engine: *mut TeleologyEngine, nation: CNationId, value: f32) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    let Some(nid) = NonZeroU32::new(nation.raw) else { return };
    let Some(mut store) = world.get_resource_mut::<NationStore>() else { return };
    if let Some(n) = store.get_mut(NationId(nid)) { n.war_exhaustion = value; }
}

// ===========================================================================
// Phase 2: Diplomacy
// ===========================================================================

fn ensure_diplomacy(world: &mut GameWorld) {
    if world.get_resource::<DiplomaticRelations>().is_none() {
        let nc = world.get_resource::<WorldBounds>().map(|b| b.nation_count).unwrap_or(0);
        world.insert_resource(DiplomaticRelations::new(nc));
        world.insert_resource(WarRegistry::new());
        world.insert_resource(DiplomacyConfig::default());
    }
}

#[no_mangle]
pub extern "C" fn teleology_diplomacy_get_opinion(engine: *mut TeleologyEngine, a: CNationId, b: CNationId) -> i16 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_diplomacy(world);
    let Some(rel) = world.get_resource::<DiplomaticRelations>() else { return 0 };
    let (Some(an), Some(bn)) = (NonZeroU32::new(a.raw), NonZeroU32::new(b.raw)) else { return 0 };
    rel.get(NationId(an), NationId(bn)).opinion
}

#[no_mangle]
pub extern "C" fn teleology_diplomacy_get_trust(engine: *mut TeleologyEngine, a: CNationId, b: CNationId) -> i16 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_diplomacy(world);
    let Some(rel) = world.get_resource::<DiplomaticRelations>() else { return 0 };
    let (Some(an), Some(bn)) = (NonZeroU32::new(a.raw), NonZeroU32::new(b.raw)) else { return 0 };
    rel.get(NationId(an), NationId(bn)).trust
}

#[no_mangle]
pub extern "C" fn teleology_diplomacy_modify_opinion(engine: *mut TeleologyEngine, a: CNationId, b: CNationId, delta: i16) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_diplomacy(world);
    let (Some(an), Some(bn)) = (NonZeroU32::new(a.raw), NonZeroU32::new(b.raw)) else { return };
    let Some(mut rel) = world.get_resource_mut::<DiplomaticRelations>() else { return };
    rel.modify_opinion(NationId(an), NationId(bn), delta);
}

#[no_mangle]
pub extern "C" fn teleology_diplomacy_modify_trust(engine: *mut TeleologyEngine, a: CNationId, b: CNationId, delta: i16) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_diplomacy(world);
    let (Some(an), Some(bn)) = (NonZeroU32::new(a.raw), NonZeroU32::new(b.raw)) else { return };
    let Some(mut rel) = world.get_resource_mut::<DiplomaticRelations>() else { return };
    rel.modify_trust(NationId(an), NationId(bn), delta);
}

/// Declare war. goal_type: 0=Conquest, 1=Subjugation, 2=Independence, 3=Custom.
/// target_province is used for Conquest; target_nation for Subjugation.
/// Returns WarId raw (0 on failure).
#[no_mangle]
pub extern "C" fn teleology_diplomacy_declare_war(
    engine: *mut TeleologyEngine,
    attacker: CNationId,
    defender: CNationId,
    goal_type: u32,
    target_province: u32,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_diplomacy(world);
    let (Some(att), Some(def)) = (NonZeroU32::new(attacker.raw), NonZeroU32::new(defender.raw)) else { return 0 };
    let date = world.get_resource::<GameDate>().copied().unwrap_or(GameDate { day: 1, month: 1, year: 1 });
    let war_goal = match goal_type {
        0 => WarGoal::Conquest { target_provinces: if target_province > 0 { vec![target_province] } else { Vec::new() } },
        1 => WarGoal::Subjugation { target: NationId(def) },
        2 => WarGoal::Independence,
        _ => WarGoal::Custom { id: goal_type, payload: Vec::new() },
    };
    let Some(mut reg) = world.get_resource_mut::<WarRegistry>() else { return 0 };
    reg.declare_war(NationId(att), NationId(def), war_goal, date).0.get()
}

#[no_mangle]
pub extern "C" fn teleology_diplomacy_end_war(engine: *mut TeleologyEngine, war_id: u32, truce_days: i64) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_diplomacy(world);
    let Some(wid) = NonZeroU32::new(war_id) else { return };
    let date = world.get_resource::<GameDate>().copied().unwrap_or(GameDate { day: 1, month: 1, year: 1 });
    let Some(mut reg) = world.get_resource_mut::<WarRegistry>() else { return };
    reg.end_war(WarId(wid), truce_days, date);
}

#[no_mangle]
pub extern "C" fn teleology_diplomacy_are_at_war(engine: *mut TeleologyEngine, a: CNationId, b: CNationId) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_diplomacy(world);
    let (Some(an), Some(bn)) = (NonZeroU32::new(a.raw), NonZeroU32::new(b.raw)) else { return 0 };
    let Some(reg) = world.get_resource::<WarRegistry>() else { return 0 };
    if reg.are_at_war(NationId(an), NationId(bn)) { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn teleology_diplomacy_get_war_score(engine: *mut TeleologyEngine, war_id: u32) -> i16 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_diplomacy(world);
    let Some(wid) = NonZeroU32::new(war_id) else { return 0 };
    let Some(reg) = world.get_resource::<WarRegistry>() else { return 0 };
    reg.get_war(WarId(wid)).map(|w| w.war_score).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn teleology_diplomacy_set_war_score(engine: *mut TeleologyEngine, war_id: u32, score: i16) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_diplomacy(world);
    let Some(wid) = NonZeroU32::new(war_id) else { return };
    let Some(mut reg) = world.get_resource_mut::<WarRegistry>() else { return };
    if let Some(w) = reg.get_war_mut(WarId(wid)) { w.war_score = score.clamp(-100, 100); }
}

#[no_mangle]
pub extern "C" fn teleology_diplomacy_form_alliance(engine: *mut TeleologyEngine, a: CNationId, b: CNationId) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_diplomacy(world);
    let (Some(an), Some(bn)) = (NonZeroU32::new(a.raw), NonZeroU32::new(b.raw)) else { return };
    let date = world.get_resource::<GameDate>().copied().unwrap_or(GameDate { day: 1, month: 1, year: 1 });
    let Some(mut reg) = world.get_resource_mut::<WarRegistry>() else { return };
    reg.form_alliance(NationId(an), NationId(bn), date);
}

#[no_mangle]
pub extern "C" fn teleology_diplomacy_break_alliance(engine: *mut TeleologyEngine, a: CNationId, b: CNationId) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_diplomacy(world);
    let (Some(an), Some(bn)) = (NonZeroU32::new(a.raw), NonZeroU32::new(b.raw)) else { return };
    let Some(mut reg) = world.get_resource_mut::<WarRegistry>() else { return };
    reg.break_alliance(NationId(an), NationId(bn));
}

#[no_mangle]
pub extern "C" fn teleology_diplomacy_are_allied(engine: *mut TeleologyEngine, a: CNationId, b: CNationId) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_diplomacy(world);
    let (Some(an), Some(bn)) = (NonZeroU32::new(a.raw), NonZeroU32::new(b.raw)) else { return 0 };
    let Some(reg) = world.get_resource::<WarRegistry>() else { return 0 };
    if reg.are_allied(NationId(an), NationId(bn)) { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn teleology_diplomacy_has_truce(engine: *mut TeleologyEngine, a: CNationId, b: CNationId) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_diplomacy(world);
    let (Some(an), Some(bn)) = (NonZeroU32::new(a.raw), NonZeroU32::new(b.raw)) else { return 0 };
    let Some(reg) = world.get_resource::<WarRegistry>() else { return 0 };
    if reg.has_truce(NationId(an), NationId(bn)) { 1 } else { 0 }
}

// ===========================================================================
// Phase 3: Economy
// ===========================================================================

fn ensure_economy(world: &mut GameWorld) {
    if world.get_resource::<NationBudgets>().is_none() {
        let bounds = world.get_resource::<WorldBounds>().cloned();
        let (nc, pc) = bounds.map(|b| (b.nation_count as usize, b.province_count as usize)).unwrap_or((0, 0));
        world.insert_resource(EconomyConfig::default());
        world.insert_resource(NationBudgets::new(nc));
        world.insert_resource(GoodsRegistry::new());
        world.insert_resource(ProvinceEconomy::new(pc));
        world.insert_resource(TradeNetwork::new());
    }
}

#[no_mangle]
pub extern "C" fn teleology_economy_get_tax_income(engine: *mut TeleologyEngine, nation: CNationId) -> f64 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0.0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_economy(world);
    let Some(nid) = NonZeroU32::new(nation.raw) else { return 0.0 };
    world.get_resource::<NationBudgets>()
        .and_then(|b| b.get(NationId(nid)))
        .map(|e| e.tax_income)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn teleology_economy_get_production_income(engine: *mut TeleologyEngine, nation: CNationId) -> f64 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0.0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_economy(world);
    let Some(nid) = NonZeroU32::new(nation.raw) else { return 0.0 };
    world.get_resource::<NationBudgets>()
        .and_then(|b| b.get(NationId(nid)))
        .map(|e| e.production_income)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn teleology_economy_get_trade_income(engine: *mut TeleologyEngine, nation: CNationId) -> f64 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0.0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_economy(world);
    let Some(nid) = NonZeroU32::new(nation.raw) else { return 0.0 };
    world.get_resource::<NationBudgets>()
        .and_then(|b| b.get(NationId(nid)))
        .map(|e| e.trade_income)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn teleology_economy_get_total_income(engine: *mut TeleologyEngine, nation: CNationId) -> f64 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0.0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_economy(world);
    let Some(nid) = NonZeroU32::new(nation.raw) else { return 0.0 };
    world.get_resource::<NationBudgets>()
        .and_then(|b| b.get(NationId(nid)))
        .map(|e| e.total_income)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn teleology_economy_get_total_expenses(engine: *mut TeleologyEngine, nation: CNationId) -> f64 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0.0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_economy(world);
    let Some(nid) = NonZeroU32::new(nation.raw) else { return 0.0 };
    world.get_resource::<NationBudgets>()
        .and_then(|b| b.get(NationId(nid)))
        .map(|e| e.total_expenses)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn teleology_economy_get_balance(engine: *mut TeleologyEngine, nation: CNationId) -> f64 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0.0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_economy(world);
    let Some(nid) = NonZeroU32::new(nation.raw) else { return 0.0 };
    world.get_resource::<NationBudgets>()
        .and_then(|b| b.get(NationId(nid)))
        .map(|e| e.balance)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn teleology_economy_register_good(engine: *mut TeleologyEngine, name: *const std::ffi::c_char, base_price: f64) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_economy(world);
    let n = if name.is_null() { String::new() } else { unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned() };
    let Some(mut reg) = world.get_resource_mut::<GoodsRegistry>() else { return 0 };
    reg.register(n, base_price).raw()
}

#[no_mangle]
pub extern "C" fn teleology_economy_get_good_price(engine: *mut TeleologyEngine, good_id: u32) -> f64 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0.0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_economy(world);
    let Some(gid) = NonZeroU32::new(good_id) else { return 0.0 };
    let Some(reg) = world.get_resource::<GoodsRegistry>() else { return 0.0 };
    reg.base_price(teleology_core::GoodTypeId(gid))
}

#[no_mangle]
pub extern "C" fn teleology_economy_get_province_good(engine: *mut TeleologyEngine, province: CProvinceId) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_economy(world);
    let Some(pid) = NonZeroU32::new(province.raw) else { return 0 };
    let Some(econ) = world.get_resource::<ProvinceEconomy>() else { return 0 };
    econ.produced_good.get(ProvinceId(pid).index())
        .and_then(|o| *o)
        .map(|g| g.raw())
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn teleology_economy_set_province_good(engine: *mut TeleologyEngine, province: CProvinceId, good_id: u32) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_economy(world);
    let Some(pid) = NonZeroU32::new(province.raw) else { return };
    let Some(mut econ) = world.get_resource_mut::<ProvinceEconomy>() else { return };
    let idx = ProvinceId(pid).index();
    if let Some(slot) = econ.produced_good.get_mut(idx) {
        *slot = NonZeroU32::new(good_id).map(teleology_core::GoodTypeId);
    }
}

#[no_mangle]
pub extern "C" fn teleology_economy_get_province_trade_power(engine: *mut TeleologyEngine, province: CProvinceId) -> f64 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0.0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_economy(world);
    let Some(pid) = NonZeroU32::new(province.raw) else { return 0.0 };
    let Some(econ) = world.get_resource::<ProvinceEconomy>() else { return 0.0 };
    econ.local_trade_power.get(ProvinceId(pid).index()).copied().unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn teleology_economy_set_province_trade_power(engine: *mut TeleologyEngine, province: CProvinceId, value: f64) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_economy(world);
    let Some(pid) = NonZeroU32::new(province.raw) else { return };
    let Some(mut econ) = world.get_resource_mut::<ProvinceEconomy>() else { return };
    let idx = ProvinceId(pid).index();
    if let Some(slot) = econ.local_trade_power.get_mut(idx) { *slot = value; }
}

// ===========================================================================
// Phase 4: Population
// ===========================================================================

fn ensure_population(world: &mut GameWorld) {
    if world.get_resource::<ProvincePops>().is_none() {
        let pc = world.get_resource::<WorldBounds>().map(|b| b.province_count as usize).unwrap_or(0);
        world.insert_resource(PopulationConfig::default());
        world.insert_resource(ProvincePops::new(pc));
    }
}

#[no_mangle]
pub extern "C" fn teleology_pop_total(engine: *mut TeleologyEngine, province: CProvinceId) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_population(world);
    let Some(pid) = NonZeroU32::new(province.raw) else { return 0 };
    let Some(pops) = world.get_resource::<ProvincePops>() else { return 0 };
    pops.total_pop(ProvinceId(pid))
}

#[no_mangle]
pub extern "C" fn teleology_pop_average_unrest(engine: *mut TeleologyEngine, province: CProvinceId) -> f32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0.0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_population(world);
    let Some(pid) = NonZeroU32::new(province.raw) else { return 0.0 };
    let Some(pops) = world.get_resource::<ProvincePops>() else { return 0.0 };
    pops.average_unrest(ProvinceId(pid))
}

#[no_mangle]
pub extern "C" fn teleology_pop_group_count(engine: *mut TeleologyEngine, province: CProvinceId) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_population(world);
    let Some(pid) = NonZeroU32::new(province.raw) else { return 0 };
    let Some(pops) = world.get_resource::<ProvincePops>() else { return 0 };
    pops.get(ProvinceId(pid)).len() as u32
}

#[no_mangle]
pub extern "C" fn teleology_pop_group_size(engine: *mut TeleologyEngine, province: CProvinceId, index: u32) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_population(world);
    let Some(pid) = NonZeroU32::new(province.raw) else { return 0 };
    let Some(pops) = world.get_resource::<ProvincePops>() else { return 0 };
    pops.get(ProvinceId(pid)).get(index as usize).map(|g| g.size).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn teleology_pop_group_unrest(engine: *mut TeleologyEngine, province: CProvinceId, index: u32) -> f32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0.0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_population(world);
    let Some(pid) = NonZeroU32::new(province.raw) else { return 0.0 };
    let Some(pops) = world.get_resource::<ProvincePops>() else { return 0.0 };
    pops.get(ProvinceId(pid)).get(index as usize).map(|g| g.unrest).unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn teleology_pop_group_culture(engine: *mut TeleologyEngine, province: CProvinceId, index: u32) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_population(world);
    let Some(pid) = NonZeroU32::new(province.raw) else { return 0 };
    let Some(pops) = world.get_resource::<ProvincePops>() else { return 0 };
    pops.get(ProvinceId(pid)).get(index as usize).map(|g| g.culture.raw()).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn teleology_pop_group_religion(engine: *mut TeleologyEngine, province: CProvinceId, index: u32) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_population(world);
    let Some(pid) = NonZeroU32::new(province.raw) else { return 0 };
    let Some(pops) = world.get_resource::<ProvincePops>() else { return 0 };
    pops.get(ProvinceId(pid)).get(index as usize).map(|g| g.religion.raw()).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn teleology_pop_add_group(engine: *mut TeleologyEngine, province: CProvinceId, culture: u32, religion: u32, size: u32) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_population(world);
    let Some(pid) = NonZeroU32::new(province.raw) else { return };
    let Some(cult) = NonZeroU32::new(culture) else { return };
    let Some(rel) = NonZeroU32::new(religion) else { return };
    let Some(mut pops) = world.get_resource_mut::<ProvincePops>() else { return };
    if let Some(groups) = pops.get_mut(ProvinceId(pid)) {
        groups.push(teleology_core::PopGroup {
            culture: TagId(cult),
            religion: TagId(rel),
            size,
            unrest: 0.0,
        });
    }
}

/// Check for revolts. Writes up to `cap` revolting provinces to out arrays.
/// Returns the number of revolts found.
#[no_mangle]
pub extern "C" fn teleology_pop_check_revolts(
    engine: *mut TeleologyEngine,
    out_provinces: *mut u32,
    out_strengths: *mut u32,
    cap: u32,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_population(world);
    let pc = world.get_resource::<WorldBounds>().map(|b| b.province_count).unwrap_or(0);
    let config = world.get_resource::<PopulationConfig>().cloned().unwrap_or_default();
    let Some(pops) = world.get_resource::<ProvincePops>() else { return 0 };
    let revolts = check_revolts(&config, pops, pc);
    let n = (cap as usize).min(revolts.len());
    for i in 0..n {
        if !out_provinces.is_null() { unsafe { *out_provinces.add(i) = revolts[i].0.0.get(); } }
        if !out_strengths.is_null() { unsafe { *out_strengths.add(i) = revolts[i].1; } }
    }
    revolts.len() as u32
}

// ===========================================================================
// Phase 5: Modifiers
// ===========================================================================

fn ensure_modifiers(world: &mut GameWorld) {
    if world.get_resource::<ProvinceModifiers>().is_none() {
        let bounds = world.get_resource::<WorldBounds>().cloned();
        let (nc, pc) = bounds.map(|b| (b.nation_count as usize, b.province_count as usize)).unwrap_or((0, 0));
        world.insert_resource(ProvinceModifiers::new(pc));
        world.insert_resource(NationModifiers::new(nc));
    }
}

fn make_modifier_value(op: u32, value: f64) -> ModifierValue {
    match op {
        0 => ModifierValue::Additive(value),
        1 => ModifierValue::Multiplicative(value),
        2 => ModifierValue::Set(value),
        _ => ModifierValue::Custom { op_id: op, value },
    }
}

/// Add a modifier to a province. Returns ModifierId raw (0 on failure).
#[no_mangle]
pub extern "C" fn teleology_modifier_add_province(
    engine: *mut TeleologyEngine,
    province: CProvinceId,
    type_id: u32,
    op: u32,
    value: f64,
    source_id: u32,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_modifiers(world);
    let Some(pid) = NonZeroU32::new(province.raw) else { return 0 };
    let Some(tid) = NonZeroU32::new(type_id) else { return 0 };
    let m = Modifier {
        id: ModifierId(NonZeroU32::new(1).unwrap()), // will be reassigned by add()
        ty: ModifierTypeId(tid),
        value: make_modifier_value(op, value),
        source_id,
        expires_on: None,
    };
    let Some(mut mods) = world.get_resource_mut::<ProvinceModifiers>() else { return 0 };
    mods.add(ProvinceId(pid), m).0.get()
}

/// Add a modifier to a nation. Returns ModifierId raw (0 on failure).
#[no_mangle]
pub extern "C" fn teleology_modifier_add_nation(
    engine: *mut TeleologyEngine,
    nation: CNationId,
    type_id: u32,
    op: u32,
    value: f64,
    source_id: u32,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_modifiers(world);
    let Some(nid) = NonZeroU32::new(nation.raw) else { return 0 };
    let Some(tid) = NonZeroU32::new(type_id) else { return 0 };
    let m = Modifier {
        id: ModifierId(NonZeroU32::new(1).unwrap()),
        ty: ModifierTypeId(tid),
        value: make_modifier_value(op, value),
        source_id,
        expires_on: None,
    };
    let Some(mut mods) = world.get_resource_mut::<NationModifiers>() else { return 0 };
    mods.add(NationId(nid), m).0.get()
}

#[no_mangle]
pub extern "C" fn teleology_modifier_remove_province(engine: *mut TeleologyEngine, province: CProvinceId, modifier_id: u32) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_modifiers(world);
    let Some(pid) = NonZeroU32::new(province.raw) else { return 0 };
    let Some(mid) = NonZeroU32::new(modifier_id) else { return 0 };
    let Some(mut mods) = world.get_resource_mut::<ProvinceModifiers>() else { return 0 };
    if mods.remove(ProvinceId(pid), ModifierId(mid)) { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn teleology_modifier_remove_nation(engine: *mut TeleologyEngine, nation: CNationId, modifier_id: u32) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_modifiers(world);
    let Some(nid) = NonZeroU32::new(nation.raw) else { return 0 };
    let Some(mid) = NonZeroU32::new(modifier_id) else { return 0 };
    let Some(mut mods) = world.get_resource_mut::<NationModifiers>() else { return 0 };
    if mods.remove(NationId(nid), ModifierId(mid)) { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn teleology_modifier_list_province(engine: *mut TeleologyEngine, province: CProvinceId) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_modifiers(world);
    let Some(pid) = NonZeroU32::new(province.raw) else { return 0 };
    let Some(mods) = world.get_resource::<ProvinceModifiers>() else { return 0 };
    mods.list(ProvinceId(pid)).len() as u32
}

#[no_mangle]
pub extern "C" fn teleology_modifier_list_nation(engine: *mut TeleologyEngine, nation: CNationId) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_modifiers(world);
    let Some(nid) = NonZeroU32::new(nation.raw) else { return 0 };
    let Some(mods) = world.get_resource::<NationModifiers>() else { return 0 };
    mods.list(NationId(nid)).len() as u32
}

/// Apply all modifiers of a given type for a scope to a base value.
/// scope_kind: 0=Province, 1=Nation. scope_id: the raw province/nation id.
#[no_mangle]
pub extern "C" fn teleology_modifier_apply(engine: *mut TeleologyEngine, base: f64, type_id: u32, scope_kind: u32, scope_id: u32) -> f64 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return base };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_modifiers(world);
    let Some(tid) = NonZeroU32::new(type_id) else { return base };
    let Some(sid) = NonZeroU32::new(scope_id) else { return base };
    let target_ty = ModifierTypeId(tid);
    let now = world.get_resource::<GameDate>().copied();

    match scope_kind {
        0 => {
            let Some(mods) = world.get_resource::<ProvinceModifiers>() else { return base };
            let list: Vec<_> = mods.list(ProvinceId(sid)).iter().filter(|m| m.ty == target_ty).cloned().collect();
            apply_modifiers(base, &list, None, now)
        }
        1 => {
            let Some(mods) = world.get_resource::<NationModifiers>() else { return base };
            let list: Vec<_> = mods.list(NationId(sid)).iter().filter(|m| m.ty == target_ty).cloned().collect();
            apply_modifiers(base, &list, None, now)
        }
        _ => base,
    }
}

// ===========================================================================
// Phase 6: Characters
// ===========================================================================

static NEXT_PERSISTENT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Spawn a character. Returns persistent_id (0 on failure).
#[no_mangle]
pub extern "C" fn teleology_character_spawn(engine: *mut TeleologyEngine, name_id: u32, birth_year: i32) -> u64 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    let pid = NEXT_PERSISTENT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let character = Character {
        name_id,
        persistent_id: pid,
        birth_year: if birth_year != 0 { Some(birth_year) } else { None },
        death_year: None,
    };
    spawn_character(world, character);
    pid
}

/// Set a character's role. role: 0=Leader, 1=General, 2=Advisor, 3+=Custom.
#[no_mangle]
pub extern "C" fn teleology_character_set_role(
    engine: *mut TeleologyEngine,
    persistent_id: u64,
    role: u32,
    nation: CNationId,
    army: u32,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    // Find entity by persistent_id.
    let entity = {
        let mut found = None;
        for (e, c) in world.query::<(Entity, &Character)>().iter(world) {
            if c.persistent_id == persistent_id { found = Some(e); break; }
        }
        found
    };
    let Some(entity) = entity else { return };
    let nid_opt = NonZeroU32::new(nation.raw).map(NationId);
    let char_role = match role {
        0 => nid_opt.map(CharacterRole::Leader),
        1 => nid_opt.map(|n| CharacterRole::General { nation: n, army_raw: army }),
        2 => nid_opt.map(CharacterRole::Advisor),
        _ => Some(CharacterRole::Custom(role)),
    };
    if let Some(r) = char_role {
        world.entity_mut(entity).insert(r);
    }
}

/// Get a character stat. stat: 0=military, 1=diplomacy, 2=administration.
#[no_mangle]
pub extern "C" fn teleology_character_get_stat(engine: *mut TeleologyEngine, persistent_id: u64, stat: u32) -> i16 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    for (_e, c, s) in world.query::<(Entity, &Character, &CharacterStats)>().iter(world) {
        if c.persistent_id == persistent_id {
            return match stat {
                0 => s.military,
                1 => s.diplomacy,
                2 => s.administration,
                _ => 0,
            };
        }
    }
    0
}

/// Set a character stat. stat: 0=military, 1=diplomacy, 2=administration.
#[no_mangle]
pub extern "C" fn teleology_character_set_stat(engine: *mut TeleologyEngine, persistent_id: u64, stat: u32, value: i16) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    for (_e, c, mut s) in world.query::<(Entity, &Character, &mut CharacterStats)>().iter_mut(world) {
        if c.persistent_id == persistent_id {
            match stat {
                0 => s.military = value,
                1 => s.diplomacy = value,
                2 => s.administration = value,
                _ => {}
            }
            return;
        }
    }
}

/// Get a custom stat value for a character. Returns 0 if not found.
#[no_mangle]
pub extern "C" fn teleology_character_get_custom_stat(engine: *mut TeleologyEngine, persistent_id: u64, stat_id: u32) -> i32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    for (_e, c, s) in world.query::<(Entity, &Character, &CharacterStats)>().iter(world) {
        if c.persistent_id == persistent_id {
            return s.custom.get(&stat_id).copied().unwrap_or(0);
        }
    }
    0
}

/// Set a custom stat value for a character.
#[no_mangle]
pub extern "C" fn teleology_character_set_custom_stat(engine: *mut TeleologyEngine, persistent_id: u64, stat_id: u32, value: i32) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    for (_e, c, mut s) in world.query::<(Entity, &Character, &mut CharacterStats)>().iter_mut(world) {
        if c.persistent_id == persistent_id {
            s.custom.insert(stat_id, value);
            return;
        }
    }
}

/// Mark a character as dead.
#[no_mangle]
pub extern "C" fn teleology_character_kill(engine: *mut TeleologyEngine, persistent_id: u64, death_year: i32) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    for (_e, mut c) in world.query::<(Entity, &mut Character)>().iter_mut(world) {
        if c.persistent_id == persistent_id {
            c.death_year = Some(death_year);
            return;
        }
    }
}

// ===========================================================================
// Phase 7: Combat (config + inspection)
// ===========================================================================

fn ensure_combat(world: &mut GameWorld) {
    if world.get_resource::<CombatModel>().is_none() {
        world.insert_resource(CombatModel::default());
        world.insert_resource(CombatResultLog::new());
        world.insert_resource(UnitTypeRegistry::new());
    }
}

/// Set the active combat model. model: 0=StackBased, 1=OneUnitPerTile, 2=Deployment, 3=TacticalGrid.
#[no_mangle]
pub extern "C" fn teleology_combat_set_model(engine: *mut TeleologyEngine, model: u8) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_combat(world);
    let cm = match model {
        0 => CombatModel::StackBased(Default::default()),
        1 => CombatModel::OneUnitPerTile(Default::default()),
        2 => CombatModel::Deployment(Default::default()),
        3 => CombatModel::TacticalGrid(Default::default()),
        _ => return,
    };
    world.insert_resource(cm);
}

/// Get the active combat model. Returns 0=Stack, 1=Tile, 2=Deploy, 3=Tactical, 255=none.
#[no_mangle]
pub extern "C" fn teleology_combat_get_model(engine: *mut TeleologyEngine) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 255 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_combat(world);
    let Some(cm) = world.get_resource::<CombatModel>() else { return 255 };
    match cm {
        CombatModel::StackBased(_) => 0,
        CombatModel::OneUnitPerTile(_) => 1,
        CombatModel::Deployment(_) => 2,
        CombatModel::TacticalGrid(_) => 3,
    }
}

/// Register a unit type. category: 0=Infantry, 1=Cavalry, 2=Ranged, 3=Siege, 4=Naval, 5+=Custom.
/// Returns UnitTypeId raw (0 on failure).
#[no_mangle]
pub extern "C" fn teleology_combat_register_unit_type(
    engine: *mut TeleologyEngine,
    name: *const std::ffi::c_char,
    category: u32,
    strength: u16,
    morale: u16,
    speed: u8,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_combat(world);
    let n = if name.is_null() { String::new() } else { unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned() };
    let cat = match category {
        0 => UnitCategory::Infantry,
        1 => UnitCategory::Cavalry,
        2 => UnitCategory::Ranged,
        3 => UnitCategory::Siege,
        4 => UnitCategory::Naval,
        c => UnitCategory::Custom(c),
    };
    let Some(mut reg) = world.get_resource_mut::<UnitTypeRegistry>() else { return 0 };
    reg.register(n, cat, strength, morale, speed).raw()
}

/// Get the number of logged combat results.
#[no_mangle]
pub extern "C" fn teleology_combat_result_count(engine: *mut TeleologyEngine) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_combat(world);
    let Some(log) = world.get_resource::<CombatResultLog>() else { return 0 };
    log.results.len() as u32
}

/// Get a combat result by index. Returns the location province raw (0 if invalid).
/// Writes casualties and winner to out pointers.
/// winner_out: 0=Attacker, 1=Defender, 2=Draw.
#[no_mangle]
pub extern "C" fn teleology_combat_result_get(
    engine: *mut TeleologyEngine,
    index: u32,
    attacker_casualties_out: *mut u32,
    defender_casualties_out: *mut u32,
    winner_out: *mut u8,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_combat(world);
    let Some(log) = world.get_resource::<CombatResultLog>() else { return 0 };
    let Some(r) = log.results.get(index as usize) else { return 0 };
    if !attacker_casualties_out.is_null() { unsafe { *attacker_casualties_out = r.attacker_casualties; } }
    if !defender_casualties_out.is_null() { unsafe { *defender_casualties_out = r.defender_casualties; } }
    if !winner_out.is_null() {
        unsafe { *winner_out = match r.winner { BattleSide::Attacker => 0, BattleSide::Defender => 1, BattleSide::Draw => 2 }; }
    }
    r.location.0.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_context_tick_advances_date() {
        let mut engine = EngineContext::new();
        for _ in 0..5 {
            engine.tick();
        }
        let date = engine.world().get_resource::<GameDate>().copied().unwrap();
        // Default start 1444-1-1, +5 days = 1444-1-6
        assert_eq!(date.day, 6);
        assert_eq!(date.month, 1);
        assert_eq!(date.year, 1444);
    }

    #[test]
    fn engine_context_has_provinces_and_nations() {
        let engine = EngineContext::new();
        let bounds = engine.world().get_resource::<WorldBounds>().unwrap();
        assert_eq!(bounds.province_count, 100);
        assert_eq!(bounds.nation_count, 20);
        let map_kind = engine.world().get_resource::<teleology_core::MapKind>().unwrap();
        let (w, h) = match map_kind {
            teleology_core::MapKind::Square(m) => (m.width, m.height),
            _ => (0, 0),
        };
        assert_eq!(w, 20);
        assert_eq!(h, 10);
    }
}
