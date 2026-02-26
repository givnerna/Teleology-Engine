//! Modifiers, Characters, and Combat C API.

use bevy_ecs::entity::Entity;
use std::ffi::CStr;
use std::num::NonZeroU32;
use teleology_core::{
    apply_modifiers, spawn_character,
    Character, CharacterRole, CharacterStats,
    CombatModel, CombatResultLog, BattleSide,
    GameDate, GameWorld, Modifier, ModifierId, ModifierTypeId, ModifierValue,
    NationId, NationModifiers, ProvinceId, ProvinceModifiers,
    UnitCategory, UnitTypeRegistry, WorldBounds,
};
use teleology_script_api::{CNationId, CProvinceId, TeleologyEngine};

use crate::context_from_engine;

// --- Modifiers ---

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
        id: ModifierId(NonZeroU32::new(1).unwrap()),
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

// --- Characters ---

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

/// Set a character's role.
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

/// Get a character stat.
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

/// Set a character stat.
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

/// Get a custom stat value for a character.
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

// --- Combat ---

fn ensure_combat(world: &mut GameWorld) {
    if world.get_resource::<CombatModel>().is_none() {
        world.insert_resource(CombatModel::default());
        world.insert_resource(CombatResultLog::new());
        world.insert_resource(UnitTypeRegistry::new());
    }
}

/// Set the active combat model.
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

/// Get the active combat model.
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

/// Register a unit type.
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

/// Get a combat result by index.
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
