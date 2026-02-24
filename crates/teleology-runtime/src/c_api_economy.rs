//! Economy C API: budgets, goods, trade power.

use std::ffi::CStr;
use std::num::NonZeroU32;
use teleology_core::{
    EconomyConfig, GameWorld, GoodsRegistry, NationBudgets, NationId,
    ProvinceEconomy, ProvinceId, TradeNetwork, WorldBounds,
};
use teleology_script_api::{CNationId, CProvinceId, TeleologyEngine};

use crate::context_from_engine;

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
