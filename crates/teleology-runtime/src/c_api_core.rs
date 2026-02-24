//! Core C API: date/time, province/nation count, province owner get/set.

use std::num::NonZeroU32;
use teleology_core::{
    GameDate, NationId, ProvinceId, ProvinceStore, WorldBounds,
};
use teleology_script_api::{CGameDate, CGameTime, CNationId, CProvinceId, TeleologyEngine};

use crate::context_from_engine;

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
