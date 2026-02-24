//! Population C API: pop totals, unrest, groups, revolts.

use std::num::NonZeroU32;
use teleology_core::{
    GameWorld, PopulationConfig, ProvinceId, ProvincePops, TagId,
    WorldBounds, check_revolts,
};
use teleology_script_api::{CProvinceId, TeleologyEngine};

use crate::context_from_engine;

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

/// Check for revolts.
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
