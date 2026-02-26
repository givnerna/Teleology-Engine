//! Diplomacy C API: opinion, trust, wars, alliances, truces.

use std::num::NonZeroU32;
use teleology_core::{
    DiplomaticRelations, DiplomacyConfig, GameDate, GameWorld,
    NationId, WarGoal, WarId, WarRegistry, WorldBounds,
};
use teleology_script_api::{CNationId, TeleologyEngine};

use crate::context_from_engine;

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

/// Declare war. Returns WarId raw (0 on failure).
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
