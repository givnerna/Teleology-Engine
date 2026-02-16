//! Modifier system (dev customizable).
//!
//! Modifiers are stackable, can expire, and can be attached to multiple scopes.
//! Designed to be modular: games that don't enable modifiers simply omit the resources/components.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

use crate::world::{GameDate, NationId, ProvinceId};

/// Stable id for a modifier instance.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModifierId(pub NonZeroU32);

/// Stable id for a modifier category/type (e.g. tax_income, stability, custom).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModifierTypeId(pub NonZeroU32);

/// How a modifier changes a base value.
#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum ModifierValue {
    /// Add directly to the base value.
    Additive(f64),
    /// Multiply base by (1 + x). Example: Multiplicative(0.1) = +10%.
    Multiplicative(f64),
    /// Override the value.
    Set(f64),
    /// Custom operation id + value (resolved by a calculator at runtime).
    Custom { op_id: u32, value: f64 },
}

/// A single modifier instance.
#[derive(Clone, Serialize, Deserialize)]
pub struct Modifier {
    pub id: ModifierId,
    pub ty: ModifierTypeId,
    pub value: ModifierValue,
    /// Game-defined source id (event/building/trait/etc.) for tracking/removal.
    pub source_id: u32,
    /// Optional expiration date.
    pub expires_on: Option<GameDate>,
}

/// Dev-provided calculator for custom modifier ops.
pub trait ModifierCalculator: Send + Sync {
    fn apply_custom(&self, op_id: u32, base: f64, value: f64) -> f64;
}

/// Modifiers attached to provinces (indexed by ProvinceId).
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct ProvinceModifiers {
    /// per_province[i] = modifiers for ProvinceId(i+1)
    pub per_province: Vec<Vec<Modifier>>,
    pub next_id_raw: u32,
}

impl ProvinceModifiers {
    pub fn new(province_count: usize) -> Self {
        Self {
            per_province: vec![Vec::new(); province_count],
            next_id_raw: 1,
        }
    }

    fn alloc_id(&mut self) -> ModifierId {
        let raw = self.next_id_raw.max(1);
        self.next_id_raw = raw.saturating_add(1);
        ModifierId(NonZeroU32::new(raw).unwrap())
    }

    pub fn list(&self, province: ProvinceId) -> &[Modifier] {
        self.per_province
            .get(province.index())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn add(&mut self, province: ProvinceId, mut m: Modifier) -> ModifierId {
        let id = self.alloc_id();
        m.id = id;
        if let Some(v) = self.per_province.get_mut(province.index()) {
            v.push(m);
        }
        id
    }

    pub fn remove(&mut self, province: ProvinceId, id: ModifierId) -> bool {
        let Some(v) = self.per_province.get_mut(province.index()) else { return false };
        let before = v.len();
        v.retain(|m| m.id != id);
        before != v.len()
    }
}

/// Modifiers attached to nations (indexed by NationId).
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct NationModifiers {
    pub per_nation: Vec<Vec<Modifier>>,
    pub next_id_raw: u32,
}

impl NationModifiers {
    pub fn new(nation_count: usize) -> Self {
        Self {
            per_nation: vec![Vec::new(); nation_count],
            next_id_raw: 1,
        }
    }

    fn alloc_id(&mut self) -> ModifierId {
        let raw = self.next_id_raw.max(1);
        self.next_id_raw = raw.saturating_add(1);
        ModifierId(NonZeroU32::new(raw).unwrap())
    }

    pub fn list(&self, nation: NationId) -> &[Modifier] {
        self.per_nation
            .get(nation.index())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn add(&mut self, nation: NationId, mut m: Modifier) -> ModifierId {
        let id = self.alloc_id();
        m.id = id;
        if let Some(v) = self.per_nation.get_mut(nation.index()) {
            v.push(m);
        }
        id
    }

    pub fn remove(&mut self, nation: NationId, id: ModifierId) -> bool {
        let Some(v) = self.per_nation.get_mut(nation.index()) else { return false };
        let before = v.len();
        v.retain(|m| m.id != id);
        before != v.len()
    }
}

/// Modifiers attached to a character entity.
#[derive(Component, Clone, Default, Serialize, Deserialize)]
pub struct CharacterModifiers {
    pub mods: Vec<Modifier>,
}

/// Modifiers attached to an army entity.
#[derive(Component, Clone, Default, Serialize, Deserialize)]
pub struct ArmyModifiers {
    pub mods: Vec<Modifier>,
}

/// Apply a list of modifiers to a base value.
pub fn apply_modifiers(
    mut base: f64,
    mods: &[Modifier],
    calculator: Option<&dyn ModifierCalculator>,
    now: Option<GameDate>,
) -> f64 {
    // Filter expired if date provided.
    let active = mods.iter().filter(|m| {
        if let (Some(exp), Some(now)) = (m.expires_on, now) {
            now.to_days_since_epoch() <= exp.to_days_since_epoch()
        } else {
            true
        }
    });

    let mut add = 0.0;
    let mut mult = 1.0;
    let mut set: Option<f64> = None;

    for m in active {
        match m.value {
            ModifierValue::Additive(x) => add += x,
            ModifierValue::Multiplicative(x) => mult *= 1.0 + x,
            ModifierValue::Set(x) => set = Some(x),
            ModifierValue::Custom { op_id, value } => {
                if let Some(calc) = calculator {
                    base = calc.apply_custom(op_id, base, value);
                }
            }
        }
    }

    base += add;
    base *= mult;
    if let Some(x) = set {
        base = x;
    }
    base
}

