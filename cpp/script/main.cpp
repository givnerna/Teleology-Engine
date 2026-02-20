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

static CNationId me  = { 1 };
static CNationId rival = { 2 };

static void on_init(TeleologyEngine* engine) {
    std::printf("[script] on_init — provinces=%u, nations=%u\n",
        teleology_get_province_count(engine),
        teleology_get_nation_count(engine));

    // Register unit types for the combat system.
    teleology_combat_set_model(engine, 0); // StackBased (Paradox-style)
    teleology_combat_register_unit_type(engine, "Infantry", 0, 10, 100, 1);
    teleology_combat_register_unit_type(engine, "Cavalry",  1, 15, 80,  3);

    // Register a trade good.
    teleology_economy_register_good(engine, "Wheat", 2.0);

    // Spawn a ruler character.
    uint64_t ruler = teleology_character_spawn(engine, 1, 1420);
    teleology_character_set_role(engine, ruler, 0, me, 0); // Leader
    teleology_character_set_stat(engine, ruler, 0, 6);     // military=6
    teleology_character_set_stat(engine, ruler, 1, 4);     // diplomacy=4
    teleology_character_set_stat(engine, ruler, 2, 5);     // admin=5
}

static void on_daily_tick(TeleologyEngine* engine) {
    CGameDate d = teleology_get_date(engine);
    if (d.day == 1)
        std::printf("[script] %d-%02u-%02u\n", d.year, d.month, d.day);
}

static void on_monthly_tick(TeleologyEngine* engine) {
    // Check diplomacy — if rival dislikes us, declare war.
    int16_t opinion = teleology_diplomacy_get_opinion(engine, me, rival);
    if (opinion < -50 && !teleology_diplomacy_are_at_war(engine, me, rival)
                      && !teleology_diplomacy_has_truce(engine, me, rival)) {
        CProvinceId target = { 5 };
        uint32_t war = teleology_diplomacy_declare_war(engine, me, rival, 0, target.raw);
        std::printf("[script] Declared war on rival! war_id=%u\n", war);
    }

    // Show economy info.
    int64_t gold = teleology_get_nation_treasury(engine, me);
    double income = teleology_economy_get_total_income(engine, me);
    double expenses = teleology_economy_get_total_expenses(engine, me);
    std::printf("[script] Treasury: %lld  Income: %.1f  Expenses: %.1f\n",
        (long long)gold, income, expenses);

    // If rich, boost stability.
    if (gold > 500) {
        int8_t stab = teleology_get_nation_stability(engine, me);
        if (stab < 3) teleology_set_nation_stability(engine, me, stab + 1);
    }

    // Check population unrest in capital.
    CProvinceId capital = { 1 };
    float unrest = teleology_pop_average_unrest(engine, capital);
    if (unrest > 50.0f) {
        // Add a stability modifier to reduce unrest.
        teleology_modifier_add_province(engine, capital, 1, 0, -5.0, 100);
        std::printf("[script] High unrest in capital (%.1f%%), added calming modifier\n", unrest);
    }

    // Check for revolts.
    uint32_t revolt_provs[16], revolt_str[16];
    uint32_t count = teleology_pop_check_revolts(engine, revolt_provs, revolt_str, 16);
    if (count > 0) {
        std::printf("[script] %u provinces revolting!\n", count);
    }

    // Draw HUD.
    char buf[128];
    std::snprintf(buf, sizeof(buf), "Gold: %lld | Stability: %d | War Exhaustion: %.0f%%",
        (long long)gold, teleology_get_nation_stability(engine, me),
        teleology_get_nation_war_exhaustion(engine, me));
    teleology_ui_begin_window(engine, "Status", 10, 10, 500, 40);
      teleology_ui_label(engine, buf);
    teleology_ui_end_window(engine);
}

static void on_yearly_tick(TeleologyEngine* engine) {
    std::printf("[script] on_yearly_tick — prestige=%d, manpower=%u\n",
        teleology_get_nation_prestige(engine, me),
        teleology_get_nation_manpower(engine, me));
}

static void on_event(TeleologyEngine* engine, uint32_t event_id, const uint8_t* payload, uint32_t payload_len) {
    (void)engine; (void)payload; (void)payload_len;
    std::printf("[script] on_event id=%u\n", event_id);
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
