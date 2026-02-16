//! Runtime: world setup, script loading, hot reload, and engine C API implementation.
//! On WebGL, script loading and hot reload are no-ops; simulation runs without C++ scripts.

mod audio;
mod video;

use std::cell::UnsafeCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use teleology_core::{
    publish_event, ArmyComposition, ArmyRegistry, EventBus, EventScopeRef,
    GameDate, GameWorld, NationId, NationTags, ProvinceId, ProvinceStore, ProvinceTags,
    ProgressState, ProgressTrees, SimulationSchedule, TagId, TagRegistry, TagTypeId, WorldBuilder,
    WorldBounds, WorldSimulation,
};
use teleology_script_api::{
    load_script_api, CArmyId, CGameDate, CNodeId, CNationId, CProvinceId, CTagId, CTagTypeId,
    CTreeId, TeleologyEngine, TeleologyScriptApi, ScriptHandle,
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
        WorldSimulation::tick_day(world);

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
    publish_event(world, &topic, EventScopeRef::Global, payload_type_id, bytes, 0);
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
