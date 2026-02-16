/**
 * Sample C++ script for Teleology. Build as a shared library and pass to the engine:
 *   cargo run --bin teleology -- target/debug/libscript.so
 *
 * Compile (Unix):
 *   c++ -std=c++17 -shared -fPIC -I../include main.cpp -o libscript.so
 * Compile (Windows):
 *   cl /LD /I..\include main.cpp /Fe:script.dll
 */
#include "../include/teleology.h"
#include <cstdint>
#include <cstdio>

static void on_init(TeleologyEngine* engine) {
    (void)engine;
    std::printf("[C++ script] on_init\n");
}

static void on_daily_tick(TeleologyEngine* engine) {
    (void)engine;
    // Example: read date from engine
    CGameDate d = teleology_get_date(engine);
    if (d.day == 1) std::printf("[C++ script] monthly tick %d-%02u-%02u\n", d.year, d.month, d.day);
}

static void on_monthly_tick(TeleologyEngine* engine) {
    (void)engine;
    uint32_t n = teleology_get_province_count(engine);
    std::printf("[C++ script] on_monthly_tick, provinces=%u\n", n);
    // Example: script could change ownership via teleology_set_province_owner(engine, pid, nid);
}

static void on_yearly_tick(TeleologyEngine* engine) {
    (void)engine;
    std::printf("[C++ script] on_yearly_tick\n");
}

static void on_event(TeleologyEngine* engine, uint32_t event_id, const uint8_t* payload, uint32_t payload_len) {
    (void)engine;
    (void)payload;
    (void)payload_len;
    std::printf("[C++ script] on_event id=%u len=%u\n", event_id, payload_len);
}

static const TeleologyScriptApi kApi = {
    .version = 1,
    .on_init = on_init,
    .on_daily_tick = on_daily_tick,
    .on_monthly_tick = on_monthly_tick,
    .on_yearly_tick = on_yearly_tick,
    .on_event = on_event,
};

extern "C" const TeleologyScriptApi* teleology_script_get_api(void) {
    return &kApi;
}
