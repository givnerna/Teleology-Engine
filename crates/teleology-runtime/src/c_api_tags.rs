//! Tags C API: register types/tags, get/set province/nation tags.

use std::ffi::CStr;
use std::num::NonZeroU32;
use teleology_core::{
    GameWorld, NationId, NationTags, ProvinceId, ProvinceTags,
    TagId, TagRegistry, TagTypeId,
};
use teleology_script_api::{CTagId, CTagTypeId, CNationId, CProvinceId, TeleologyEngine};

use crate::context_from_engine;

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
