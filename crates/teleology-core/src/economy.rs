//! Economy system: taxes, production, trade, budgets, and maintenance.
//!
//! Data-driven via `EconomyConfig`. All formulas are configurable so game
//! makers can tune the economy without writing code.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

use crate::armies::Army;
use crate::world::{NationId, NationStore, ProvinceStore, WorldBounds};

// ---------------------------------------------------------------------------
// Goods
// ---------------------------------------------------------------------------

/// Stable id for a trade good type.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GoodTypeId(pub NonZeroU32);

impl GoodTypeId {
    #[inline]
    pub fn raw(self) -> u32 { self.0.get() }
}

/// Definition of a trade good.
#[derive(Clone, Serialize, Deserialize)]
pub struct GoodTypeDef {
    pub id: GoodTypeId,
    pub name: String,
    pub base_price: f64,
}

/// Registry of all trade good types.
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct GoodsRegistry {
    pub goods: Vec<GoodTypeDef>,
    next_raw: u32,
}

impl GoodsRegistry {
    pub fn new() -> Self {
        Self { goods: Vec::new(), next_raw: 1 }
    }

    pub fn register(&mut self, name: String, base_price: f64) -> GoodTypeId {
        let id = GoodTypeId(NonZeroU32::new(self.next_raw).unwrap());
        self.next_raw += 1;
        self.goods.push(GoodTypeDef { id, name, base_price });
        id
    }

    pub fn get(&self, id: GoodTypeId) -> Option<&GoodTypeDef> {
        self.goods.iter().find(|g| g.id == id)
    }

    pub fn base_price(&self, id: GoodTypeId) -> f64 {
        self.get(id).map(|g| g.base_price).unwrap_or(1.0)
    }
}

// ---------------------------------------------------------------------------
// Per-province economy data
// ---------------------------------------------------------------------------

/// Per-province economy data (produced good, trade power).
#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct ProvinceEconomy {
    /// Index = province index (0-based). What good the province produces.
    pub produced_good: Vec<Option<GoodTypeId>>,
    /// Local trade power per province.
    pub local_trade_power: Vec<f64>,
}

impl ProvinceEconomy {
    pub fn new(province_count: usize) -> Self {
        Self {
            produced_good: vec![None; province_count],
            local_trade_power: vec![1.0; province_count],
        }
    }
}

// ---------------------------------------------------------------------------
// Trade nodes (optional layer)
// ---------------------------------------------------------------------------

/// Stable id for a trade node.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TradeNodeId(pub NonZeroU32);

/// A trade node: a collection of provinces that pool trade value.
#[derive(Clone, Serialize, Deserialize)]
pub struct TradeNode {
    pub id: TradeNodeId,
    pub name: String,
    /// Province raw ids in this node.
    pub provinces: Vec<u32>,
    /// Downstream nodes (trade flows toward these).
    pub downstream: Vec<TradeNodeId>,
}

/// Trade network: collection of trade nodes. Optional — if absent, trade income
/// is computed as a flat bonus per production development.
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct TradeNetwork {
    pub nodes: Vec<TradeNode>,
    next_raw: u32,
}

impl TradeNetwork {
    pub fn new() -> Self {
        Self { nodes: Vec::new(), next_raw: 1 }
    }

    pub fn add_node(&mut self, name: String, provinces: Vec<u32>) -> TradeNodeId {
        let id = TradeNodeId(NonZeroU32::new(self.next_raw).unwrap());
        self.next_raw += 1;
        self.nodes.push(TradeNode { id, name, provinces, downstream: Vec::new() });
        id
    }

    pub fn get_node(&self, id: TradeNodeId) -> Option<&TradeNode> {
        self.nodes.iter().find(|n| n.id == id)
    }
}

// ---------------------------------------------------------------------------
// Per-nation budget (computed each secondary tick)
// ---------------------------------------------------------------------------

/// Budget breakdown for one nation, computed each secondary (monthly) tick.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct BudgetEntry {
    pub tax_income: f64,
    pub production_income: f64,
    pub trade_income: f64,
    pub total_income: f64,
    pub army_maintenance: f64,
    pub advisor_cost: f64,
    pub loan_interest: f64,
    pub total_expenses: f64,
    pub balance: f64,
}

/// Per-nation budget storage.
#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct NationBudgets {
    pub budgets: Vec<BudgetEntry>,
}

impl NationBudgets {
    pub fn new(nation_count: usize) -> Self {
        Self { budgets: vec![BudgetEntry::default(); nation_count] }
    }

    pub fn get(&self, id: NationId) -> Option<&BudgetEntry> {
        self.budgets.get(id.index())
    }
}

// ---------------------------------------------------------------------------
// Economy config (data-driven, all formulas tweakable)
// ---------------------------------------------------------------------------

/// Configuration for the economy. Every formula coefficient is exposed so game
/// makers can tune without writing code.
#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct EconomyConfig {
    /// Tax income per tax development point per province.
    pub base_tax_per_dev: f64,
    /// Production income per production development point × good price.
    pub base_production_per_dev: f64,
    /// Trade income per production development (flat mode, no trade network).
    pub base_trade_per_dev: f64,
    /// Army maintenance cost per unit of total army strength.
    pub army_maintenance_per_strength: f64,
    /// Advisor cost per advisor (flat).
    pub advisor_cost: f64,
    /// Interest rate per loan per secondary tick.
    pub loan_interest_rate: f64,
    /// Stability hit when treasury goes negative and no loans available.
    pub bankruptcy_stability_hit: i8,
    /// Manpower recovery per manpower development per secondary tick.
    pub manpower_recovery_per_dev: f64,
}

impl Default for EconomyConfig {
    fn default() -> Self {
        Self {
            base_tax_per_dev: 1.0,
            base_production_per_dev: 0.5,
            base_trade_per_dev: 0.3,
            army_maintenance_per_strength: 0.1,
            advisor_cost: 1.0,
            loan_interest_rate: 0.04,
            bankruptcy_stability_hit: -3,
            manpower_recovery_per_dev: 250.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Collect taxes: runs every secondary tick (e.g. monthly).
/// For each nation, sums tax development of owned provinces.
pub fn system_economy_collect_taxes(
    config: Res<EconomyConfig>,
    bounds: Res<WorldBounds>,
    provinces: Res<ProvinceStore>,
    mut nations: ResMut<NationStore>,
    mut budgets: ResMut<NationBudgets>,
) {
    let _ = bounds;
    let nc = nations.nations.len();

    // Zero out income fields.
    for b in budgets.budgets.iter_mut().take(nc) {
        b.tax_income = 0.0;
        b.production_income = 0.0;
        b.trade_income = 0.0;
        b.total_income = 0.0;
    }

    // Sum province contributions.
    for prov in &provinces.provinces {
        if let Some(owner) = prov.owner {
            if owner.index() < nc {
                let b = &mut budgets.budgets[owner.index()];
                b.tax_income += prov.development[0] as f64 * config.base_tax_per_dev;
                b.production_income += prov.development[1] as f64 * config.base_production_per_dev;
                b.trade_income += prov.development[1] as f64 * config.base_trade_per_dev;
            }
        }
    }

    // Compute total income and apply to treasury.
    for (i, _nation) in nations.nations.iter_mut().enumerate() {
        if i < budgets.budgets.len() {
            let b = &mut budgets.budgets[i];
            b.total_income = b.tax_income + b.production_income + b.trade_income;
        }
    }
}

/// Compute expenses: army maintenance, advisors, loan interest.
pub fn system_economy_expenses(
    config: Res<EconomyConfig>,
    _bounds: Res<WorldBounds>,
    mut budgets: ResMut<NationBudgets>,
    army_query: Query<&Army>,
) {
    let nc = budgets.budgets.len();

    // Zero expenses.
    for b in budgets.budgets.iter_mut().take(nc) {
        b.army_maintenance = 0.0;
        b.advisor_cost = 0.0;
        b.loan_interest = 0.0;
        b.total_expenses = 0.0;
    }

    // Sum army maintenance per nation.
    for army in army_query.iter() {
        let idx = army.owner.index();
        if idx < nc {
            budgets.budgets[idx].army_maintenance +=
                army.strength as f64 * config.army_maintenance_per_strength;
        }
    }

    // Compute totals.
    for b in budgets.budgets.iter_mut().take(nc) {
        b.total_expenses = b.army_maintenance + b.advisor_cost + b.loan_interest;
    }
}

/// Apply balance: income - expenses → treasury.
pub fn system_economy_balance(
    _config: Res<EconomyConfig>,
    mut nations: ResMut<NationStore>,
    mut budgets: ResMut<NationBudgets>,
) {
    for (i, nation) in nations.nations.iter_mut().enumerate() {
        if i < budgets.budgets.len() {
            let b = &mut budgets.budgets[i];
            b.balance = b.total_income - b.total_expenses;
            nation.treasury += b.balance as i64;
        }
    }
}

/// Manpower recovery: runs every secondary tick.
pub fn system_economy_manpower(
    config: Res<EconomyConfig>,
    _bounds: Res<WorldBounds>,
    provinces: Res<ProvinceStore>,
    mut nations: ResMut<NationStore>,
) {
    let nc = nations.nations.len();
    let mut manpower_gain = vec![0.0_f64; nc];

    for prov in &provinces.provinces {
        if let Some(owner) = prov.owner {
            if owner.index() < nc {
                manpower_gain[owner.index()] +=
                    prov.development[2] as f64 * config.manpower_recovery_per_dev;
            }
        }
    }

    for (i, nation) in nations.nations.iter_mut().enumerate() {
        if i < nc {
            nation.manpower = nation.manpower.saturating_add(manpower_gain[i] as u32);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::world::World;
    use crate::world::{ScopeId, WorldBuilder};

    fn setup_economy_world() -> World {
        let mut world = World::new();
        WorldBuilder::new()
            .provinces(4)
            .nations(2)
            .build(&mut world);
        world.insert_resource(EconomyConfig::default());
        world.insert_resource(NationBudgets::new(2));

        // Set province ownership and development.
        {
            let mut store = world.get_resource_mut::<ProvinceStore>().unwrap();
            // Nation 1 owns provinces 1,2 with dev [3,2,1]
            store.provinces[0].owner = Some(NationId::from_raw(1));
            store.provinces[0].development = [3, 2, 1];
            store.provinces[1].owner = Some(NationId::from_raw(1));
            store.provinces[1].development = [2, 3, 2];
            // Nation 2 owns province 3 with dev [5,1,3]
            store.provinces[2].owner = Some(NationId::from_raw(2));
            store.provinces[2].development = [5, 1, 3];
            // Province 4 unowned.
        }
        world
    }

    #[test]
    fn tax_collection() {
        let mut world = setup_economy_world();

        // Use a schedule to run the system.
        let mut schedule = bevy_ecs::schedule::Schedule::default();
        schedule.add_systems(system_economy_collect_taxes);
        schedule.run(&mut world);

        let budgets = world.get_resource::<NationBudgets>().unwrap();
        // Nation 1: tax_dev = 3+2=5, tax_income = 5*1.0 = 5.0
        assert_eq!(budgets.budgets[0].tax_income, 5.0);
        // Nation 2: tax_dev = 5, tax_income = 5*1.0 = 5.0
        assert_eq!(budgets.budgets[1].tax_income, 5.0);
    }

    #[test]
    fn production_income() {
        let mut world = setup_economy_world();

        let mut schedule = bevy_ecs::schedule::Schedule::default();
        schedule.add_systems(system_economy_collect_taxes);
        schedule.run(&mut world);

        let budgets = world.get_resource::<NationBudgets>().unwrap();
        // Nation 1: prod_dev = 2+3=5, prod_income = 5*0.5 = 2.5
        assert_eq!(budgets.budgets[0].production_income, 2.5);
    }

    #[test]
    fn balance_updates_treasury() {
        let mut world = setup_economy_world();
        // Manually set budget.
        {
            let mut budgets = world.get_resource_mut::<NationBudgets>().unwrap();
            budgets.budgets[0].total_income = 10.0;
            budgets.budgets[0].total_expenses = 3.0;
        }

        let mut schedule = bevy_ecs::schedule::Schedule::default();
        schedule.add_systems(system_economy_balance);
        schedule.run(&mut world);

        let nations = world.get_resource::<NationStore>().unwrap();
        assert_eq!(nations.nations[0].treasury, 7); // 10 - 3 = 7
    }

    #[test]
    fn manpower_recovery() {
        let mut world = setup_economy_world();

        let mut schedule = bevy_ecs::schedule::Schedule::default();
        schedule.add_systems(system_economy_manpower);
        schedule.run(&mut world);

        let nations = world.get_resource::<NationStore>().unwrap();
        // Nation 1: manpower_dev = 1+2=3, gain = 3*250 = 750
        assert_eq!(nations.nations[0].manpower, 750);
        // Nation 2: manpower_dev = 3, gain = 3*250 = 750
        assert_eq!(nations.nations[1].manpower, 750);
    }

    #[test]
    fn goods_registry() {
        let mut reg = GoodsRegistry::new();
        let wheat = reg.register("Wheat".into(), 2.0);
        let iron = reg.register("Iron".into(), 5.0);
        assert_eq!(reg.base_price(wheat), 2.0);
        assert_eq!(reg.base_price(iron), 5.0);
        assert_eq!(reg.goods.len(), 2);
    }
}
