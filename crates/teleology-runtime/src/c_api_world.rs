//! C API: world creation/reset, terrain registry, province generation, map save/load, time config.
//!
//! These functions let C++ developers configure the game world entirely from `on_init()`
//! without writing any Rust code.

use std::ffi::CStr;
use teleology_core::{
    GameDate, GameTime, HexMapLayout, MapKind, MapLayout, ProvinceStore, SimulationSchedule,
    TerrainRegistry, TerrainType, WorldBounds, WorldBuilder,
    generate_provinces_hex, generate_provinces_square, MapFile,
};
use teleology_script_api::TeleologyEngine;

use crate::context_from_engine;

// ---------------------------------------------------------------------------
// World reset / rebuild
// ---------------------------------------------------------------------------

/// Tear down the current world and rebuild it with the given parameters.
///
/// `map_type`: 0 = Square (filled), 1 = Square (empty), 2 = Hex (filled),
///             3 = Hex (empty), 4 = Irregular/vector.
///
/// `map_w` / `map_h` are ignored for Irregular maps.
///
/// All optional subsystems (tags, events, diplomacy, economy, etc.) remain
/// lazily initialised — they are created on first use from other C API calls.
#[no_mangle]
pub extern "C" fn teleology_world_reset(
    engine: *mut TeleologyEngine,
    provinces: u32,
    nations: u32,
    map_type: u32,
    map_w: u32,
    map_h: u32,
) {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return,
    };
    let world = unsafe { &mut *ctx.world.get() };

    // Build a fresh world with the requested geometry.
    let mut builder = WorldBuilder::new().provinces(provinces).nations(nations);
    builder = match map_type {
        0 => builder.map_size(map_w, map_h),
        1 => builder.map_size_empty(map_w, map_h),
        2 => builder.map_hex(map_w, map_h),
        3 => builder.map_hex_empty(map_w, map_h),
        4 => builder.map_irregular(),
        _ => builder.map_size(map_w, map_h), // default to square
    };

    // Reset the ECS world and rebuild.
    *world = teleology_core::GameWorld::new();
    builder.build(world);
    SimulationSchedule::build(world);
    world.insert_resource(teleology_core::UiCommandBuffer::new());
    world.insert_resource(teleology_core::Viewport {
        base_cell: 14.0,
        zoom: 1.0,
        ..teleology_core::Viewport::default()
    });
}

// ---------------------------------------------------------------------------
// Terrain registry
// ---------------------------------------------------------------------------

/// Register (or overwrite) a terrain type.
/// `is_land`: 1 = passable land, 0 = water/impassable.
/// Returns 1 on success, 0 on failure (null engine).
#[no_mangle]
pub extern "C" fn teleology_terrain_register(
    engine: *mut TeleologyEngine,
    id: u8,
    name_utf8: *const std::os::raw::c_char,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
    is_land: u8,
) -> u8 {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return 0,
    };
    let name = if name_utf8.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(name_utf8) }
            .to_string_lossy()
            .into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };

    // Ensure registry exists.
    if world.get_resource::<TerrainRegistry>().is_none() {
        world.insert_resource(TerrainRegistry::default());
    }
    let mut reg = world.get_resource_mut::<TerrainRegistry>().unwrap();
    reg.register(TerrainType {
        id,
        name,
        color: [r, g, b, a],
        is_land: is_land != 0,
    });
    1
}

/// Returns the number of registered terrain types.
#[no_mangle]
pub extern "C" fn teleology_terrain_count(engine: *mut TeleologyEngine) -> u32 {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return 0,
    };
    let world = unsafe { &*ctx.world.get() };
    world
        .get_resource::<TerrainRegistry>()
        .map(|r| r.types.len() as u32)
        .unwrap_or(0)
}

/// Write terrain name for `id` into `out` (NUL-terminated). Returns full length (excl NUL).
#[no_mangle]
pub extern "C" fn teleology_terrain_get_name(
    engine: *mut TeleologyEngine,
    id: u8,
    out: *mut u8,
    out_cap: u32,
) -> u32 {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return 0,
    };
    let world = unsafe { &*ctx.world.get() };
    let name = match world.get_resource::<TerrainRegistry>() {
        Some(reg) => reg.name(id),
        None => return 0,
    };
    let bytes = name.as_bytes();
    if !out.is_null() && out_cap > 0 {
        let copy_len = bytes.len().min((out_cap - 1) as usize);
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, copy_len);
            *out.add(copy_len) = 0;
        }
    }
    bytes.len() as u32
}

/// Get terrain color (RGBA) for `id`.
#[no_mangle]
pub extern "C" fn teleology_terrain_get_color(
    engine: *mut TeleologyEngine,
    id: u8,
    r: *mut u8,
    g: *mut u8,
    b: *mut u8,
    a: *mut u8,
) {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return,
    };
    let world = unsafe { &*ctx.world.get() };
    let color = match world.get_resource::<TerrainRegistry>() {
        Some(reg) => reg.color(id),
        None => [128, 128, 128, 255],
    };
    unsafe {
        if !r.is_null() { *r = color[0]; }
        if !g.is_null() { *g = color[1]; }
        if !b.is_null() { *b = color[2]; }
        if !a.is_null() { *a = color[3]; }
    }
}

/// Returns 1 if terrain `id` is land, 0 if water/unknown.
#[no_mangle]
pub extern "C" fn teleology_terrain_is_land(engine: *mut TeleologyEngine, id: u8) -> u8 {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return 0,
    };
    let world = unsafe { &*ctx.world.get() };
    match world.get_resource::<TerrainRegistry>() {
        Some(reg) => match reg.get(id) {
            Some(t) => t.is_land as u8,
            None => 0,
        },
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// Province auto-generation
// ---------------------------------------------------------------------------

/// Auto-generate provinces using jittered-seed BFS flood fill.
/// Works for both square and hex maps. Returns the actual province count generated.
/// On irregular maps or if no map resource exists, returns 0.
#[no_mangle]
pub extern "C" fn teleology_generate_provinces(
    engine: *mut TeleologyEngine,
    target_count: u32,
) -> u32 {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return 0,
    };
    let world = unsafe { &mut *ctx.world.get() };

    // Determine map kind and dispatch.
    let map_kind = world.get_resource::<MapKind>().cloned();
    match map_kind {
        Some(MapKind::Square { .. }) => {
            let map = world.get_resource_mut::<MapLayout>();
            if map.is_none() { return 0; }
            // We need mutable refs to multiple resources, so remove-and-reinsert.
            let mut map = world.remove_resource::<MapLayout>().unwrap();
            let mut store = world.remove_resource::<ProvinceStore>().unwrap_or_else(|| ProvinceStore::new(0));
            let mut bounds = world.remove_resource::<WorldBounds>().unwrap_or(WorldBounds { province_count: 0, nation_count: 0 });
            generate_provinces_square(&mut map, target_count, &mut store, &mut bounds);
            let count = bounds.province_count;
            world.insert_resource(map);
            world.insert_resource(store);
            world.insert_resource(bounds);
            count
        }
        Some(MapKind::Hex { .. }) => {
            let map = world.get_resource_mut::<HexMapLayout>();
            if map.is_none() { return 0; }
            let mut map = world.remove_resource::<HexMapLayout>().unwrap();
            let mut store = world.remove_resource::<ProvinceStore>().unwrap_or_else(|| ProvinceStore::new(0));
            let mut bounds = world.remove_resource::<WorldBounds>().unwrap_or(WorldBounds { province_count: 0, nation_count: 0 });
            generate_provinces_hex(&mut map, target_count, &mut store, &mut bounds);
            let count = bounds.province_count;
            world.insert_resource(map);
            world.insert_resource(store);
            world.insert_resource(bounds);
            count
        }
        _ => 0, // Irregular or missing — cannot auto-generate
    }
}

// ---------------------------------------------------------------------------
// Map file save / load
// ---------------------------------------------------------------------------

/// Save the current world state to a binary map file. Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn teleology_map_save(
    engine: *mut TeleologyEngine,
    path_utf8: *const std::os::raw::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return 0,
    };
    if path_utf8.is_null() { return 0; }
    let path = unsafe { CStr::from_ptr(path_utf8) }.to_string_lossy();
    let world = unsafe { &mut *ctx.world.get() };

    let map_file = match MapFile::from_world(world) {
        Some(mf) => mf,
        None => return 0,
    };
    let file = match std::fs::File::create(path.as_ref()) {
        Ok(f) => f,
        Err(_) => return 0,
    };
    let mut writer = std::io::BufWriter::new(file);
    match map_file.write(&mut writer) {
        Ok(_) => 1,
        Err(_) => 0,
    }
}

/// Load a binary map file and apply it to the world. Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn teleology_map_load(
    engine: *mut TeleologyEngine,
    path_utf8: *const std::os::raw::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return 0,
    };
    if path_utf8.is_null() { return 0; }
    let path = unsafe { CStr::from_ptr(path_utf8) }.to_string_lossy();
    let world = unsafe { &mut *ctx.world.get() };

    let file = match std::fs::File::open(path.as_ref()) {
        Ok(f) => f,
        Err(_) => return 0,
    };
    let mut reader = std::io::BufReader::new(file);
    let map_file = match MapFile::read(&mut reader) {
        Ok(mf) => mf,
        Err(_) => return 0,
    };
    map_file.apply_to_world(world);
    1
}

/// Save the current world state to a JSON map file (pretty-printed). Returns 1 on success.
#[no_mangle]
pub extern "C" fn teleology_map_save_json(
    engine: *mut TeleologyEngine,
    path_utf8: *const std::os::raw::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return 0,
    };
    if path_utf8.is_null() { return 0; }
    let path = unsafe { CStr::from_ptr(path_utf8) }.to_string_lossy();
    let world = unsafe { &mut *ctx.world.get() };

    let map_file = match MapFile::from_world(world) {
        Some(mf) => mf,
        None => return 0,
    };
    let file = match std::fs::File::create(path.as_ref()) {
        Ok(f) => f,
        Err(_) => return 0,
    };
    let mut writer = std::io::BufWriter::new(file);
    match map_file.write_json_pretty(&mut writer) {
        Ok(_) => 1,
        Err(_) => 0,
    }
}

/// Load a JSON map file and apply it to the world. Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn teleology_map_load_json(
    engine: *mut TeleologyEngine,
    path_utf8: *const std::os::raw::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return 0,
    };
    if path_utf8.is_null() { return 0; }
    let path = unsafe { CStr::from_ptr(path_utf8) }.to_string_lossy();
    let world = unsafe { &mut *ctx.world.get() };

    let file = match std::fs::File::open(path.as_ref()) {
        Ok(f) => f,
        Err(_) => return 0,
    };
    let mut reader = std::io::BufReader::new(file);
    let map_file = match MapFile::read_json(&mut reader) {
        Ok(mf) => mf,
        Err(_) => return 0,
    };
    map_file.apply_to_world(world);
    1
}

// ---------------------------------------------------------------------------
// Time / date configuration
// ---------------------------------------------------------------------------

/// Set the start date for the simulation.
#[no_mangle]
pub extern "C" fn teleology_set_start_date(
    engine: *mut TeleologyEngine,
    day: u16,
    month: u8,
    year: i32,
) {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return,
    };
    let world = unsafe { &mut *ctx.world.get() };
    world.insert_resource(GameDate { day, month, year });
}

/// Set the full game time (second, minute, hour, day, month, year, tick).
#[no_mangle]
pub extern "C" fn teleology_set_start_time(
    engine: *mut TeleologyEngine,
    second: u8,
    minute: u8,
    hour: u8,
    day: u16,
    month: u8,
    year: i32,
    tick: u64,
) {
    let ctx = match context_from_engine(engine) {
        Some(c) => c,
        None => return,
    };
    let world = unsafe { &mut *ctx.world.get() };
    world.insert_resource(GameTime {
        second,
        minute,
        hour,
        day,
        month,
        year,
        tick,
    });
}
