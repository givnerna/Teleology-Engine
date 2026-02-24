//! Events C API: event bus, pop-up events, keywords, event style.

use std::ffi::CStr;
use std::num::NonZeroU32;
use teleology_core::{
    publish_event, queue_event, register_builtin_templates,
    ActiveEvent, EventBus, EventChoice, EventDefinition, EventId, EventPopupStyle,
    EventQueue, EventRegistry, EventScope, EventScopeRef, EventTemplate,
    GameWorld, KeywordEntry, KeywordRegistry, PopupAnchor,
};
use teleology_script_api::TeleologyEngine;

use crate::context_from_engine;

fn ensure_event_bus(world: &mut GameWorld) {
    if world.get_resource::<EventBus>().is_none() {
        world.insert_resource(EventBus::new());
    }
}

fn ensure_event_system(world: &mut GameWorld) {
    if world.get_resource::<EventRegistry>().is_none() {
        world.insert_resource(EventRegistry::new());
        world.insert_resource(EventQueue::default());
        world.insert_resource(ActiveEvent::default());
        world.insert_resource(EventPopupStyle::default());
        // Auto-load keywords.json from the working directory if present.
        let mut kw = KeywordRegistry::default();
        let path = std::path::Path::new("keywords.json");
        if path.exists() {
            let _ = kw.load_from_file(path);
        }
        world.insert_resource(kw);
    }
}

#[no_mangle]
pub extern "C" fn teleology_eventbus_publish(
    engine: *mut TeleologyEngine,
    topic: *const std::ffi::c_char,
    payload_type_id: u32,
    payload: *const u8,
    payload_len: u32,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_bus(world);
    let Some(topic) = (!topic.is_null()).then(|| unsafe { CStr::from_ptr(topic) }) else { return };
    let topic = topic.to_string_lossy();
    let bytes = if payload.is_null() || payload_len == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(payload, payload_len as usize) }.to_vec()
    };
    publish_event(world, &topic, EventScopeRef::global(), payload_type_id, bytes, 0);
}

/// Poll next eventbus message. Returns payload_len (0 if none).
#[no_mangle]
pub extern "C" fn teleology_eventbus_poll(
    engine: *mut TeleologyEngine,
    topic_raw_out: *mut u32,
    payload_type_out: *mut u32,
    payload_out: *mut u8,
    payload_cap: u32,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_bus(world);
    let Some(mut bus) = world.get_resource_mut::<EventBus>() else { return 0 };
    let Some(env) = bus.poll() else { return 0 };
    unsafe {
        if !topic_raw_out.is_null() { *topic_raw_out = env.topic.raw(); }
        if !payload_type_out.is_null() { *payload_type_out = env.payload.payload_type_id; }
    }
    let required = env.payload.bytes.len() as u32;
    if !payload_out.is_null() && payload_cap > 0 {
        let n = (payload_cap as usize).min(env.payload.bytes.len());
        unsafe { std::ptr::copy_nonoverlapping(env.payload.bytes.as_ptr(), payload_out, n); }
    }
    required
}

#[no_mangle]
pub extern "C" fn teleology_eventbus_topic_name(
    engine: *mut TeleologyEngine,
    topic_raw: u32,
    out: *mut std::ffi::c_char,
    out_cap: u32,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_bus(world);
    let Some(bus) = world.get_resource::<EventBus>() else { return 0 };
    let Some(nz) = NonZeroU32::new(topic_raw) else { return 0 };
    let name = bus.topic_name(teleology_core::EventTopicId(nz)).unwrap_or("");
    if out.is_null() || out_cap == 0 { return name.len() as u32; }
    let bytes = name.as_bytes();
    let n = (out_cap as usize).saturating_sub(1).min(bytes.len());
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out as *mut u8, n);
        *out.add(n) = 0;
    }
    name.len() as u32
}

// --- Pop-up events ---

/// Define a new event. Returns the event_id (0 on failure).
#[no_mangle]
pub extern "C" fn teleology_event_define(
    engine: *mut TeleologyEngine,
    title: *const std::ffi::c_char,
    body: *const std::ffi::c_char,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let title = if title.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(title) }.to_string_lossy().into_owned()
    };
    let body = if body.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(body) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<EventRegistry>() else { return 0 };
    let def = EventDefinition {
        id: EventId(NonZeroU32::new(1).unwrap()), // placeholder; insert() reassigns
        title,
        body,
        choices: Vec::new(),
        image: String::new(),
        image_w: 0.0,
        image_h: 0.0,
    };
    reg.insert(def).raw()
}

/// Create a new event from a built-in template. Returns the event_id.
/// template: 0=Notification, 1=BinaryChoice, 2=ThreeWayChoice, 3=Narrative, 4=DiplomaticProposal.
#[no_mangle]
pub extern "C" fn teleology_event_from_template(
    engine: *mut TeleologyEngine,
    template: u32,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let tmpl = match template {
        0 => EventTemplate::Notification,
        1 => EventTemplate::BinaryChoice,
        2 => EventTemplate::ThreeWayChoice,
        3 => EventTemplate::Narrative,
        4 => EventTemplate::DiplomaticProposal,
        _ => return 0,
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<EventRegistry>() else { return 0 };
    reg.insert(tmpl.create()).raw()
}

/// Add a choice to an event. Returns the choice index (0-based), or -1 on failure.
#[no_mangle]
pub extern "C" fn teleology_event_add_choice(
    engine: *mut TeleologyEngine,
    event_id: u32,
    text: *const std::ffi::c_char,
    next_event_id: u32,
) -> i32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return -1 };
    let text = if text.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<EventRegistry>() else { return -1 };
    let Some(def) = reg.events.get_mut(&event_id) else { return -1 };
    let next = NonZeroU32::new(next_event_id).map(EventId);
    let idx = def.choices.len();
    def.choices.push(EventChoice {
        text,
        next_event: next,
        effects_payload: Vec::new(),
    });
    idx as i32
}

/// Set the text of an existing choice. Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn teleology_event_set_choice_text(
    engine: *mut TeleologyEngine,
    event_id: u32,
    choice_idx: u32,
    text: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let text = if text.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<EventRegistry>() else { return 0 };
    let Some(def) = reg.events.get_mut(&event_id) else { return 0 };
    let Some(ch) = def.choices.get_mut(choice_idx as usize) else { return 0 };
    ch.text = text;
    1
}

/// Set the title of an existing event. Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn teleology_event_set_title(
    engine: *mut TeleologyEngine,
    event_id: u32,
    title: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let title = if title.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(title) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<EventRegistry>() else { return 0 };
    let Some(def) = reg.events.get_mut(&event_id) else { return 0 };
    def.title = title;
    1
}

/// Set the body of an existing event. Returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn teleology_event_set_body(
    engine: *mut TeleologyEngine,
    event_id: u32,
    body: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let body = if body.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(body) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<EventRegistry>() else { return 0 };
    let Some(def) = reg.events.get_mut(&event_id) else { return 0 };
    def.body = body;
    1
}

/// Set the image for an event definition.
#[no_mangle]
pub extern "C" fn teleology_event_set_image(
    engine: *mut TeleologyEngine,
    event_id: u32,
    path: *const std::ffi::c_char,
    w: f32,
    h: f32,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let path = if path.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<EventRegistry>() else { return 0 };
    let Some(def) = reg.events.get_mut(&event_id) else { return 0 };
    def.image = path;
    def.image_w = w;
    def.image_h = h;
    1
}

/// Queue an event for display as a pop-up.
#[no_mangle]
pub extern "C" fn teleology_event_queue(
    engine: *mut TeleologyEngine,
    event_id: u32,
    scope_type: u32,
    scope_raw: u32,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let Some(eid) = NonZeroU32::new(event_id) else { return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let scope = EventScope { scope_type, raw: scope_raw, raw_hi: 0 };
    queue_event(world, EventId(eid), scope, Vec::new());
}

/// Get the active event. Returns event_id (0 if no active event).
#[no_mangle]
pub extern "C" fn teleology_event_get_active(
    engine: *mut TeleologyEngine,
    choice_count_out: *mut u32,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    let Some(active) = world.get_resource::<ActiveEvent>() else { return 0 };
    let Some(inst) = &active.current else { return 0 };
    let eid = inst.event_id.raw();
    if !choice_count_out.is_null() {
        let count = world.get_resource::<EventRegistry>()
            .and_then(|r| r.get(inst.event_id))
            .map(|d| d.choices.len() as u32)
            .unwrap_or(0);
        unsafe { *choice_count_out = count; }
    }
    eid
}

/// Get text of the active event (title or body).
#[no_mangle]
pub extern "C" fn teleology_event_get_text(
    engine: *mut TeleologyEngine,
    field: u32,
    out: *mut std::ffi::c_char,
    out_cap: u32,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    let Some(active) = world.get_resource::<ActiveEvent>() else { return 0 };
    let Some(inst) = &active.current else { return 0 };
    let Some(reg) = world.get_resource::<EventRegistry>() else { return 0 };
    let Some(def) = reg.get(inst.event_id) else { return 0 };
    let text = match field {
        0 => &def.title,
        1 => &def.body,
        _ => return 0,
    };
    if out.is_null() || out_cap == 0 { return text.len() as u32; }
    let bytes = text.as_bytes();
    let n = (out_cap as usize).saturating_sub(1).min(bytes.len());
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out as *mut u8, n);
        *out.add(n) = 0;
    }
    text.len() as u32
}

/// Get choice text for the active event.
#[no_mangle]
pub extern "C" fn teleology_event_get_choice_text(
    engine: *mut TeleologyEngine,
    choice_idx: u32,
    out: *mut std::ffi::c_char,
    out_cap: u32,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &*ctx.world.get() };
    let Some(active) = world.get_resource::<ActiveEvent>() else { return 0 };
    let Some(inst) = &active.current else { return 0 };
    let Some(reg) = world.get_resource::<EventRegistry>() else { return 0 };
    let Some(def) = reg.get(inst.event_id) else { return 0 };
    let Some(ch) = def.choices.get(choice_idx as usize) else { return 0 };
    let text = &ch.text;
    if out.is_null() || out_cap == 0 { return text.len() as u32; }
    let bytes = text.as_bytes();
    let n = (out_cap as usize).saturating_sub(1).min(bytes.len());
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out as *mut u8, n);
        *out.add(n) = 0;
    }
    text.len() as u32
}

/// Choose an option for the active event.
#[no_mangle]
pub extern "C" fn teleology_event_choose(
    engine: *mut TeleologyEngine,
    choice_idx: u32,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let (inst, next_event) = {
        let Some(active) = world.get_resource::<ActiveEvent>() else { return 0 };
        let Some(inst) = active.current.clone() else { return 0 };
        let Some(reg) = world.get_resource::<EventRegistry>() else { return 0 };
        let Some(def) = reg.get(inst.event_id) else { return 0 };
        let Some(ch) = def.choices.get(choice_idx as usize) else { return 0 };
        (inst, ch.next_event)
    };
    if let Some(mut active) = world.get_resource_mut::<ActiveEvent>() {
        active.current = None;
    }
    if let Some(next) = next_event {
        queue_event(world, next, inst.scope, inst.payload);
    }
    1
}

// --- Event style ---

#[no_mangle]
pub extern "C" fn teleology_event_style_reset(engine: *mut TeleologyEngine) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    if let Some(mut style) = world.get_resource_mut::<EventPopupStyle>() {
        *style = EventPopupStyle::default();
    }
}

#[no_mangle]
pub extern "C" fn teleology_event_style_set_anchor(
    engine: *mut TeleologyEngine,
    anchor: u32,
    x: f32,
    y: f32,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    if let Some(mut style) = world.get_resource_mut::<EventPopupStyle>() {
        style.anchor = match anchor {
            0 => PopupAnchor::Center,
            _ => PopupAnchor::Fixed { x, y },
        };
    }
}

#[no_mangle]
pub extern "C" fn teleology_event_style_set_colors(
    engine: *mut TeleologyEngine,
    bg_r: u8, bg_g: u8, bg_b: u8, bg_a: u8,
    title_r: u8, title_g: u8, title_b: u8, title_a: u8,
    body_r: u8, body_g: u8, body_b: u8, body_a: u8,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    if let Some(mut style) = world.get_resource_mut::<EventPopupStyle>() {
        style.bg_color = [bg_r, bg_g, bg_b, bg_a];
        style.title_color = [title_r, title_g, title_b, title_a];
        style.body_color = [body_r, body_g, body_b, body_a];
    }
}

#[no_mangle]
pub extern "C" fn teleology_event_style_set_image(
    engine: *mut TeleologyEngine,
    path: *const std::ffi::c_char,
    w: f32,
    h: f32,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let path = if path.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    if let Some(mut style) = world.get_resource_mut::<EventPopupStyle>() {
        style.image_path = path;
        style.image_w = w;
        style.image_h = h;
    }
}

#[no_mangle]
pub extern "C" fn teleology_event_style_set_layout(
    engine: *mut TeleologyEngine,
    width: f32,
    modal: u8,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    if let Some(mut style) = world.get_resource_mut::<EventPopupStyle>() {
        style.width = width;
        style.modal = modal != 0;
    }
}

/// Register all built-in event templates.
#[no_mangle]
pub extern "C" fn teleology_event_register_templates(
    engine: *mut TeleologyEngine,
    ids_out: *mut u32,
) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<EventRegistry>() else { return };
    let ids = register_builtin_templates(&mut reg);
    if !ids_out.is_null() {
        for (i, id) in ids.iter().enumerate() {
            unsafe { *ids_out.add(i) = id.raw(); }
        }
    }
}

// --- Keyword tooltip system ---

#[no_mangle]
pub extern "C" fn teleology_keyword_add(
    engine: *mut TeleologyEngine,
    keyword: *const std::ffi::c_char,
    title: *const std::ffi::c_char,
    description: *const std::ffi::c_char,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return u32::MAX };
    let keyword = if keyword.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(keyword) }.to_string_lossy().into_owned()
    };
    let title = if title.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(title) }.to_string_lossy().into_owned()
    };
    let description = if description.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(description) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<KeywordRegistry>() else { return u32::MAX };
    reg.add(KeywordEntry {
        keyword,
        title,
        description,
        icon: String::new(),
        color: [0, 0, 0, 0],
    }) as u32
}

#[no_mangle]
pub extern "C" fn teleology_keyword_set_icon(
    engine: *mut TeleologyEngine,
    index: u32,
    path: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let path = if path.is_null() { String::new() } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<KeywordRegistry>() else { return 0 };
    let Some(entry) = reg.entries.get_mut(index as usize) else { return 0 };
    entry.icon = path;
    1
}

#[no_mangle]
pub extern "C" fn teleology_keyword_set_color(
    engine: *mut TeleologyEngine,
    index: u32,
    r: u8, g: u8, b: u8, a: u8,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<KeywordRegistry>() else { return 0 };
    let Some(entry) = reg.entries.get_mut(index as usize) else { return 0 };
    entry.color = [r, g, b, a];
    1
}

#[no_mangle]
pub extern "C" fn teleology_keyword_remove(
    engine: *mut TeleologyEngine,
    index: u32,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<KeywordRegistry>() else { return 0 };
    if reg.remove(index as usize) { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn teleology_keyword_clear(engine: *mut TeleologyEngine) {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    if let Some(mut reg) = world.get_resource_mut::<KeywordRegistry>() {
        reg.clear();
    }
}

#[no_mangle]
pub extern "C" fn teleology_keyword_count(engine: *mut TeleologyEngine) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(reg) = world.get_resource::<KeywordRegistry>() else { return 0 };
    reg.entries.len() as u32
}

#[no_mangle]
pub extern "C" fn teleology_keyword_load_file(
    engine: *mut TeleologyEngine,
    path: *const std::ffi::c_char,
) -> u32 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return u32::MAX };
    let path = if path.is_null() { return u32::MAX } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(mut reg) = world.get_resource_mut::<KeywordRegistry>() else { return u32::MAX };
    match reg.load_from_file(std::path::Path::new(&path)) {
        Ok(n) => n as u32,
        Err(_) => u32::MAX,
    }
}

#[no_mangle]
pub extern "C" fn teleology_keyword_save_file(
    engine: *mut TeleologyEngine,
    path: *const std::ffi::c_char,
) -> u8 {
    let ctx = match context_from_engine(engine) { Some(c) => c, None => return 0 };
    let path = if path.is_null() { return 0 } else {
        unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned()
    };
    let world = unsafe { &mut *ctx.world.get() };
    ensure_event_system(world);
    let Some(reg) = world.get_resource::<KeywordRegistry>() else { return 0 };
    match reg.save_to_file(std::path::Path::new(&path)) {
        Ok(()) => 1,
        Err(_) => 0,
    }
}
