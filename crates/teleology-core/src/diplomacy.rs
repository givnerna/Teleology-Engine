//! Diplomacy system: relations, wars, alliances, truces, and peace deals.
//!
//! Data-driven via `DiplomacyConfig`. All opinion modifiers and thresholds
//! are configurable so game makers can tune diplomacy without code.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

use crate::tags::TagTypeId;
use crate::world::{GameDate, NationId, ProvinceStore, ScopeId, WorldBounds};

// ---------------------------------------------------------------------------
// Bilateral relations (N×N matrix)
// ---------------------------------------------------------------------------

/// Bilateral diplomatic relations between two nations.
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
pub struct Relations {
    /// Opinion: -200 to +200.
    pub opinion: i16,
    /// Trust: -100 to +100. Slower to change than opinion.
    pub trust: i16,
}

/// N×N matrix of diplomatic relations between all nation pairs.
#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct DiplomaticRelations {
    /// Flat matrix: index = a.index() * nation_count + b.index().
    relations: Vec<Relations>,
    nation_count: u32,
}

impl DiplomaticRelations {
    pub fn new(nation_count: u32) -> Self {
        let n = nation_count as usize;
        Self {
            relations: vec![Relations::default(); n * n],
            nation_count,
        }
    }

    #[inline]
    fn idx(&self, a: NationId, b: NationId) -> usize {
        a.index() * self.nation_count as usize + b.index()
    }

    pub fn get(&self, a: NationId, b: NationId) -> Relations {
        self.relations.get(self.idx(a, b)).copied().unwrap_or_default()
    }

    pub fn get_mut(&mut self, a: NationId, b: NationId) -> Option<&mut Relations> {
        let idx = self.idx(a, b);
        self.relations.get_mut(idx)
    }

    /// Modify opinion symmetrically (both a→b and b→a).
    pub fn modify_opinion(&mut self, a: NationId, b: NationId, delta: i16) {
        if let Some(r) = self.get_mut(a, b) {
            r.opinion = (r.opinion as i32 + delta as i32).clamp(-200, 200) as i16;
        }
        if let Some(r) = self.get_mut(b, a) {
            r.opinion = (r.opinion as i32 + delta as i32).clamp(-200, 200) as i16;
        }
    }

    /// Modify trust symmetrically.
    pub fn modify_trust(&mut self, a: NationId, b: NationId, delta: i16) {
        if let Some(r) = self.get_mut(a, b) {
            r.trust = (r.trust as i32 + delta as i32).clamp(-100, 100) as i16;
        }
        if let Some(r) = self.get_mut(b, a) {
            r.trust = (r.trust as i32 + delta as i32).clamp(-100, 100) as i16;
        }
    }
}

// ---------------------------------------------------------------------------
// Wars
// ---------------------------------------------------------------------------

/// Stable id for a war.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WarId(pub NonZeroU32);

/// What the attacker wants to achieve.
#[derive(Clone, Serialize, Deserialize)]
pub enum WarGoal {
    /// Conquer specific provinces.
    Conquest { target_provinces: Vec<u32> },
    /// Make a nation a subject/vassal.
    Subjugation { target: NationId },
    /// Break free from a subject relationship.
    Independence,
    /// Game-defined custom war goal.
    Custom { id: u32, payload: Vec<u8> },
}

/// An active war between two coalitions.
#[derive(Clone, Serialize, Deserialize)]
pub struct War {
    pub id: WarId,
    pub attacker_leader: NationId,
    pub defender_leader: NationId,
    pub attackers: Vec<NationId>,
    pub defenders: Vec<NationId>,
    /// War score: -100 (defender winning) to +100 (attacker winning).
    pub war_score: i16,
    pub start_date: GameDate,
    pub war_goal: WarGoal,
}

// ---------------------------------------------------------------------------
// Alliances and truces
// ---------------------------------------------------------------------------

/// A bilateral alliance between two nations.
#[derive(Clone, Serialize, Deserialize)]
pub struct Alliance {
    pub nation_a: NationId,
    pub nation_b: NationId,
    pub formed: GameDate,
}

/// A truce preventing war between two nations until expiry.
#[derive(Clone, Serialize, Deserialize)]
pub struct Truce {
    pub nation_a: NationId,
    pub nation_b: NationId,
    pub expires: GameDate,
}

// ---------------------------------------------------------------------------
// War registry (central storage for all diplomatic state)
// ---------------------------------------------------------------------------

/// Central storage for wars, alliances, and truces.
#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct WarRegistry {
    pub wars: Vec<War>,
    pub alliances: Vec<Alliance>,
    pub truces: Vec<Truce>,
    next_war_raw: u32,
}

impl WarRegistry {
    pub fn new() -> Self {
        Self {
            wars: Vec::new(),
            alliances: Vec::new(),
            truces: Vec::new(),
            next_war_raw: 1,
        }
    }

    /// Declare war. Returns the WarId.
    pub fn declare_war(
        &mut self,
        attacker: NationId,
        defender: NationId,
        war_goal: WarGoal,
        date: GameDate,
    ) -> WarId {
        let id = WarId(NonZeroU32::new(self.next_war_raw).unwrap());
        self.next_war_raw += 1;

        // Remove any alliance between the two.
        self.alliances.retain(|a| {
            !((a.nation_a == attacker && a.nation_b == defender)
                || (a.nation_a == defender && a.nation_b == attacker))
        });

        self.wars.push(War {
            id,
            attacker_leader: attacker,
            defender_leader: defender,
            attackers: vec![attacker],
            defenders: vec![defender],
            war_score: 0,
            start_date: date,
            war_goal,
        });
        id
    }

    /// End a war, set truces.
    pub fn end_war(&mut self, war_id: WarId, truce_length_days: i64, current_date: GameDate) {
        let war = match self.wars.iter().find(|w| w.id == war_id) {
            Some(w) => w.clone(),
            None => return,
        };

        // Create truces between all attacker/defender pairs.
        let mut truce_date = current_date;
        // Approximate truce expiry by adding days to the year.
        let total_days = truce_date.to_days_since_epoch() + truce_length_days;
        let truce_year = (total_days / 365) as i32;
        let remaining = (total_days % 365) as u16;
        truce_date.year = truce_year;
        truce_date.month = ((remaining / 31) as u8).max(1).min(12);
        truce_date.day = (remaining % 31).max(1);

        for &att in &war.attackers {
            for &def in &war.defenders {
                self.truces.push(Truce {
                    nation_a: att,
                    nation_b: def,
                    expires: truce_date,
                });
            }
        }

        self.wars.retain(|w| w.id != war_id);
    }

    /// Check if two nations are at war with each other.
    pub fn are_at_war(&self, a: NationId, b: NationId) -> bool {
        self.wars.iter().any(|w| {
            (w.attackers.contains(&a) && w.defenders.contains(&b))
                || (w.attackers.contains(&b) && w.defenders.contains(&a))
        })
    }

    /// Check if two nations are allied.
    pub fn are_allied(&self, a: NationId, b: NationId) -> bool {
        self.alliances.iter().any(|al| {
            (al.nation_a == a && al.nation_b == b) || (al.nation_a == b && al.nation_b == a)
        })
    }

    /// Check if a truce prevents war between two nations.
    pub fn has_truce(&self, a: NationId, b: NationId) -> bool {
        self.truces.iter().any(|t| {
            (t.nation_a == a && t.nation_b == b) || (t.nation_a == b && t.nation_b == a)
        })
    }

    /// Form an alliance.
    pub fn form_alliance(&mut self, a: NationId, b: NationId, date: GameDate) {
        if !self.are_allied(a, b) {
            self.alliances.push(Alliance { nation_a: a, nation_b: b, formed: date });
        }
    }

    /// Break an alliance.
    pub fn break_alliance(&mut self, a: NationId, b: NationId) {
        self.alliances.retain(|al| {
            !((al.nation_a == a && al.nation_b == b) || (al.nation_a == b && al.nation_b == a))
        });
    }

    /// Get war by id.
    pub fn get_war(&self, id: WarId) -> Option<&War> {
        self.wars.iter().find(|w| w.id == id)
    }

    /// Get mutable war by id.
    pub fn get_war_mut(&mut self, id: WarId) -> Option<&mut War> {
        self.wars.iter_mut().find(|w| w.id == id)
    }
}

// ---------------------------------------------------------------------------
// Diplomacy config
// ---------------------------------------------------------------------------

/// Configuration for the diplomacy system. All thresholds and rates are exposed.
#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct DiplomacyConfig {
    /// Opinion decays toward 0 by this amount per secondary tick.
    pub opinion_decay_per_tick: i16,
    /// Opinion bonus for sharing the same religion tag type.
    pub same_religion_bonus: i16,
    /// Opinion bonus for sharing the same culture tag type.
    pub same_culture_bonus: i16,
    /// Opinion penalty for sharing a border.
    pub border_friction: i16,
    /// Opinion bonus for being allied.
    pub alliance_opinion_bonus: i16,
    /// Trust penalty for breaking an alliance.
    pub alliance_break_trust_penalty: i16,
    /// Default truce length in ticks after a war ends.
    pub truce_length_ticks: u32,
    /// Tag type id for religion (if set, enables same-religion opinion).
    pub religion_tag_type: Option<TagTypeId>,
    /// Tag type id for culture (if set, enables same-culture opinion).
    pub culture_tag_type: Option<TagTypeId>,
}

impl Default for DiplomacyConfig {
    fn default() -> Self {
        Self {
            opinion_decay_per_tick: 1,
            same_religion_bonus: 25,
            same_culture_bonus: 15,
            border_friction: -20,
            alliance_opinion_bonus: 50,
            alliance_break_trust_penalty: -30,
            truce_length_ticks: 60, // ~5 years in grand strategy (60 months)
            religion_tag_type: None,
            culture_tag_type: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Opinion decay: each secondary tick, opinions drift toward 0.
pub fn system_diplomacy_opinion_tick(
    config: Res<DiplomacyConfig>,
    bounds: Res<WorldBounds>,
    mut relations: ResMut<DiplomaticRelations>,
    war_reg: Res<WarRegistry>,
) {
    let n = bounds.nation_count;
    let decay = config.opinion_decay_per_tick;

    for ai in 1..=n {
        for bi in (ai + 1)..=n {
            let a = NationId::from_raw(ai);
            let b = NationId::from_raw(bi);

            // Decay opinion toward 0.
            if let Some(r) = relations.get_mut(a, b) {
                if r.opinion > 0 {
                    r.opinion = (r.opinion - decay).max(0);
                } else if r.opinion < 0 {
                    r.opinion = (r.opinion + decay).min(0);
                }
            }
            if let Some(r) = relations.get_mut(b, a) {
                if r.opinion > 0 {
                    r.opinion = (r.opinion - decay).max(0);
                } else if r.opinion < 0 {
                    r.opinion = (r.opinion + decay).min(0);
                }
            }

            // Alliance bonus (applied as standing modifier, not cumulative).
            if war_reg.are_allied(a, b) {
                // Alliance bonus is a floor, not additive each tick.
                if let Some(r) = relations.get_mut(a, b) {
                    r.opinion = r.opinion.max(config.alliance_opinion_bonus);
                }
                if let Some(r) = relations.get_mut(b, a) {
                    r.opinion = r.opinion.max(config.alliance_opinion_bonus);
                }
            }
        }
    }
}

/// Remove expired truces.
pub fn system_diplomacy_truce_expiry(
    date: Res<GameDate>,
    mut war_reg: ResMut<WarRegistry>,
) {
    let now = date.to_days_since_epoch();
    war_reg.truces.retain(|t| t.expires.to_days_since_epoch() > now);
}

/// Update war score based on occupied provinces.
pub fn system_diplomacy_war_score(
    provinces: Res<ProvinceStore>,
    mut war_reg: ResMut<WarRegistry>,
) {
    for war in &mut war_reg.wars {
        let mut attacker_occupied = 0i32;
        let mut defender_occupied = 0i32;

        for prov in &provinces.provinces {
            if let Some(owner) = prov.owner {
                // If a defender's province is occupied by an attacker.
                if war.attackers.contains(&owner) {
                    // Check if this province should belong to a defender.
                    // (Simplified: count provinces owned by attackers vs defenders.)
                    attacker_occupied += 1;
                }
                if war.defenders.contains(&owner) {
                    defender_occupied += 1;
                }
            }
        }

        // War score swings based on relative province control.
        let total = (attacker_occupied + defender_occupied).max(1);
        let raw_score = ((attacker_occupied - defender_occupied) * 100) / total;
        war.war_score = raw_score.clamp(-100, 100) as i16;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relations_symmetric_modify() {
        let mut rel = DiplomaticRelations::new(3);
        let a = NationId::from_raw(1);
        let b = NationId::from_raw(2);
        rel.modify_opinion(a, b, 50);
        assert_eq!(rel.get(a, b).opinion, 50);
        assert_eq!(rel.get(b, a).opinion, 50);
    }

    #[test]
    fn relations_clamped() {
        let mut rel = DiplomaticRelations::new(2);
        let a = NationId::from_raw(1);
        let b = NationId::from_raw(2);
        rel.modify_opinion(a, b, 250);
        assert_eq!(rel.get(a, b).opinion, 200);
        rel.modify_opinion(a, b, -500);
        assert_eq!(rel.get(a, b).opinion, -200);
    }

    #[test]
    fn declare_war_and_check() {
        let mut reg = WarRegistry::new();
        let a = NationId::from_raw(1);
        let b = NationId::from_raw(2);
        let date = GameDate::new(1444, 1, 1);

        // Form alliance first.
        reg.form_alliance(a, b, date);
        assert!(reg.are_allied(a, b));

        // Declare war breaks alliance.
        let war_id = reg.declare_war(a, b, WarGoal::Independence, date);
        assert!(reg.are_at_war(a, b));
        assert!(!reg.are_allied(a, b));

        // End war creates truce.
        reg.end_war(war_id, 1825, date); // ~5 year truce
        assert!(!reg.are_at_war(a, b));
        assert!(reg.has_truce(a, b));
    }

    #[test]
    fn alliance_formation() {
        let mut reg = WarRegistry::new();
        let a = NationId::from_raw(1);
        let b = NationId::from_raw(2);
        let c = NationId::from_raw(3);
        let date = GameDate::new(1444, 1, 1);

        reg.form_alliance(a, b, date);
        reg.form_alliance(a, c, date);
        assert!(reg.are_allied(a, b));
        assert!(reg.are_allied(b, a));
        assert!(reg.are_allied(a, c));
        assert!(!reg.are_allied(b, c));

        reg.break_alliance(a, b);
        assert!(!reg.are_allied(a, b));
    }
}
