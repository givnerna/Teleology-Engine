//! Spatial C API: raycasting, coordinate conversion, province/nation property accessors.

use std::num::NonZeroU32;
use teleology_core::{
    raycast, screen_to_world, world_to_screen, screen_to_tile_square, screen_to_tile_hex,
    tile_distance_square, tile_distance_hex, RaycastHit, MapKind,
    NationId, NationStore, ProvinceId, ProvinceStore, Viewport, WorldBounds,
};
use teleology_script_api::{CNationId, CProvinceId, TeleologyEngine};

use crate::context_from_engine;

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

/// Update the viewport state.
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

/// Perform a raycast: screen coordinates -> province/tile/world.
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

/// Convert screen coordinates to world space.
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

/// Convert world coordinates to screen space.
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

/// Convert screen coordinates to tile coordinates.
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

/// Compute tile distance between two tiles.
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

// --- Province & Nation extended field accessors ---

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
