/**
 * Teleology Engine — C API for C++ scripting (grand strategy games).
 * Script DLLs implement teleology_script_get_api(); the engine provides the rest.
 */
#ifndef TELEOLOGY_SCRIPT_API_H
#define TELEOLOGY_SCRIPT_API_H

#ifdef __cplusplus
extern "C" {
#endif

#include <stdint.h>

/* Opaque engine context. Passed to script callbacks and to engine API calls. */
typedef struct TeleologyEngine TeleologyEngine;

typedef struct CGameDate {
    uint16_t day;
    uint8_t month;
    int32_t year;
} CGameDate;

typedef struct CProvinceId { uint32_t raw; } CProvinceId;
typedef struct CNationId   { uint32_t raw; } CNationId;
typedef struct CTagTypeId  { uint32_t raw; } CTagTypeId;
typedef struct CTagId      { uint32_t raw; } CTagId;
typedef struct CArmyId     { uint32_t raw; } CArmyId;
typedef struct CTreeId     { uint32_t raw; } CTreeId;
typedef struct CNodeId     { uint32_t raw; } CNodeId;

/* Script implements this struct and returns it from teleology_script_get_api(). */
/* version 1 = original; version 2 = with input callbacks (on_click/on_key_down/on_key_up; may be NULL). */
typedef struct TeleologyScriptApi {
    uint32_t version;
    void (*on_init)(TeleologyEngine* engine);
    void (*on_daily_tick)(TeleologyEngine* engine);
    void (*on_monthly_tick)(TeleologyEngine* engine);
    void (*on_yearly_tick)(TeleologyEngine* engine);
    void (*on_event)(TeleologyEngine* engine, uint32_t event_id, const uint8_t* payload, uint32_t payload_len);
    /* Input callbacks (optional; set to NULL if unused). Engine calls when input occurs. */
    void (*on_click)(TeleologyEngine* engine, float x, float y);
    void (*on_key_down)(TeleologyEngine* engine, uint32_t key_code);
    void (*on_key_up)(TeleologyEngine* engine, uint32_t key_code);
} TeleologyScriptApi;

/* Key codes for input API. Align with keyboard-types (W3C UI Events) where possible.
   Letters/digits use ASCII (e.g. 65='A', 32=Space). Special keys: */
#define TELEOLOGY_KEY_SPACE     32
#define TELEOLOGY_KEY_ESCAPE    256
#define TELEOLOGY_KEY_ENTER     257
#define TELEOLOGY_KEY_TAB       258
#define TELEOLOGY_KEY_BACKSPACE 259
#define TELEOLOGY_KEY_INSERT    260
#define TELEOLOGY_KEY_DELETE    261
#define TELEOLOGY_KEY_RIGHT     262
#define TELEOLOGY_KEY_LEFT      263
#define TELEOLOGY_KEY_DOWN      264
#define TELEOLOGY_KEY_UP        265
#define TELEOLOGY_KEY_HOME      266
#define TELEOLOGY_KEY_END       267
#define TELEOLOGY_KEY_PAGE_UP   268
#define TELEOLOGY_KEY_PAGE_DOWN 269
#define TELEOLOGY_KEY_F1        270
#define TELEOLOGY_KEY_F2        271
#define TELEOLOGY_KEY_F3        272
#define TELEOLOGY_KEY_F4        273
#define TELEOLOGY_KEY_F5        274
#define TELEOLOGY_KEY_F6        275
#define TELEOLOGY_KEY_F7        276
#define TELEOLOGY_KEY_F8        277
#define TELEOLOGY_KEY_F9        278
#define TELEOLOGY_KEY_F10       279
#define TELEOLOGY_KEY_F11       280
#define TELEOLOGY_KEY_F12       281
/* Any key with code >= 0x8000 is an unmapped egui key (F13+, punctuation, etc.); still delivered to on_key_down/on_key_up. */

/* --- Script must export --- */
const TeleologyScriptApi* teleology_script_get_api(void);

/* --- Engine provides (link against engine binary/dll) --- */
CGameDate    teleology_get_date(TeleologyEngine* engine);
uint32_t     teleology_get_province_count(TeleologyEngine* engine);
CNationId    teleology_get_province_owner(TeleologyEngine* engine, CProvinceId province);
void         teleology_set_province_owner(TeleologyEngine* engine, CProvinceId province, CNationId nation);

/* --- Optional modules: tags (no-op if not enabled) --- */
CTagTypeId   teleology_tags_register_type(TeleologyEngine* engine, const char* name_utf8);
CTagId       teleology_tags_register_tag(TeleologyEngine* engine, CTagTypeId ty, const char* name_utf8);
CTagId       teleology_province_get_tag(TeleologyEngine* engine, CProvinceId province, CTagTypeId ty);
void         teleology_province_set_tag(TeleologyEngine* engine, CProvinceId province, CTagTypeId ty, CTagId tag);
CTagId       teleology_nation_get_tag(TeleologyEngine* engine, CNationId nation, CTagTypeId ty);
void         teleology_nation_set_tag(TeleologyEngine* engine, CNationId nation, CTagTypeId ty, CTagId tag);

/* --- Optional modules: EventBus (dev-facing) --- */
void         teleology_eventbus_publish(TeleologyEngine* engine, const char* topic_utf8, uint32_t payload_type_id, const uint8_t* payload, uint32_t payload_len);
/* Returns required payload_len (0 if none). Copies up to payload_cap bytes into payload_out. */
uint32_t     teleology_eventbus_poll(TeleologyEngine* engine, uint32_t* topic_raw_out, uint32_t* payload_type_out, uint8_t* payload_out, uint32_t payload_cap);
/* Writes a NUL-terminated topic name; returns full length (excluding NUL). */
uint32_t     teleology_eventbus_topic_name(TeleologyEngine* engine, uint32_t topic_raw, char* out, uint32_t out_cap);

/* --- Optional modules: progress trees (nation scope) --- */
void         teleology_progress_unlock_nation(TeleologyEngine* engine, CNationId nation, CTreeId tree, CNodeId node);
uint8_t      teleology_progress_is_unlocked_nation(TeleologyEngine* engine, CNationId nation, CTreeId tree, CNodeId node);

/* --- Optional modules: armies (minimal) --- */
CArmyId      teleology_spawn_army(TeleologyEngine* engine, CNationId owner, CProvinceId location);
void         teleology_set_army_location(TeleologyEngine* engine, CArmyId army, CProvinceId location);

/* --- Input (OnClick, OnKeyDown, OnKeyUp): host feeds input; scripts poll or use callbacks --- */
/* Returns 1 if there was a click (writes x,y to out); 0 otherwise. */
int          teleology_input_last_click(TeleologyEngine* engine, float* x_out, float* y_out);
/* Returns 1 if key is currently down; 0 otherwise. */
int          teleology_input_key_down(TeleologyEngine* engine, uint32_t key_code);

/* --- Game UI (immediate-mode; call during tick; rendered by engine each frame) ---
 *
 * Scripts declare UI each frame by calling begin/end pairs and widget functions.
 * The engine renders them after the tick. Button clicks are available next frame
 * via teleology_ui_button_was_clicked().
 *
 * Example:
 *   teleology_ui_begin_window(engine, "HUD", 0, 0, 400, 40);
 *     teleology_ui_label(engine, "Gold: 1234");
 *     teleology_ui_button(engine, 1, "Diplomacy");
 *   teleology_ui_end_window(engine);
 *   if (teleology_ui_button_was_clicked(engine, 1)) { ... }
 */

/* Containers */
void         teleology_ui_begin_window(TeleologyEngine* engine, const char* title, float x, float y, float w, float h);
void         teleology_ui_end_window(TeleologyEngine* engine);
void         teleology_ui_begin_horizontal(TeleologyEngine* engine);
void         teleology_ui_end_horizontal(TeleologyEngine* engine);
void         teleology_ui_begin_vertical(TeleologyEngine* engine);
void         teleology_ui_end_vertical(TeleologyEngine* engine);

/* Widgets */
void         teleology_ui_label(TeleologyEngine* engine, const char* text);
void         teleology_ui_label_sized(TeleologyEngine* engine, const char* text, float font_size);
void         teleology_ui_button(TeleologyEngine* engine, uint32_t id, const char* text);
/* Returns 1 if the button with this id was clicked last frame; 0 otherwise. */
uint8_t      teleology_ui_button_was_clicked(TeleologyEngine* engine, uint32_t id);
void         teleology_ui_progress_bar(TeleologyEngine* engine, float fraction, const char* text, float width);
void         teleology_ui_image(TeleologyEngine* engine, const char* path, float w, float h);
void         teleology_ui_separator(TeleologyEngine* engine);
void         teleology_ui_spacing(TeleologyEngine* engine, float amount);

/* Styling (applies to the next widget only) */
void         teleology_ui_set_color(TeleologyEngine* engine, uint8_t r, uint8_t g, uint8_t b, uint8_t a);
void         teleology_ui_set_font_size(TeleologyEngine* engine, float size);

/* --- UI Prefabs (reusable templates; record, instantiate, save/load) ---
 *
 * Record a prefab: call prefab_begin, then any teleology_ui_* calls (they go
 * into the recording buffer instead of rendering), then prefab_end to save it.
 * Text fields may contain {0}, {1}, … placeholders for substitution.
 *
 * Instantiate: prefab_instantiate replays the saved commands into the render
 * buffer, substituting placeholders with the NUL-separated params string.
 *
 * Example:
 *   // Record once (e.g. in on_init)
 *   teleology_ui_prefab_begin(engine, "resource_bar");
 *     teleology_ui_begin_horizontal(engine);
 *       teleology_ui_label(engine, "{0}:");
 *       teleology_ui_progress_bar(engine, 0.0f, "{1}", 200.0f);
 *     teleology_ui_end_horizontal(engine);
 *   teleology_ui_prefab_end(engine);
 *
 *   // Use every frame (in on_daily_tick)
 *   teleology_ui_prefab_instantiate(engine, "resource_bar", "Gold\0""75%\0");
 */

/* Recording */
void         teleology_ui_prefab_begin(TeleologyEngine* engine, const char* name);
void         teleology_ui_prefab_end(TeleologyEngine* engine);

/* Instantiation (params: NUL-separated, double-NUL-terminated) */
uint8_t      teleology_ui_prefab_instantiate(TeleologyEngine* engine, const char* name, const char* params);

/* Management */
uint8_t      teleology_ui_prefab_delete(TeleologyEngine* engine, const char* name);
uint32_t     teleology_ui_prefab_count(TeleologyEngine* engine);

/* Persistence */
uint8_t      teleology_ui_prefab_save(TeleologyEngine* engine, const char* name, const char* path);
uint8_t      teleology_ui_prefab_load(TeleologyEngine* engine, const char* path);
uint8_t      teleology_ui_prefab_save_all(TeleologyEngine* engine, const char* path);
uint8_t      teleology_ui_prefab_load_all(TeleologyEngine* engine, const char* path);

/* --- Pop-up events (define, queue, display, choose) ---
 *
 * Scripts can define events at runtime, queue them for display, and handle
 * the player's choice. The engine renders styled pop-up windows.
 *
 * Basic workflow:
 *   // In on_init: define events (or use templates)
 *   uint32_t evt = teleology_event_define(engine, "Rebellion!", "Peasants revolt in your lands.");
 *   teleology_event_add_choice(engine, evt, "Crush them", 0);
 *   teleology_event_add_choice(engine, evt, "Negotiate", 0);
 *
 *   // In on_daily_tick: queue when conditions met
 *   teleology_event_queue(engine, evt, 0, 0);  // global scope
 *
 *   // In on_daily_tick: check for player response
 *   uint32_t choices;
 *   uint32_t active = teleology_event_get_active(engine, &choices);
 *   if (active != 0) {
 *       // Event is showing — player hasn't chosen yet
 *       // (or call teleology_event_choose to auto-resolve from script)
 *   }
 *
 * Templates (ready-made events you can customize):
 *   uint32_t ids[5];
 *   teleology_event_register_templates(engine, ids);
 *   // ids: [0]=Notification, [1]=BinaryChoice, [2]=ThreeWay, [3]=Narrative, [4]=Diplomatic
 *   teleology_event_set_title(engine, ids[0], "Custom Title");
 *   teleology_event_set_body(engine, ids[0], "Custom body text.");
 *   teleology_event_queue(engine, ids[0], 0, 0);
 */

/* Event definition */
uint32_t     teleology_event_define(TeleologyEngine* engine, const char* title, const char* body);
/* Template: 0=Notification, 1=BinaryChoice, 2=ThreeWay, 3=Narrative, 4=Diplomatic */
uint32_t     teleology_event_from_template(TeleologyEngine* engine, uint32_t template);
int32_t      teleology_event_add_choice(TeleologyEngine* engine, uint32_t event_id, const char* text, uint32_t next_event_id);
uint8_t      teleology_event_set_choice_text(TeleologyEngine* engine, uint32_t event_id, uint32_t choice_idx, const char* text);
uint8_t      teleology_event_set_title(TeleologyEngine* engine, uint32_t event_id, const char* title);
uint8_t      teleology_event_set_body(TeleologyEngine* engine, uint32_t event_id, const char* body);
/* Set per-event image. path is relative to project resources dir. w/h=0 for natural size. */
uint8_t      teleology_event_set_image(TeleologyEngine* engine, uint32_t event_id, const char* path, float w, float h);

/* Event lifecycle */
/* scope_type: 0=Global, 1=Province, 2=Nation, 3=Character, 4=Army */
void         teleology_event_queue(TeleologyEngine* engine, uint32_t event_id, uint32_t scope_type, uint32_t scope_raw);
/* Returns active event_id (0 if none). Writes choice count to out. */
uint32_t     teleology_event_get_active(TeleologyEngine* engine, uint32_t* choice_count_out);
/* field: 0=title, 1=body. Writes NUL-terminated; returns full length. */
uint32_t     teleology_event_get_text(TeleologyEngine* engine, uint32_t field, char* out, uint32_t out_cap);
uint32_t     teleology_event_get_choice_text(TeleologyEngine* engine, uint32_t choice_idx, char* out, uint32_t out_cap);
/* Choose an option (0-based). Clears active event and chains to next. */
uint8_t      teleology_event_choose(TeleologyEngine* engine, uint32_t choice_idx);

/* Register all 5 built-in templates. Writes 5 event IDs to ids_out. */
void         teleology_event_register_templates(TeleologyEngine* engine, uint32_t* ids_out);

/* Pop-up styling (set before queueing; applies to next displayed event) */
void         teleology_event_style_reset(TeleologyEngine* engine);
/* anchor: 0=Center, 1=Fixed(x,y) */
void         teleology_event_style_set_anchor(TeleologyEngine* engine, uint32_t anchor, float x, float y);
void         teleology_event_style_set_colors(TeleologyEngine* engine,
                uint8_t bg_r, uint8_t bg_g, uint8_t bg_b, uint8_t bg_a,
                uint8_t title_r, uint8_t title_g, uint8_t title_b, uint8_t title_a,
                uint8_t body_r, uint8_t body_g, uint8_t body_b, uint8_t body_a);
void         teleology_event_style_set_image(TeleologyEngine* engine, const char* path, float w, float h);
/* width: 0=auto. modal: 1=pause game while showing. */
void         teleology_event_style_set_layout(TeleologyEngine* engine, float width, uint8_t modal);

/* --- Keyword tooltip system ---
 *
 * Register keywords that are automatically highlighted in event pop-up text.
 * When the player hovers a keyword, a tooltip panel appears with its
 * title, description, and optional icon.
 *
 * Example:
 *   uint32_t idx = teleology_keyword_add(engine, "Prestige", "Prestige",
 *       "A measure of your realm's renown. Affects diplomacy, vassal opinion, and succession.");
 *   teleology_keyword_set_color(engine, idx, 255, 215, 0, 255);  // gold
 *   teleology_keyword_set_icon(engine, idx, "icons/prestige.png");
 */

/* Add a keyword. Returns index (0xFFFFFFFF on failure). */
uint32_t     teleology_keyword_add(TeleologyEngine* engine, const char* keyword, const char* title, const char* description);
/* Set icon image for keyword tooltip. */
uint8_t      teleology_keyword_set_icon(TeleologyEngine* engine, uint32_t index, const char* path);
/* Set highlight color (RGBA) for keyword in text. 0,0,0,0 = default gold. */
uint8_t      teleology_keyword_set_color(TeleologyEngine* engine, uint32_t index, uint8_t r, uint8_t g, uint8_t b, uint8_t a);
/* Remove a keyword by index. */
uint8_t      teleology_keyword_remove(TeleologyEngine* engine, uint32_t index);
/* Remove all keywords. */
void         teleology_keyword_clear(TeleologyEngine* engine);
/* Get the number of registered keywords. */
uint32_t     teleology_keyword_count(TeleologyEngine* engine);

/* Load keywords from a JSON file (appends to existing).
 * Returns number of keywords loaded, or 0xFFFFFFFF on error.
 *
 * JSON format: array of objects with keyword, title, description,
 * and optional icon (string) and color ([r,g,b,a] array).
 *
 * Example keywords.json:
 *   [
 *     {
 *       "keyword": "Prestige",
 *       "title": "Prestige",
 *       "description": "A measure of your realm's renown.",
 *       "icon": "icons/prestige.png",
 *       "color": [255, 215, 0, 255]
 *     },
 *     {
 *       "keyword": "Casus Belli",
 *       "title": "Casus Belli",
 *       "description": "A justification for declaring war."
 *     }
 *   ]
 *
 * The engine also auto-loads "keywords.json" from the working directory
 * on startup if the file exists.
 */
uint32_t     teleology_keyword_load_file(TeleologyEngine* engine, const char* path);
/* Save current keywords to a JSON file (pretty-printed). */
uint8_t      teleology_keyword_save_file(TeleologyEngine* engine, const char* path);

/* --- Raycasting / coordinate conversion (screen <-> world <-> tile) ---
 *
 * The engine maintains a Viewport resource describing the current map view
 * (zoom, pan, canvas rect). The editor feeds this each frame. Scripts can
 * query it to convert screen coordinates to world/tile coordinates and
 * perform hit-testing against provinces.
 *
 * Example:
 *   float sx, sy;
 *   if (teleology_input_last_click(engine, &sx, &sy)) {
 *       CRaycastHit hit = teleology_raycast(engine, sx, sy);
 *       if (hit.province_raw != 0) {
 *           // clicked on province hit.province_raw at tile (hit.tile_x, hit.tile_y)
 *       }
 *   }
 */

typedef struct CRaycastHit {
    uint32_t province_raw;  /* 0 = miss / no province */
    int32_t  tile_x;        /* -1 if not applicable (irregular maps) */
    int32_t  tile_y;
    float    world_x;
    float    world_y;
} CRaycastHit;

/* Update viewport state (called by host/editor each frame; scripts usually don't need this). */
void         teleology_viewport_set(TeleologyEngine* engine, float base_cell, float zoom, float pan_x, float pan_y, float canvas_x, float canvas_y, float canvas_w, float canvas_h);

/* Raycast: screen coords -> province/tile/world. Returns CRaycastHit. */
CRaycastHit  teleology_raycast(TeleologyEngine* engine, float screen_x, float screen_y);

/* Coordinate conversion */
void         teleology_screen_to_world(TeleologyEngine* engine, float screen_x, float screen_y, float* x_out, float* y_out);
void         teleology_world_to_screen(TeleologyEngine* engine, float world_x, float world_y, float* x_out, float* y_out);

/* Screen -> tile. Returns 1 if valid tile, 0 if out of bounds. */
uint8_t      teleology_screen_to_tile(TeleologyEngine* engine, float screen_x, float screen_y, int32_t* tile_x_out, int32_t* tile_y_out);

/* Tile distance (Chebyshev for square grids, axial for hex; 0 for irregular). */
uint32_t     teleology_tile_distance(TeleologyEngine* engine, uint32_t x0, uint32_t y0, uint32_t x1, uint32_t y1);

#ifdef __cplusplus
}
#endif

#endif /* TELEOLOGY_SCRIPT_API_H */
