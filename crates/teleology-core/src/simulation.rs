//! Simulation tick and schedule with configurable time granularity.
//!
//! The three schedule tiers (primary/secondary/tertiary) fire based on
//! `TimeConfig` thresholds. For grand strategy this is Day/Month/Year;
//! for tactical games Hour/Day/Month; for RTS Second/Minute/Hour; etc.

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::{ExecutorKind, ScheduleLabel};
use std::marker::PhantomData;

use crate::world::{GameDate, GameTime, GameWorld, ProvinceStore, TickUnit, TimeConfig, WorldBounds};

/// Tick rate: how often a system runs when advancing time.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum TickRate {
    /// Runs every tick (primary schedule).
    EveryTick,
    /// Runs every `secondary_every` ticks (secondary schedule).
    EverySecondary,
    /// Runs every `tertiary_every` ticks (tertiary schedule).
    EveryTertiary,
    // Legacy aliases for backward compatibility.
    EveryDay,
    EveryMonth,
    EveryYear,
}

/// Advance GameTime by one tick according to the configured tick unit.
pub fn advance_time_in_place(time: &mut GameTime, unit: TickUnit) {
    time.tick += 1;
    match unit {
        TickUnit::Second => advance_by_second(time),
        TickUnit::Minute => advance_by_minute(time),
        TickUnit::Hour => advance_by_hour(time),
        TickUnit::Day => advance_by_day(time),
        TickUnit::Week => {
            for _ in 0..7 {
                advance_by_day(time);
            }
        }
        TickUnit::Month => advance_by_month(time),
        TickUnit::Year => advance_by_year(time),
    }
}

fn advance_by_second(time: &mut GameTime) {
    time.second += 1;
    if time.second >= 60 {
        time.second = 0;
        advance_by_minute(time);
    }
}

fn advance_by_minute(time: &mut GameTime) {
    time.minute += 1;
    if time.minute >= 60 {
        time.minute = 0;
        advance_by_hour(time);
    }
}

fn advance_by_hour(time: &mut GameTime) {
    time.hour += 1;
    if time.hour >= 24 {
        time.hour = 0;
        advance_by_day(time);
    }
}

fn advance_by_day(time: &mut GameTime) {
    const DAYS: [u8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let month_idx = (time.month as usize).saturating_sub(1).min(11);
    let max_day = DAYS[month_idx] as u16;

    time.day += 1;
    if time.day > max_day {
        time.day = 1;
        advance_by_month_no_day(time);
    }
}

fn advance_by_month_no_day(time: &mut GameTime) {
    time.month += 1;
    if time.month > 12 {
        time.month = 1;
        time.year += 1;
    }
}

fn advance_by_month(time: &mut GameTime) {
    advance_by_month_no_day(time);
}

fn advance_by_year(time: &mut GameTime) {
    time.year += 1;
}

/// Backward-compatible: advance a GameDate by one day in place.
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

/// Advance the game date by one day (system version, backward compatible).
pub fn advance_date_by_day(mut date: ResMut<GameDate>) {
    advance_date_in_place(&mut *date);
}

/// Run primary systems (every tick).
pub fn run_daily_systems(world: &mut GameWorld) {
    world.run_schedule(DailySchedule);
}

/// Run secondary systems (e.g. monthly).
pub fn run_monthly_systems(world: &mut GameWorld) {
    world.run_schedule(MonthlySchedule);
}

/// Run tertiary systems (e.g. yearly).
pub fn run_yearly_systems(world: &mut GameWorld) {
    world.run_schedule(YearlySchedule);
}

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DailySchedule;

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct MonthlySchedule;

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct YearlySchedule;

/// Builds and holds the simulation schedule. The three schedule tiers map to:
/// - DailySchedule = primary (every tick)
/// - MonthlySchedule = secondary (every `secondary_every` ticks)
/// - YearlySchedule = tertiary (every `tertiary_every` ticks)
///
/// The labels `Daily`/`Monthly`/`Yearly` are kept for backward compatibility
/// but their actual firing rate is controlled by `TimeConfig`.
pub struct SimulationSchedule {
    _marker: PhantomData<()>,
}

impl SimulationSchedule {
    pub fn build(world: &mut GameWorld) {
        let mut daily = Schedule::new(DailySchedule);
        daily.set_executor_kind(ExecutorKind::MultiThreaded);
        daily.add_systems(system_daily_province_tick);

        let mut monthly = Schedule::new(MonthlySchedule);
        monthly.set_executor_kind(ExecutorKind::MultiThreaded);
        monthly.add_systems(system_monthly_income);

        let mut yearly = Schedule::new(YearlySchedule);
        yearly.set_executor_kind(ExecutorKind::MultiThreaded);

        // Economy systems (secondary tick = monthly).
        if world.get_resource::<crate::economy::EconomyConfig>().is_some() {
            monthly.add_systems((
                crate::economy::system_economy_collect_taxes,
                crate::economy::system_economy_expenses,
            ));
            monthly.add_systems(crate::economy::system_economy_balance);
            monthly.add_systems(crate::economy::system_economy_manpower);
        }

        // Diplomacy systems (secondary tick = monthly).
        if world.get_resource::<crate::diplomacy::DiplomacyConfig>().is_some() {
            monthly.add_systems((
                crate::diplomacy::system_diplomacy_opinion_tick,
                crate::diplomacy::system_diplomacy_truce_expiry,
                crate::diplomacy::system_diplomacy_war_score,
            ));
        }

        // Combat systems (registered based on active combat model).
        if let Some(model) = world.get_resource::<crate::combat::CombatModel>() {
            match model.clone() {
                crate::combat::CombatModel::StackBased(_) => {
                    daily.add_systems((
                        crate::combat::stack::system_stack_detect_battles,
                        crate::combat::stack::system_stack_resolve_battles,
                        crate::combat::stack::system_stack_siege_tick,
                        crate::combat::stack::system_stack_army_movement,
                        crate::combat::stack::system_stack_org_recovery,
                    ));
                }
                crate::combat::CombatModel::OneUnitPerTile(_) => {
                    daily.add_systems((
                        crate::combat::tile::system_tile_enforce_1upt,
                        crate::combat::tile::system_tile_movement,
                        crate::combat::tile::system_tile_reset_movement,
                    ));
                }
                crate::combat::CombatModel::Deployment(_) => {
                    daily.add_systems((
                        crate::combat::deployment::system_deployment_initiate,
                        crate::combat::deployment::system_deployment_resolve_round,
                    ));
                }
                crate::combat::CombatModel::TacticalGrid(_) => {
                    daily.add_systems((
                        crate::combat::tactical::system_tactical_create_grid,
                        crate::combat::tactical::system_tactical_tick,
                    ));
                }
            }
        }

        // Population systems (secondary tick = monthly).
        if world.get_resource::<crate::population::PopulationConfig>().is_some() {
            monthly.add_systems((
                crate::population::system_pop_growth,
                crate::population::system_pop_unrest,
                crate::population::system_pop_assimilation,
            ));
        }

        world.add_schedule(daily);
        world.add_schedule(monthly);
        world.add_schedule(yearly);
    }
}

/// Placeholder: runs every primary tick.
pub fn system_daily_province_tick(
    bounds: Res<WorldBounds>,
    mut provinces: ResMut<ProvinceStore>,
) {
    let _ = bounds;
    for (_id, prov) in provinces.items.iter_mut().enumerate() {
        let _ = prov;
    }
}

/// Placeholder: runs every secondary tick.
pub fn system_monthly_income(
    bounds: Res<WorldBounds>,
    provinces: Res<ProvinceStore>,
) {
    let _ = (bounds, provinces);
}

/// Facade for running the full simulation step.
pub struct WorldSimulation;

impl WorldSimulation {
    /// Advance time by one tick and run all systems that are due.
    /// Uses `TimeConfig` to determine tick unit and schedule thresholds.
    /// Falls back to day-based advancement if `TimeConfig` or `GameTime` are missing.
    pub fn tick(world: &mut GameWorld) {
        let config = world.get_resource::<TimeConfig>().cloned().unwrap_or_default();

        if let Some(mut time) = world.get_resource_mut::<GameTime>() {
            let prev_tick = time.tick;
            advance_time_in_place(&mut *time, config.tick_unit);
            let new_tick = time.tick;

            // Sync GameDate from GameTime.
            let date = time.to_date();
            drop(time);
            if let Some(mut d) = world.get_resource_mut::<GameDate>() {
                *d = date;
            }

            // Run primary schedule (every tick).
            run_daily_systems(world);

            // Run secondary schedule when threshold crossed.
            let run_secondary = config.secondary_every > 0
                && (new_tick / config.secondary_every as u64) != (prev_tick / config.secondary_every as u64);
            if run_secondary {
                run_monthly_systems(world);
            }

            // Run tertiary schedule when threshold crossed.
            let run_tertiary = config.tertiary_every > 0
                && (new_tick / config.tertiary_every as u64) != (prev_tick / config.tertiary_every as u64);
            if run_tertiary {
                run_yearly_systems(world);
            }
        } else {
            // Fallback: legacy day-based tick for worlds without GameTime.
            Self::tick_day(world);
        }
    }

    /// Legacy: advance time by one day. Kept for backward compatibility.
    pub fn tick_day(world: &mut GameWorld) {
        let (run_monthly, run_yearly) = {
            let date = world.get_resource::<GameDate>().copied().unwrap_or_default();
            let next = advance_date_by_day_inline(date);
            let run_monthly = next.day == 1;
            let run_yearly = run_monthly && next.month == 1;
            (run_monthly, run_yearly)
        };

        // Advance GameTime if present.
        if let Some(mut time) = world.get_resource_mut::<GameTime>() {
            advance_time_in_place(&mut *time, TickUnit::Day);
            let date = time.to_date();
            drop(time);
            if let Some(mut d) = world.get_resource_mut::<GameDate>() {
                *d = date;
            }
        } else if let Some(mut date) = world.get_resource_mut::<GameDate>() {
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
        // Also update GameTime to match.
        if let Some(mut time) = world.get_resource_mut::<GameTime>() {
            time.day = 31;
            time.month = 1;
            time.year = 1444;
        }
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
        if let Some(mut time) = world.get_resource_mut::<GameTime>() {
            time.day = 31;
            time.month = 12;
            time.year = 1444;
        }
        WorldSimulation::tick_day(&mut world);
        let date = world.get_resource::<GameDate>().unwrap();
        assert_eq!(date.day, 1);
        assert_eq!(date.month, 1);
        assert_eq!(date.year, 1445);
    }

    #[test]
    fn tick_with_hourly_config() {
        let mut world = World::new();
        WorldBuilder::new()
            .provinces(1)
            .nations(1)
            .time_config(TimeConfig::tactical())
            .build(&mut world);
        SimulationSchedule::build(&mut world);

        // Tick 23 times (23 hours).
        for _ in 0..23 {
            WorldSimulation::tick(&mut world);
        }
        let time = world.get_resource::<GameTime>().unwrap();
        assert_eq!(time.hour, 23);
        assert_eq!(time.day, 1);

        // 24th tick should roll over to next day.
        WorldSimulation::tick(&mut world);
        let time = world.get_resource::<GameTime>().unwrap();
        assert_eq!(time.hour, 0);
        assert_eq!(time.day, 2);
        // 24 ticks = secondary threshold for tactical config.
        assert_eq!(time.tick, 24);
    }

    #[test]
    fn tick_with_second_config() {
        let mut world = World::new();
        WorldBuilder::new()
            .provinces(1)
            .nations(1)
            .time_config(TimeConfig::realtime())
            .build(&mut world);
        SimulationSchedule::build(&mut world);

        // Tick 60 times (60 seconds = 1 minute).
        for _ in 0..60 {
            WorldSimulation::tick(&mut world);
        }
        let time = world.get_resource::<GameTime>().unwrap();
        assert_eq!(time.second, 0);
        assert_eq!(time.minute, 1);
        assert_eq!(time.tick, 60);
    }

    #[test]
    fn tick_with_yearly_config() {
        let mut world = World::new();
        WorldBuilder::new()
            .provinces(1)
            .nations(1)
            .time_config(TimeConfig::civilization())
            .build(&mut world);
        SimulationSchedule::build(&mut world);

        for _ in 0..10 {
            WorldSimulation::tick(&mut world);
        }
        let time = world.get_resource::<GameTime>().unwrap();
        assert_eq!(time.year, 1454);
        assert_eq!(time.tick, 10);
    }

    #[test]
    fn custom_time_config() {
        let config = TimeConfig::custom(
            TickUnit::Week,
            4,   // secondary every 4 weeks (~month)
            52,  // tertiary every 52 weeks (~year)
            ["Weekly", "Monthly", "Yearly"],
        );
        assert_eq!(config.tick_unit, TickUnit::Week);
        assert_eq!(config.secondary_every, 4);
        assert_eq!(config.tertiary_every, 52);
        assert_eq!(config.primary_label, "Weekly");
    }

    #[test]
    fn game_time_to_date_compat() {
        let time = GameTime::with_time(1444, 6, 15, 14, 30, 0);
        let date = time.to_date();
        assert_eq!(date.year, 1444);
        assert_eq!(date.month, 6);
        assert_eq!(date.day, 15);
    }
}
