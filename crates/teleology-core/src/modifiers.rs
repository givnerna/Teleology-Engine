//! Modifier system (dev customizable).
//!
//! Modifiers are stackable, can expire, and can be attached to multiple scopes.
//! Designed to be modular: games that don't enable modifiers simply omit the resources/components.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use std::num::NonZeroU32;

use crate::world::{GameDate, NationId, ProvinceId, ScopeId};

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

/// Generic modifiers indexed by any ScopeId (ProvinceId, NationId, etc.).
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ScopedModifiers<Id: ScopeId> {
    pub per_scope: Vec<Vec<Modifier>>,
    pub next_id_raw: u32,
    #[serde(skip)]
    _marker: PhantomData<Id>,
}

impl<Id: ScopeId> ScopedModifiers<Id> {
    pub fn new(count: usize) -> Self {
        Self {
            per_scope: vec![Vec::new(); count],
            next_id_raw: 1,
            _marker: PhantomData,
        }
    }

    fn alloc_id(&mut self) -> ModifierId {
        let raw = self.next_id_raw.max(1);
        self.next_id_raw = raw.saturating_add(1);
        ModifierId(NonZeroU32::new(raw).unwrap())
    }

    pub fn list(&self, id: Id) -> &[Modifier] {
        self.per_scope
            .get(id.index())
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn add(&mut self, id: Id, mut m: Modifier) -> ModifierId {
        let mid = self.alloc_id();
        m.id = mid;
        if let Some(v) = self.per_scope.get_mut(id.index()) {
            v.push(m);
        }
        mid
    }

    pub fn remove(&mut self, id: Id, mid: ModifierId) -> bool {
        let Some(v) = self.per_scope.get_mut(id.index()) else { return false };
        let before = v.len();
        v.retain(|m| m.id != mid);
        before != v.len()
    }
}

/// Modifiers attached to provinces. Type alias for backwards compatibility.
pub type ProvinceModifiers = ScopedModifiers<ProvinceId>;

/// Modifiers attached to nations. Type alias for backwards compatibility.
pub type NationModifiers = ScopedModifiers<NationId>;

// Resource impls — Bevy requires Resource on concrete types, not generic.
// We implement Resource for the two concrete aliases via wrapper newtype or direct impl.
// Since type aliases can't have trait impls, we use the blanket approach:
// ScopedModifiers<ProvinceId> and ScopedModifiers<NationId> both need Resource.
impl Resource for ScopedModifiers<ProvinceId> {}
impl Resource for ScopedModifiers<NationId> {}

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
