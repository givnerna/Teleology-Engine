# Expose Core Simulation Systems to C++ Scripts — Implementation Plan

## Problem

The Teleology Engine has fully implemented Rust subsystems for **diplomacy, economy, population, combat, characters, and modifiers**, but none of them are accessible from C++ scripts. Script developers can show UI and handle events, but can't actually interact with the game simulation — they can't declare wars, check treasury, modify population, add modifiers, or spawn characters.

## Approach

Follow the existing C API pattern: `#[unsafe(no_mangle)] pub extern "C" fn teleology_*` functions in `teleology-runtime/src/lib.rs`, with matching declarations in `cpp/include/teleology.h`. Each subsystem gets a lazy `ensure_*` initializer and a focused set of accessor/mutator functions.

All changes touch **3 files**: `lib.rs` (runtime), `teleology.h` (C header), `teleology_ffi.h` (FFI types if new C structs needed).

---

## Phase 1: Province & Nation Extended Fields

Exposes the fields on `Province` and `Nation` that scripts currently can't read or write.

### Province API
| Function | Signature | Purpose |
|---|---|---|
| `teleology_get_province_terrain` | `(engine, province) -> u8` | Get terrain type (0=land, 1=sea) |
| `teleology_set_province_terrain` | `(engine, province, terrain)` | Set terrain type |
| `teleology_get_province_development` | `(engine, province, index) -> u16` | Get dev level (0=tax, 1=prod, 2=manpower) |
| `teleology_set_province_development` | `(engine, province, index, value)` | Set dev level |
| `teleology_get_province_population` | `(engine, province) -> u32` | Get raw population count |
| `teleology_set_province_population` | `(engine, province, value)` | Set raw population count |
| `teleology_get_province_occupation` | `(engine, province) -> CNationId` | Get occupier (0 = none) |
| `teleology_set_province_occupation` | `(engine, province, nation)` | Set occupier |

### Nation API
| Function | Signature | Purpose |
|---|---|---|
| `teleology_get_nation_treasury` | `(engine, nation) -> i64` | Get gold |
| `teleology_set_nation_treasury` | `(engine, nation, value)` | Set gold |
| `teleology_get_nation_stability` | `(engine, nation) -> i8` | Get stability (-3 to +3) |
| `teleology_set_nation_stability` | `(engine, nation, value)` | Set stability |
| `teleology_get_nation_prestige` | `(engine, nation) -> i32` | Get prestige |
| `teleology_set_nation_prestige` | `(engine, nation, value)` | Set prestige |
| `teleology_get_nation_manpower` | `(engine, nation) -> u32` | Get manpower pool |
| `teleology_set_nation_manpower` | `(engine, nation, value)` | Set manpower pool |
| `teleology_get_nation_war_exhaustion` | `(engine, nation) -> f32` | Get war exhaustion (0–100) |
| `teleology_set_nation_war_exhaustion` | `(engine, nation, value)` | Set war exhaustion |

---

## Phase 2: Diplomacy

### Relations API
| Function | Signature | Purpose |
|---|---|---|
| `teleology_diplomacy_get_opinion` | `(engine, a, b) -> i16` | Get opinion of a toward b |
| `teleology_diplomacy_get_trust` | `(engine, a, b) -> i16` | Get trust between a and b |
| `teleology_diplomacy_modify_opinion` | `(engine, a, b, delta)` | Modify opinion (symmetric) |
| `teleology_diplomacy_modify_trust` | `(engine, a, b, delta)` | Modify trust (symmetric) |

### War API
| Function | Signature | Purpose |
|---|---|---|
| `teleology_diplomacy_declare_war` | `(engine, attacker, defender, goal_type, target_province) -> u32` | Declare war, returns WarId raw |
| `teleology_diplomacy_end_war` | `(engine, war_id, truce_days)` | End war, create truce |
| `teleology_diplomacy_are_at_war` | `(engine, a, b) -> u8` | Check if two nations are at war |
| `teleology_diplomacy_get_war_score` | `(engine, war_id) -> i16` | Get war score (-100 to +100) |
| `teleology_diplomacy_set_war_score` | `(engine, war_id, score)` | Set war score |

### Alliance & Truce API
| Function | Signature | Purpose |
|---|---|---|
| `teleology_diplomacy_form_alliance` | `(engine, a, b)` | Form alliance |
| `teleology_diplomacy_break_alliance` | `(engine, a, b)` | Break alliance |
| `teleology_diplomacy_are_allied` | `(engine, a, b) -> u8` | Check alliance |
| `teleology_diplomacy_has_truce` | `(engine, a, b) -> u8` | Check truce |

**Lazy init:** `ensure_diplomacy(world)` inserts `DiplomaticRelations`, `WarRegistry`, and `DiplomacyConfig` with defaults.

---

## Phase 3: Economy

### Budget Query API
| Function | Signature | Purpose |
|---|---|---|
| `teleology_economy_get_tax_income` | `(engine, nation) -> f64` | Nation's tax income |
| `teleology_economy_get_production_income` | `(engine, nation) -> f64` | Production income |
| `teleology_economy_get_trade_income` | `(engine, nation) -> f64` | Trade income |
| `teleology_economy_get_total_income` | `(engine, nation) -> f64` | Total income |
| `teleology_economy_get_total_expenses` | `(engine, nation) -> f64` | Total expenses |
| `teleology_economy_get_balance` | `(engine, nation) -> f64` | Net balance |

### Goods & Trade API
| Function | Signature | Purpose |
|---|---|---|
| `teleology_economy_register_good` | `(engine, name, base_price) -> u32` | Register a trade good type |
| `teleology_economy_get_good_price` | `(engine, good_id) -> f64` | Get base price |
| `teleology_economy_get_province_good` | `(engine, province) -> u32` | Get province's produced good (0=none) |
| `teleology_economy_set_province_good` | `(engine, province, good_id)` | Set province's produced good |
| `teleology_economy_get_province_trade_power` | `(engine, province) -> f64` | Get local trade power |
| `teleology_economy_set_province_trade_power` | `(engine, province, value)` | Set local trade power |

**Lazy init:** `ensure_economy(world)` inserts `EconomyConfig`, `NationBudgets`, `GoodsRegistry`, `ProvinceEconomy`, `TradeNetwork` with defaults.

---

## Phase 4: Population

| Function | Signature | Purpose |
|---|---|---|
| `teleology_pop_total` | `(engine, province) -> u32` | Total population in province |
| `teleology_pop_average_unrest` | `(engine, province) -> f32` | Weighted average unrest |
| `teleology_pop_group_count` | `(engine, province) -> u32` | Number of pop groups |
| `teleology_pop_group_size` | `(engine, province, index) -> u32` | Size of pop group at index |
| `teleology_pop_group_unrest` | `(engine, province, index) -> f32` | Unrest of pop group |
| `teleology_pop_group_culture` | `(engine, province, index) -> u32` | Culture TagId of pop group |
| `teleology_pop_group_religion` | `(engine, province, index) -> u32` | Religion TagId of pop group |
| `teleology_pop_add_group` | `(engine, province, culture, religion, size)` | Add a new pop group |
| `teleology_pop_check_revolts` | `(engine, out_provinces, out_strengths, cap) -> u32` | Check for revolts, returns count |

**Lazy init:** `ensure_population(world)` inserts `PopulationConfig`, `ProvincePops` with defaults.

---

## Phase 5: Modifiers

| Function | Signature | Purpose |
|---|---|---|
| `teleology_modifier_add_province` | `(engine, province, type_id, op, value, source_id) -> u32` | Add modifier to province |
| `teleology_modifier_add_nation` | `(engine, nation, type_id, op, value, source_id) -> u32` | Add modifier to nation |
| `teleology_modifier_remove_province` | `(engine, province, modifier_id) -> u8` | Remove province modifier |
| `teleology_modifier_remove_nation` | `(engine, nation, modifier_id) -> u8` | Remove nation modifier |
| `teleology_modifier_list_province` | `(engine, province) -> u32` | Count of province modifiers |
| `teleology_modifier_list_nation` | `(engine, nation) -> u32` | Count of nation modifiers |
| `teleology_modifier_apply` | `(engine, base, type_id, scope_kind, scope_id) -> f64` | Apply all matching modifiers to base value |

`op` parameter: 0=Additive, 1=Multiplicative, 2=Set, 3=Custom.

**Lazy init:** `ensure_modifiers(world)` inserts `ProvinceModifiers`, `NationModifiers` with defaults.

---

## Phase 6: Characters

| Function | Signature | Purpose |
|---|---|---|
| `teleology_character_spawn` | `(engine, name_id, birth_year) -> u64` | Spawn character, returns persistent_id |
| `teleology_character_set_role` | `(engine, persistent_id, role, nation, army)` | Assign role (0=Leader, 1=General, 2=Advisor, 3=Custom) |
| `teleology_character_get_stat` | `(engine, persistent_id, stat) -> i16` | Get stat (0=military, 1=diplomacy, 2=admin) |
| `teleology_character_set_stat` | `(engine, persistent_id, stat, value)` | Set stat |
| `teleology_character_get_custom_stat` | `(engine, persistent_id, stat_id) -> i32` | Get custom stat |
| `teleology_character_set_custom_stat` | `(engine, persistent_id, stat_id, value)` | Set custom stat |
| `teleology_character_kill` | `(engine, persistent_id, death_year)` | Mark character as dead |

---

## Phase 7: Combat (Read-Only + Config)

Combat resolution is complex and should stay in Rust. Scripts configure and inspect, but don't drive tick-by-tick battles.

| Function | Signature | Purpose |
|---|---|---|
| `teleology_combat_set_model` | `(engine, model) -> void` | Set active combat model (0=Stack, 1=Tile, 2=Deployment, 3=Tactical) |
| `teleology_combat_get_model` | `(engine) -> u8` | Get active combat model |
| `teleology_combat_register_unit_type` | `(engine, name, category, strength, morale, speed) -> u32` | Register unit type |
| `teleology_combat_result_count` | `(engine) -> u32` | Number of logged battles |
| `teleology_combat_result_get` | `(engine, index, out_attacker_casualties, out_defender_casualties, out_winner) -> u32` | Get battle result details |

---

## Implementation Order

| Step | Phase | Est. Functions | Key Risk |
|------|-------|---------------|----------|
| 1 | Province/Nation fields | 18 | None — straightforward field access |
| 2 | Diplomacy | 14 | War declaration needs GameDate from world |
| 3 | Economy | 12 | Budget values may need simulation tick to populate |
| 4 | Population | 9 | Revolt check returns variable-length data |
| 5 | Modifiers | 7 | ModifierValue enum mapping to C int |
| 6 | Characters | 7 | Entity lookup by persistent_id needs index |
| 7 | Combat | 5 | Read-only inspection keeps it simple |

**Total: ~72 new C API functions across all phases.**

---

## Files Changed

| File | Changes |
|------|---------|
| `crates/teleology-runtime/src/lib.rs` | All 72 `extern "C"` functions + `ensure_*` helpers |
| `cpp/include/teleology.h` | All 72 C declarations |
| `cpp/include/teleology_ffi.h` | New FFI types if needed (e.g., `CWarId`) |
| `cpp/CPP_API_GUIDE.txt` | New sections documenting each subsystem |
| `cpp/script/main.cpp` | Example usage of new APIs |

---

## Example: What Script Code Looks Like After

```cpp
void on_monthly_tick(TeleologyEngine* engine) {
    CNationId me = { 1 };
    CNationId rival = { 2 };

    // Check diplomacy
    int16_t opinion = teleology_diplomacy_get_opinion(engine, me, rival);
    if (opinion < -50 && !teleology_diplomacy_are_at_war(engine, me, rival)) {
        teleology_diplomacy_declare_war(engine, me, rival, 0, 5); // Conquest, province 5
    }

    // Check treasury
    int64_t gold = teleology_get_nation_treasury(engine, me);
    if (gold > 500) {
        teleology_set_nation_stability(engine, me,
            teleology_get_nation_stability(engine, me) + 1);
    }

    // Show population info in HUD
    CProvinceId capital = { 1 };
    uint32_t pop = teleology_pop_total(engine, capital);
    float unrest = teleology_pop_average_unrest(engine, capital);
    char buf[128];
    snprintf(buf, sizeof(buf), "Capital Pop: %u  Unrest: %.1f%%", pop, unrest);
    teleology_ui_begin_window(engine, "Pop Info", 10, 50, 300, 40);
      teleology_ui_label(engine, buf);
    teleology_ui_end_window(engine);
}
```
