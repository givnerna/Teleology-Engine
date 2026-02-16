//! Built-in character generator (dev customizable).
//!
//! This is intentionally generic so different genres/time periods can plug in their own
//! name pools, stat ranges, trait weighting, etc.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::characters::{Character, CharacterStats};
use crate::world::NationId;

/// Context passed to generators.
#[derive(Clone, Copy, Default)]
pub struct GenContext {
    pub nation: Option<NationId>,
    /// Optional role hint for generators (e.g. leader/general/advisor/custom).
    pub role_hint: u32,
    /// Current year (optional; depends on calendar).
    pub year: i32,
    /// Deterministic seed for generation.
    pub seed: u64,
}

/// Generator trait: games can implement their own.
pub trait CharacterGenerator: Send + Sync {
    fn generate(&self, ctx: GenContext) -> (Character, CharacterStats);
}

/// Config resource for the default generator.
#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct CharacterGenConfig {
    /// Name pool (string table ids) to draw from.
    pub name_pool: Vec<u32>,
    /// Inclusive stat ranges.
    pub military_min: i16,
    pub military_max: i16,
    pub diplomacy_min: i16,
    pub diplomacy_max: i16,
    pub administration_min: i16,
    pub administration_max: i16,
}

impl Default for CharacterGenConfig {
    fn default() -> Self {
        Self {
            name_pool: vec![0],
            military_min: 0,
            military_max: 6,
            diplomacy_min: 0,
            diplomacy_max: 6,
            administration_min: 0,
            administration_max: 6,
        }
    }
}

/// Simple deterministic PRNG (xorshift64* style).
#[derive(Clone, Copy)]
struct XorShift64(u64);

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    fn gen_range_i16(&mut self, min: i16, max: i16) -> i16 {
        if min >= max {
            return min;
        }
        let span = (max as i32 - min as i32 + 1).max(1) as u64;
        let r = (self.next_u64() % span) as i32;
        (min as i32 + r) as i16
    }

    fn choose_u32(&mut self, v: &[u32]) -> u32 {
        if v.is_empty() {
            return 0;
        }
        let i = (self.next_u64() % (v.len() as u64)) as usize;
        v[i]
    }
}

/// Built-in default generator. Games customize by setting `CharacterGenConfig`.
pub struct DefaultCharacterGenerator {
    pub config: CharacterGenConfig,
}

impl DefaultCharacterGenerator {
    pub fn from_config(config: CharacterGenConfig) -> Self {
        Self { config }
    }
}

impl CharacterGenerator for DefaultCharacterGenerator {
    fn generate(&self, ctx: GenContext) -> (Character, CharacterStats) {
        let mut rng = XorShift64::new(ctx.seed ^ (ctx.role_hint as u64));
        let name_id = rng.choose_u32(&self.config.name_pool);
        let stats = CharacterStats {
            military: rng.gen_range_i16(self.config.military_min, self.config.military_max),
            diplomacy: rng.gen_range_i16(self.config.diplomacy_min, self.config.diplomacy_max),
            administration: rng.gen_range_i16(
                self.config.administration_min,
                self.config.administration_max,
            ),
            custom: Default::default(),
        };
        let c = Character {
            name_id,
            persistent_id: ctx.seed,
            birth_year: Some(ctx.year.saturating_sub(30)),
            death_year: None,
        };
        (c, stats)
    }
}

