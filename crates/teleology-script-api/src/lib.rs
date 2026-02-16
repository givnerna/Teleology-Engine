//! Stable C API for C++ scripting. Engine implements the "engine" side;
//! script DLLs implement the "script" callbacks. On WebGL, scripting is unavailable.

use std::path::Path;

pub mod ffi;

/// Opaque engine context passed to script callbacks. Scripts do not dereference it.
#[repr(C)]
pub struct TeleologyEngine(*mut std::ffi::c_void);

/// C-compatible game date for scripts.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CGameDate {
    pub day: u16,
    pub month: u8,
    pub year: i32,
}

/// C-compatible province id (1-based index).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CProvinceId {
    pub raw: u32,
}

/// C-compatible nation id (1-based index).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CNationId {
    pub raw: u32,
}

/// C-compatible tag type id.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CTagTypeId {
    pub raw: u32,
}

/// C-compatible tag value id.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CTagId {
    pub raw: u32,
}

/// C-compatible army id.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CArmyId {
    pub raw: u32,
}

/// C-compatible progress tree id.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CTreeId {
    pub raw: u32,
}

/// C-compatible progress node id.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CNodeId {
    pub raw: u32,
}

/// Script API vtable: callbacks the engine calls into the loaded C++ library.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TeleologyScriptApi {
    pub version: u32,
    pub on_init: extern "C" fn(engine: *mut TeleologyEngine),
    pub on_daily_tick: extern "C" fn(engine: *mut TeleologyEngine),
    pub on_monthly_tick: extern "C" fn(engine: *mut TeleologyEngine),
    pub on_yearly_tick: extern "C" fn(engine: *mut TeleologyEngine),
    pub on_event: extern "C" fn(engine: *mut TeleologyEngine, event_id: u32, payload: *const u8, payload_len: u32),
    /// Input callbacks (optional; set to NULL if unused). Engine calls when input occurs.
    pub on_click: Option<extern "C" fn(engine: *mut TeleologyEngine, x: f32, y: f32)>,
    pub on_key_down: Option<extern "C" fn(engine: *mut TeleologyEngine, key_code: u32)>,
    pub on_key_up: Option<extern "C" fn(engine: *mut TeleologyEngine, key_code: u32)>,
}

const SCRIPT_API_VERSION: u32 = 2;

/// Handle to a loaded script library. Keeps the library loaded; drop to unload.
#[cfg(not(target_arch = "wasm32"))]
pub type ScriptHandle = std::sync::Arc<libloading::Library>;

#[cfg(target_arch = "wasm32")]
pub type ScriptHandle = ();

/// Load a script library from path. Returns the handle and API vtable.
/// Supports script API version 1 (no input callbacks) and 2 (with on_click, on_key_down, on_key_up).
/// On WebGL this always returns an error (dynamic loading is not available).
#[cfg(not(target_arch = "wasm32"))]
pub fn load_script_api(path: &Path) -> Result<(ScriptHandle, TeleologyScriptApi), Box<dyn std::error::Error>> {
    use libloading::{Library, Symbol};
    use std::sync::Arc;

    let lib = unsafe { Library::new(path)? };
    let get_api: Symbol<unsafe extern "C" fn() -> *const TeleologyScriptApi> =
        unsafe { lib.get(b"teleology_script_get_api")? };
    let api_ptr = unsafe { get_api() };
    if api_ptr.is_null() {
        return Err("teleology_script_get_api returned null".into());
    }
    let version = unsafe { std::ptr::read(api_ptr as *const u32) };
    let ptr_size = std::mem::size_of::<*const std::ffi::c_void>();
    let api = if version == 1 {
        // Version 1: read only the original 6 callback fields; input callbacks are None.
        let base = api_ptr as *const u8;
        let on_init = unsafe { std::ptr::read(base.add(4) as *const _) };
        let on_daily_tick = unsafe { std::ptr::read(base.add(4 + ptr_size) as *const _) };
        let on_monthly_tick = unsafe { std::ptr::read(base.add(4 + 2 * ptr_size) as *const _) };
        let on_yearly_tick = unsafe { std::ptr::read(base.add(4 + 3 * ptr_size) as *const _) };
        let on_event = unsafe { std::ptr::read(base.add(4 + 4 * ptr_size) as *const _) };
        TeleologyScriptApi {
            version: 1,
            on_init,
            on_daily_tick,
            on_monthly_tick,
            on_yearly_tick,
            on_event,
            on_click: None,
            on_key_down: None,
            on_key_up: None,
        }
    } else if version == SCRIPT_API_VERSION {
        unsafe { std::ptr::read(api_ptr) }
    } else {
        return Err(format!(
            "Script API version mismatch: got {}, expected 1 or {}",
            version, SCRIPT_API_VERSION
        )
        .into());
    };
    Ok((Arc::new(lib), api))
}

#[cfg(target_arch = "wasm32")]
pub fn load_script_api(_path: &Path) -> Result<(ScriptHandle, TeleologyScriptApi), Box<dyn std::error::Error>> {
    Err("C++ scripting is not available on WebGL. Use native (Windows/Mac/Linux) for scripted games.".into())
}

/// Platform-specific library name for a script (no path, no extension).
/// Use when building script path: e.g. `dir.join(script_library_filename("game"))`.
/// - Windows: `game.dll`
/// - macOS: `libgame.dylib`
/// - Linux/other: `libgame.so`
pub fn script_library_filename(base_name: &str) -> std::path::PathBuf {
    #[cfg(target_os = "windows")]
    return std::path::PathBuf::from(format!("{}.dll", base_name));

    #[cfg(target_os = "macos")]
    return std::path::PathBuf::from(format!("lib{}.dylib", base_name));

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    return std::path::PathBuf::from(format!("lib{}.so", base_name));
}

/// Input key codes. Align with [keyboard-types](https://docs.rs/keyboard-types) (W3C UI Events) where possible.
/// OnKeyDown/OnKeyUp receive every key; common keys use the constants below; unmapped keys use 0x8000+ (editor-specific).
pub mod key_codes {
    pub const KEY_SPACE: u32 = 32;
    pub const KEY_ESCAPE: u32 = 256;
    pub const KEY_ENTER: u32 = 257;
    pub const KEY_TAB: u32 = 258;
    pub const KEY_BACKSPACE: u32 = 259;
    pub const KEY_INSERT: u32 = 260;
    pub const KEY_DELETE: u32 = 261;
    pub const KEY_RIGHT: u32 = 262;
    pub const KEY_LEFT: u32 = 263;
    pub const KEY_DOWN: u32 = 264;
    pub const KEY_UP: u32 = 265;
    pub const KEY_HOME: u32 = 266;
    pub const KEY_END: u32 = 267;
    pub const KEY_PAGE_UP: u32 = 268;
    pub const KEY_PAGE_DOWN: u32 = 269;
    pub const KEY_F1: u32 = 270;
    pub const KEY_F2: u32 = 271;
    pub const KEY_F3: u32 = 272;
    pub const KEY_F4: u32 = 273;
    pub const KEY_F5: u32 = 274;
    pub const KEY_F6: u32 = 275;
    pub const KEY_F7: u32 = 276;
    pub const KEY_F8: u32 = 277;
    pub const KEY_F9: u32 = 278;
    pub const KEY_F10: u32 = 279;
    pub const KEY_F11: u32 = 280;
    pub const KEY_F12: u32 = 281;
    /// Letters A–Z and digits 0–9 use ASCII (65–90, 48–57). Other keys use 256+; any key ≥ 0x8000 is editor-specific.
    pub fn key_from_ascii(c: u8) -> u32 {
        c as u32
    }
}

/// Re-export of [keyboard_types::Code] for input handling. Standard W3C UI Events physical key type;
/// used by winit and other crates. C API uses `u32` (see [key_codes]); hosts can map Code to u32 as needed.
pub use keyboard_types::Code;

/// Engine-side API: what the engine exposes to scripts via TeleologyEngine.
pub trait EngineApi {
    fn get_date(&self) -> CGameDate;
    fn get_province_count(&self) -> u32;
    fn get_province_owner(&self, province: CProvinceId) -> CNationId;
    fn set_province_owner(&mut self, province: CProvinceId, nation: CNationId);

    // --- Tags ---
    fn register_tag_type(&mut self, _name_utf8: &[u8]) -> CTagTypeId {
        CTagTypeId { raw: 0 }
    }
    fn register_tag(&mut self, _ty: CTagTypeId, _name_utf8: &[u8]) -> CTagId {
        CTagId { raw: 0 }
    }
    fn get_province_tag(&self, _province: CProvinceId, _ty: CTagTypeId) -> CTagId {
        CTagId { raw: 0 }
    }
    fn set_province_tag(&mut self, _province: CProvinceId, _ty: CTagTypeId, _tag: CTagId) {}
    fn get_nation_tag(&self, _nation: CNationId, _ty: CTagTypeId) -> CTagId {
        CTagId { raw: 0 }
    }
    fn set_nation_tag(&mut self, _nation: CNationId, _ty: CTagTypeId, _tag: CTagId) {}

    // --- EventBus ---
    fn eventbus_publish(&mut self, _topic_utf8: &[u8], _payload_type_id: u32, _payload: &[u8]) {}
    fn eventbus_poll(&mut self, _payload_out: &mut [u8]) -> (u32, u32, u32) {
        // (topic_raw, payload_type_id, payload_len)
        (0, 0, 0)
    }
    fn eventbus_topic_name(&self, _topic_raw: u32, _out: &mut [u8]) -> u32 {
        0
    }

    // --- Progress trees (nation scope) ---
    fn progress_unlock_nation(&mut self, _nation: CNationId, _tree: CTreeId, _node: CNodeId) {}
    fn progress_is_unlocked_nation(&self, _nation: CNationId, _tree: CTreeId, _node: CNodeId) -> bool {
        false
    }

    // --- Armies (minimal) ---
    fn spawn_army(&mut self, _owner: CNationId, _location: CProvinceId) -> CArmyId {
        CArmyId { raw: 0 }
    }
    fn set_army_location(&mut self, _army: CArmyId, _location: CProvinceId) {}

    // --- Input (polling; callbacks are invoked by engine when host feeds input) ---
    fn input_last_click(&self) -> Option<(f32, f32)> {
        None
    }
    fn input_key_down(&self, _key_code: u32) -> bool {
        false
    }
}
