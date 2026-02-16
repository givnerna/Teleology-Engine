//! Simulation tick and schedule: day/month/year rates for grand strategy.
//!
//! Systems are registered by tick rate so we only run e.g. monthly logic
//! on month boundaries, reducing work when advancing by days.

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::{ExecutorKind, ScheduleLabel};
use std::marker::PhantomData;

use crate::world::{GameDate, GameWorld, ProvinceStore, WorldBounds};

/// Tick rate: how often a system runs when advancing time.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum TickRate {
    EveryDay,
    EveryMonth,
    EveryYear,
}

/// Advance a game date by one day in place. Use from systems or from tick_day.
pub fn advance_date_in_place(date: &mut GameDate) {
    const DAYS: [u8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let month_idx = (date.month as usize).saturating_sub(1).min(11);
    let max_day = DAYS[month_idx] as u16;

    date.day += 1;
    if date.day > max_day {
        date.day = 1;
        date.month += 1;
        if date.month > 12 {
            date.month = 1;
            date.year += 1;
        }
    }
}

/// Advance the game date by one day (system version).
pub fn advance_date_by_day(mut date: ResMut<GameDate>) {
    advance_date_in_place(&mut *date);
}

/// Run daily systems (economy tick, movement, etc.).
pub fn run_daily_systems(world: &mut GameWorld) {
    world.run_schedule(DailySchedule);
}

/// Run monthly systems (income, recruitment, events).
pub fn run_monthly_systems(world: &mut GameWorld) {
    world.run_schedule(MonthlySchedule);
}

/// Run yearly systems (census, major decisions).
pub fn run_yearly_systems(world: &mut GameWorld) {
    world.run_schedule(YearlySchedule);
}

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DailySchedule;

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct MonthlySchedule;

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct YearlySchedule;

/// Builds and holds the simulation schedule. Optimized for grand strategy:
/// - Daily: movement, attrition, sieges
/// - Monthly: income, expenses, recruitment, events
/// - Yearly: census, tech, major AI
pub struct SimulationSchedule {
    _marker: PhantomData<()>,
}

impl SimulationSchedule {
    pub fn build(world: &mut GameWorld) {
        let mut daily = Schedule::new(DailySchedule);
        daily.set_executor_kind(ExecutorKind::MultiThreaded);
        daily.add_systems(system_daily_province_tick);
        world.add_schedule(daily);

        let mut monthly = Schedule::new(MonthlySchedule);
        monthly.set_executor_kind(ExecutorKind::MultiThreaded);
        monthly.add_systems(system_monthly_income);
        world.add_schedule(monthly);

        let mut yearly = Schedule::new(YearlySchedule);
        yearly.set_executor_kind(ExecutorKind::MultiThreaded);
        world.add_schedule(yearly);
    }
}

/// Example: a system that runs every day (e.g. province population growth).
pub fn system_daily_province_tick(
    bounds: Res<WorldBounds>,
    mut provinces: ResMut<ProvinceStore>,
) {
    let _ = bounds;
    for (_id, prov) in provinces.provinces.iter_mut().enumerate() {
        // Placeholder: no-op. Real logic would modify development/population.
        let _ = prov;
    }
}

/// Example: a system that runs every month (e.g. collect taxes).
pub fn system_monthly_income(
    bounds: Res<WorldBounds>,
    provinces: Res<ProvinceStore>,
) {
    let _ = (bounds, provinces);
    // Query nations, sum province income by owner, add to treasury.
}

/// Facade for running the full simulation step (date + daily, then monthly/yearly if needed).
pub struct WorldSimulation;

impl WorldSimulation {
    /// Advance time by one day and run all systems that are due.
    pub fn tick_day(world: &mut GameWorld) {
        let (run_monthly, run_yearly) = {
            let date = world.get_resource::<GameDate>().copied().unwrap_or_default();
            let next = advance_date_by_day_inline(date);
            let run_monthly = next.day == 1;
            let run_yearly = run_monthly && next.month == 1;
            (run_monthly, run_yearly)
        };

        if let Some(mut date) = world.get_resource_mut::<GameDate>() {
            advance_date_in_place(&mut *date);
        }

        run_daily_systems(world);
        if run_monthly {
            run_monthly_systems(world);
        }
        if run_yearly {
            run_yearly_systems(world);
        }
    }
}

fn advance_date_by_day_inline(mut date: GameDate) -> GameDate {
    const DAYS: [u8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let month_idx = (date.month as usize).saturating_sub(1).min(11);
    let max_day = DAYS[month_idx] as u16;
    date.day += 1;
    if date.day > max_day {
        date.day = 1;
        date.month += 1;
        if date.month > 12 {
            date.month = 1;
            date.year += 1;
        }
    }
    date
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::WorldBuilder;

    #[test]
    fn tick_day_advances_date() {
        let mut world = World::new();
        WorldBuilder::new().provinces(1).nations(1).build(&mut world);
        SimulationSchedule::build(&mut world);
        WorldSimulation::tick_day(&mut world);
        let date = world.get_resource::<GameDate>().unwrap();
        assert_eq!(date.day, 2);
        assert_eq!(date.month, 1);
        assert_eq!(date.year, 1444);
    }

    #[test]
    fn tick_day_rolls_month() {
        let mut world = World::new();
        WorldBuilder::new().provinces(1).nations(1).build(&mut world);
        SimulationSchedule::build(&mut world);
        let mut date = world.get_resource_mut::<GameDate>().unwrap();
        date.day = 31;
        date.month = 1;
        date.year = 1444;
        drop(date);
        WorldSimulation::tick_day(&mut world);
        let date = world.get_resource::<GameDate>().unwrap();
        assert_eq!(date.day, 1);
        assert_eq!(date.month, 2);
        assert_eq!(date.year, 1444);
    }

    #[test]
    fn tick_day_rolls_year() {
        let mut world = World::new();
        WorldBuilder::new().provinces(1).nations(1).build(&mut world);
        SimulationSchedule::build(&mut world);
        let mut date = world.get_resource_mut::<GameDate>().unwrap();
        date.day = 31;
        date.month = 12;
        date.year = 1444;
        drop(date);
        WorldSimulation::tick_day(&mut world);
        let date = world.get_resource::<GameDate>().unwrap();
        assert_eq!(date.day, 1);
        assert_eq!(date.month, 1);
        assert_eq!(date.year, 1445);
    }
}
