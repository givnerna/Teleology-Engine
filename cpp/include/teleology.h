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

#ifdef __cplusplus
}
#endif

#endif /* TELEOLOGY_SCRIPT_API_H */
