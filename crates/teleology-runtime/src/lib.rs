//! Runtime: world setup, script loading, hot reload, and engine C API implementation.
//! On WebGL, script loading and hot reload are no-ops; simulation runs without C++ scripts.

mod audio;
mod video;

mod c_api_core;
mod c_api_tags;
mod c_api_events;
mod c_api_ui;
mod c_api_spatial;
mod c_api_diplomacy;
mod c_api_economy;
mod c_api_population;
mod c_api_modifiers;

use std::cell::UnsafeCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use teleology_core::{
    ArmyComposition, ArmyRegistry, NationId, ProvinceId,
    GameWorld, SimulationSchedule, WorldBuilder, WorldBounds, WorldSimulation,
    UiCommandBuffer, Viewport,
};
use teleology_script_api::{
    load_script_api, CArmyId, CNationId, CProvinceId, TeleologyEngine, TeleologyScriptApi, ScriptHandle,
};
use std::num::NonZeroU32;

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

// --- Engine C API helper (used by all c_api_* submodules) ---

fn context_from_engine(engine: *mut TeleologyEngine) -> Option<&'static mut EngineContext> {
    if engine.is_null() {
        return None;
    }
    Some(unsafe { &mut *(engine as *mut EngineContext) })
}

// --- Progress trees + armies + input polling (small, kept here) ---

use teleology_core::{ProgressState, ProgressTrees};
use teleology_script_api::{CNodeId, CTreeId};

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
