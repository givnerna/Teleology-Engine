//! Game UI C API: immediate-mode command buffer and UI prefabs.

use std::ffi::CStr;
use teleology_core::{GameWorld, UiCommand, UiCommandBuffer, UiPrefabRegistry};
use teleology_script_api::TeleologyEngine;

use crate::context_from_engine;

fn ensure_ui_buffer(world: &mut GameWorld) {
    if world.get_resource::<UiCommandBuffer>().is_none() {
        world.insert_resource(UiCommandBuffer::new());
    }
}

fn push_ui_cmd(engine: *mut TeleologyEngine, cmd: UiCommand) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_ui_buffer(world);
    if let Some(mut buf) = world.get_resource_mut::<UiCommandBuffer>() {
        buf.push(cmd);
    }
}

fn ensure_prefab_registry(world: &mut GameWorld) {
    if world.get_resource::<UiPrefabRegistry>().is_none() {
        world.insert_resource(UiPrefabRegistry::new());
    }
}

#[no_mangle]
pub extern "C" fn teleology_ui_begin_window(
    engine: *mut TeleologyEngine,
    title: *const std::ffi::c_char,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    let title = if title.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(title) }.to_string_lossy().into_owned()
    };
    push_ui_cmd(engine, UiCommand::BeginWindow { title, x, y, w, h });
}

#[no_mangle]
pub extern "C" fn teleology_ui_end_window(engine: *mut TeleologyEngine) {
    push_ui_cmd(engine, UiCommand::EndWindow);
}

#[no_mangle]
pub extern "C" fn teleology_ui_begin_horizontal(engine: *mut TeleologyEngine) {
    push_ui_cmd(engine, UiCommand::BeginHorizontal);
}

#[no_mangle]
pub extern "C" fn teleology_ui_end_horizontal(engine: *mut TeleologyEngine) {
    push_ui_cmd(engine, UiCommand::EndHorizontal);
}

#[no_mangle]
pub extern "C" fn teleology_ui_begin_vertical(engine: *mut TeleologyEngine) {
    push_ui_cmd(engine, UiCommand::BeginVertical);
}

#[no_mangle]
pub extern "C" fn teleology_ui_end_vertical(engine: *mut TeleologyEngine) {
    push_ui_cmd(engine, UiCommand::EndVertical);
}

#[no_mangle]
pub extern "C" fn teleology_ui_label(engine: *mut TeleologyEngine, text: *const std::ffi::c_char) {
    let text = if text.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
    };
    push_ui_cmd(engine, UiCommand::Label { text, font_size: 0.0 });
}

#[no_mangle]
pub extern "C" fn teleology_ui_label_sized(
    engine: *mut TeleologyEngine,
    text: *const std::ffi::c_char,
    font_size: f32,
) {
    let text = if text.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
    };
    push_ui_cmd(engine, UiCommand::Label { text, font_size });
}

#[no_mangle]
pub extern "C" fn teleology_ui_button(
    engine: *mut TeleologyEngine,
    id: u32,
    text: *const std::ffi::c_char,
) {
    let text = if text.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
    };
    push_ui_cmd(engine, UiCommand::Button { id, text });
}

#[no_mangle]
pub extern "C" fn teleology_ui_button_was_clicked(engine: *mut TeleologyEngine, id: u32) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    let Some(buf) = world.get_resource::<UiCommandBuffer>() else { return 0 };
    if buf.was_clicked(id) { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn teleology_ui_progress_bar(
    engine: *mut TeleologyEngine,
    fraction: f32,
    text: *const std::ffi::c_char,
    width: f32,
) {
    let text = if text.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
    };
    push_ui_cmd(engine, UiCommand::ProgressBar { fraction, text, w: width });
}

#[no_mangle]
pub extern "C" fn teleology_ui_image(
    engine: *mut TeleologyEngine,
    path: *const std::ffi::c_char,
    w: f32,
    h: f32,
) {
    let path = if path.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    push_ui_cmd(engine, UiCommand::Image { path, w, h });
}

#[no_mangle]
pub extern "C" fn teleology_ui_separator(engine: *mut TeleologyEngine) {
    push_ui_cmd(engine, UiCommand::Separator);
}

#[no_mangle]
pub extern "C" fn teleology_ui_spacing(engine: *mut TeleologyEngine, amount: f32) {
    push_ui_cmd(engine, UiCommand::Spacing { amount });
}

#[no_mangle]
pub extern "C" fn teleology_ui_set_color(
    engine: *mut TeleologyEngine,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) {
    push_ui_cmd(engine, UiCommand::SetColor { r, g, b, a });
}

#[no_mangle]
pub extern "C" fn teleology_ui_set_font_size(engine: *mut TeleologyEngine, size: f32) {
    push_ui_cmd(engine, UiCommand::SetFontSize { size });
}

// --- UI Prefabs ---

#[no_mangle]
pub extern "C" fn teleology_ui_prefab_begin(
    engine: *mut TeleologyEngine,
    name: *const std::ffi::c_char,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let name = if name.is_null() {
        return;
    } else {
        unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_ui_buffer(world);
    if let Some(mut buf) = world.get_resource_mut::<UiCommandBuffer>() {
        buf.begin_recording(&name);
    }
}

#[no_mangle]
pub extern "C" fn teleology_ui_prefab_end(engine: *mut TeleologyEngine) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_ui_buffer(world);
    ensure_prefab_registry(world);
    let prefab = {
        let Some(mut buf) = world.get_resource_mut::<UiCommandBuffer>() else { return };
        buf.end_recording()
    };
    if let Some(prefab) = prefab {
        if let Some(mut reg) = world.get_resource_mut::<UiPrefabRegistry>() {
            reg.insert(prefab);
        }
    }
}

#[no_mangle]
pub extern "C" fn teleology_ui_prefab_instantiate(
    engine: *mut TeleologyEngine,
    name: *const std::ffi::c_char,
    params: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let name = if name.is_null() {
        return 0;
    } else {
        unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned()
    };
    // Parse NUL-separated params
    let param_strs: Vec<String> = if params.is_null() {
        Vec::new()
    } else {
        let mut result = Vec::new();
        let mut ptr = params;
        loop {
            let s = unsafe { CStr::from_ptr(ptr) };
            let bytes = s.to_bytes();
            if bytes.is_empty() {
                break;
            }
            result.push(s.to_string_lossy().into_owned());
            ptr = unsafe { ptr.add(bytes.len() + 1) };
        }
        result
    };
    let param_refs: Vec<&str> = param_strs.iter().map(String::as_str).collect();

    let world = unsafe { &mut *ctx.world.get() };
    ensure_prefab_registry(world);
    ensure_ui_buffer(world);

    let expanded = {
        let Some(reg) = world.get_resource::<UiPrefabRegistry>() else { return 0 };
        let Some(prefab) = reg.get(&name) else { return 0 };
        prefab.instantiate(&param_refs)
    };
    if let Some(mut buf) = world.get_resource_mut::<UiCommandBuffer>() {
        for cmd in expanded {
            buf.push(cmd);
        }
    }
    1
}

#[no_mangle]
pub extern "C" fn teleology_ui_prefab_delete(
    engine: *mut TeleologyEngine,
    name: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let name = if name.is_null() {
        return 0;
    } else {
        unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_prefab_registry(world);
    let Some(mut reg) = world.get_resource_mut::<UiPrefabRegistry>() else { return 0 };
    if reg.remove(&name).is_some() { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn teleology_ui_prefab_save(
    engine: *mut TeleologyEngine,
    name: *const std::ffi::c_char,
    path: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let name = if name.is_null() { return 0 } else {
        unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned()
    };
    let path = if path.is_null() { return 0 } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_prefab_registry(world);
    let Some(reg) = world.get_resource::<UiPrefabRegistry>() else { return 0 };
    if reg.save_prefab(&name, std::path::Path::new(&path)).is_ok() { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn teleology_ui_prefab_load(
    engine: *mut TeleologyEngine,
    path: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let path = if path.is_null() { return 0 } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_prefab_registry(world);
    let Some(mut reg) = world.get_resource_mut::<UiPrefabRegistry>() else { return 0 };
    if reg.load_prefab(std::path::Path::new(&path)).is_ok() { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn teleology_ui_prefab_save_all(
    engine: *mut TeleologyEngine,
    path: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let path = if path.is_null() { return 0 } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_prefab_registry(world);
    let Some(reg) = world.get_resource::<UiPrefabRegistry>() else { return 0 };
    if reg.save_to_file(std::path::Path::new(&path)).is_ok() { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn teleology_ui_prefab_load_all(
    engine: *mut TeleologyEngine,
    path: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let path = if path.is_null() { return 0 } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    match UiPrefabRegistry::load_from_file(std::path::Path::new(&path)) {
        Ok(reg) => { world.insert_resource(reg); 1 }
        Err(_) => 0,
    }
}

#[no_mangle]
pub extern "C" fn teleology_ui_prefab_count(engine: *mut TeleologyEngine) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    world.get_resource::<UiPrefabRegistry>()
        .map(|r| r.prefabs.len() as u32)
        .unwrap_or(0)
}
