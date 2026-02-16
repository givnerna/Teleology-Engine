# Teleology

A **data-oriented game engine** in **Rust** with **C++ scripting**, designed for **grand strategy games** (large entity counts, day/month/year simulation, moddable logic). Games run on **Windows**, **macOS**, **Linux**, and **WebGL** (browser).

## Design

- **Data-oriented**: Dense province/nation storage (SoA-friendly), Bevy ECS for variable entities (units, events). Batch processing and parallel schedules.
- **Grand strategy focus**: Day/month/year tick rates; systems run only when due (e.g. monthly on 1st). Optimized for many provinces and nations.
- **C++ scripting**: Engine exposes a stable C API; game logic and events are implemented in C++ in a shared library loaded at runtime.

## Layout

```
teleology-core     → ECS world, Province/Nation/Unit, MapLayout, simulation tick, schedules
teleology-script-api → C types, script vtable, script DLL loading (native); stubbed on wasm
teleology-runtime  → Engine context, C API, hot reload (file watcher)
teleology-editor   → Map editor UI (egui): provinces, nations, 2D map paint, script load + hot reload
cpp/               → C++ script sample and teleology.h
```

## Platforms

| Platform    | Run games / editor                                                    | C++ scripting        |
| ----------- | --------------------------------------------------------------------- | -------------------- |
| **Windows** | `cargo run -p teleology-runtime --bin teleology` / `teleology-editor` | ✅ `.dll`            |
| **macOS**   | Same                                                                  | ✅ `.dylib`          |
| **Linux**   | Same                                                                  | ✅ `.so`             |
| **WebGL**   | Build with Trunk, open in browser                                     | ❌ (simulation only) |

Script library names: `teleology_script_api::script_library_filename("game")` → `game.dll` (Windows), `libgame.dylib` (macOS), `libgame.so` (Linux).

---

## Build and run

### Windows, macOS, Linux (native)

```bash
cargo build
cargo run -p teleology-runtime --bin teleology
```

With a C++ script:

```bash
# Linux / macOS
cd cpp/script && c++ -std=c++17 -shared -fPIC -I../include main.cpp -o libscript.so && cd ../..
cargo run -p teleology-runtime --bin teleology -- ./cpp/script/libscript.so

# Windows: build script to script.dll, then pass script.dll
# macOS: use -dynamiclib, output libscript.dylib
```

### WebGL (browser)

Map editor and simulation run in the browser; C++ scripting is not available. Build with [Trunk](https://trunkrs.dev/):

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
trunk build
# Output in dist/. Serve dist/ (e.g. trunk serve) and open in browser.
trunk serve   # then open http://127.0.0.1:8080
```

### Testing

Run unit and integration tests:

```bash
cargo test
```

Tests cover: province/nation IDs, `GameDate` and `MapLayout`, world builder resources, simulation tick (date advance, month/year rollover), and runtime engine context (tick advances date, default provinces/nations/map).

### Maps (Paradox-style)

Maps support **land/sea terrain** (`Province.terrain`: `TERRAIN_LAND`, `TERRAIN_SEA`) and **province adjacency** (borders for movement and pathfinding). The editor can:

- **Load map** — Open a `.tmap` file (binary format: layout, adjacency, provinces, nations, date). Use this to upload maps created elsewhere or previously saved.
- **Save map** — Write the current map and world state to a `.tmap` file.
- **Recompute adjacency** — After painting the grid, recompute borders from the layout (4-connected).

Map files are bincode-serialized; they include the full province/nation data and date so you can share or version maps.

### Visual editor

The visual editor has **modes**; one of them is the map editor.

```bash
cargo run -p teleology-editor --bin teleology-editor
```

- **Modes** (top bar): **Map Editor** — paint provinces, load/save maps, assign owners. **World** — overview (date, province/nation counts). **Settings** — placeholder.
- **Map Editor**: Left panel = province list; right panel = nation list; center = 2D map grid. Select a province and click/drag on the map to paint. **Assign selected province to selected nation** to set ownership. Load map / Save map / Recompute adjacency (native only).
- **Script** (native): Path + **Load script**, **Hot reload**. Run / Pause / **Tick** to advance simulation.

## C++ script API

- Implement `teleology_script_get_api()` returning a `TeleologyScriptApi` with callbacks: `on_init`, `on_daily_tick`, `on_monthly_tick`, `on_yearly_tick`, `on_event`.
- From C++ you call engine API: `teleology_get_date(engine)`, `teleology_get_province_count`, `teleology_get_province_owner`, `teleology_set_province_owner`. See `cpp/include/teleology.h`.

## Grand strategy optimizations

- **Tick granularity**: Daily systems run every day; monthly/yearly only on month/year boundaries to reduce work.
- **SoA-style storage**: Provinces and nations in contiguous arrays; easy to add true SoA (e.g. `owner: Vec<NationId>`) for maximum cache use.
- **Parallel schedules**: Bevy’s multi-threaded executor runs independent systems in parallel.
- **Batch helpers**: `par_provinces_mut` in `teleology-core` for parallel province updates.

## License

MIT or Apache-2.0, at your option.
