//! Minimal runner: create world, optionally load script, run a few ticks.

use std::path::Path;
use teleology_runtime::EngineContext;

fn main() {
    let mut engine = EngineContext::new();

    // Load script if path given (e.g. target/debug/libscript.so)
    if let Some(path) = std::env::args().nth(1) {
        if let Err(e) = engine.load_script(Path::new(&path)) {
            eprintln!("Script load failed: {}", e);
        }
    }

    for _ in 0..30 {
        engine.tick();
    }

    let date = engine
        .world()
        .get_resource::<teleology_core::GameDate>()
        .copied()
        .unwrap_or_default();
    println!("Date after 30 ticks: {}-{:02}-{:02}", date.year, date.month, date.day);
}
