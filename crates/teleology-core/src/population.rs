//! Population system: demographic groups, growth, unrest, assimilation, revolts.
//!
//! Each province contains pop groups defined by culture + religion tags.
//! Unrest builds when pops differ from their ruler's culture/religion.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::tags::{NationTags, TagId, TagTypeId};
use crate::world::{NationStore, ProvinceId, ProvinceStore, ScopeId};

// ---------------------------------------------------------------------------
// Pop groups
// ---------------------------------------------------------------------------

/// A demographic group within a province.
#[derive(Clone, Serialize, Deserialize)]
pub struct PopGroup {
    /// Culture tag (from TagRegistry).
    pub culture: TagId,
    /// Religion tag (from TagRegistry).
    pub religion: TagId,
    /// Number of people in this group.
    pub size: u32,
    /// Unrest level: 0.0 = content, 100.0 = revolt imminent.
    pub unrest: f32,
}

/// Per-province population breakdown by demographic group.
#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct ProvincePops {
    /// groups[province_index] = list of pop groups in that province.
    pub groups: Vec<Vec<PopGroup>>,
}

impl ProvincePops {
    pub fn new(province_count: usize) -> Self {
        Self {
            groups: vec![Vec::new(); province_count],
        }
    }

    pub fn get(&self, id: ProvinceId) -> &[PopGroup] {
        self.groups.get(id.index()).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn get_mut(&mut self, id: ProvinceId) -> Option<&mut Vec<PopGroup>> {
        self.groups.get_mut(id.index())
    }

    /// Total population in a province.
    pub fn total_pop(&self, id: ProvinceId) -> u32 {
        self.get(id).iter().map(|g| g.size).sum()
    }

    /// Average unrest in a province (weighted by pop size).
    pub fn average_unrest(&self, id: ProvinceId) -> f32 {
        let groups = self.get(id);
        let total: u32 = groups.iter().map(|g| g.size).sum();
        if total == 0 {
            return 0.0;
        }
        let weighted: f64 = groups.iter().map(|g| g.unrest as f64 * g.size as f64).sum();
        (weighted / total as f64) as f32
    }
}

// ---------------------------------------------------------------------------
// Population config
// ---------------------------------------------------------------------------

/// Configuration for the population system.
#[derive(Resource, Clone, Serialize, Deserialize)]
pub struct PopulationConfig {
    /// Monthly growth rate (fraction, e.g. 0.001 = 0.1% per tick).
    pub base_growth_rate: f64,
    /// Max population per development level (carrying capacity).
    pub carrying_capacity_per_dev: u32,
    /// Unrest added per tick for wrong culture (vs province owner's culture).
    pub unrest_wrong_culture: f32,
    /// Unrest added per tick for wrong religion.
    pub unrest_wrong_religion: f32,
    /// Unrest from low stability (per negative stability point).
    pub unrest_per_negative_stability: f32,
    /// Natural unrest decay per tick (drift toward 0).
    pub unrest_decay: f32,
    /// Monthly chance of a minority culture pop converting to the owner's culture.
    pub assimilation_rate: f64,
    /// Monthly chance of a minority religion pop converting to the owner's religion.
    pub conversion_rate: f64,
    /// Unrest threshold that triggers a revolt.
    pub revolt_threshold: f32,
    /// Rebel army strength as fraction of revolting population.
    pub revolt_strength_ratio: f64,
    /// Tag type id for culture (must match DiplomacyConfig).
    pub culture_tag_type: Option<TagTypeId>,
    /// Tag type id for religion.
    pub religion_tag_type: Option<TagTypeId>,
}

impl Default for PopulationConfig {
    fn default() -> Self {
        Self {
            base_growth_rate: 0.001,
            carrying_capacity_per_dev: 10_000,
            unrest_wrong_culture: 1.0,
            unrest_wrong_religion: 1.5,
            unrest_per_negative_stability: 2.0,
            unrest_decay: 0.5,
            assimilation_rate: 0.002,
            conversion_rate: 0.001,
            revolt_threshold: 80.0,
            revolt_strength_ratio: 0.01,
            culture_tag_type: None,
            religion_tag_type: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Population growth: logistic curve capped by development.
pub fn system_pop_growth(
    config: Res<PopulationConfig>,
    provinces: Res<ProvinceStore>,
    mut pops: ResMut<ProvincePops>,
) {
    for (i, prov) in provinces.provinces.iter().enumerate() {
        let total_dev = prov.development[0] as u32 + prov.development[1] as u32 + prov.development[2] as u32;
        let capacity = total_dev * config.carrying_capacity_per_dev;

        if let Some(groups) = pops.groups.get_mut(i) {
            let total_pop: u32 = groups.iter().map(|g| g.size).sum();
            if total_pop >= capacity || total_pop == 0 {
                continue;
            }

            // Logistic growth: rate * pop * (1 - pop/capacity)
            let growth_factor = config.base_growth_rate * (1.0 - total_pop as f64 / capacity as f64);

            for group in groups.iter_mut() {
                let growth = (group.size as f64 * growth_factor) as u32;
                group.size = group.size.saturating_add(growth.max(1));
            }
        }
    }
}

/// Unrest calculation: wrong culture/religion relative to owner.
pub fn system_pop_unrest(
    config: Res<PopulationConfig>,
    provinces: Res<ProvinceStore>,
    nations: Res<NationStore>,
    nation_tags: Option<Res<NationTags>>,
    mut pops: ResMut<ProvincePops>,
) {
    for (i, prov) in provinces.provinces.iter().enumerate() {
        let owner = match prov.owner {
            Some(o) => o,
            None => continue,
        };

        // Get owner's culture and religion.
        let (owner_culture, owner_religion) = if let Some(ref tags) = nation_tags {
            let culture = config.culture_tag_type
                .and_then(|ty| tags.get(owner, ty));
            let religion = config.religion_tag_type
                .and_then(|ty| tags.get(owner, ty));
            (culture, religion)
        } else {
            (None, None)
        };

        // Get stability modifier.
        let stability = nations.nations.get(owner.index())
            .map(|n| n.stability)
            .unwrap_or(0);
        let stability_unrest = if stability < 0 {
            (-stability) as f32 * config.unrest_per_negative_stability
        } else {
            0.0
        };

        if let Some(groups) = pops.groups.get_mut(i) {
            for group in groups.iter_mut() {
                let mut delta: f32 = 0.0;

                // Wrong culture.
                if let Some(oc) = owner_culture {
                    if group.culture != oc {
                        delta += config.unrest_wrong_culture;
                    }
                }

                // Wrong religion.
                if let Some(or) = owner_religion {
                    if group.religion != or {
                        delta += config.unrest_wrong_religion;
                    }
                }

                // Stability effect.
                delta += stability_unrest;

                // Natural decay.
                delta -= config.unrest_decay;

                group.unrest = (group.unrest + delta).clamp(0.0, 100.0);
            }
        }
    }
}

/// Assimilation: minority culture/religion slowly converts to owner's.
pub fn system_pop_assimilation(
    config: Res<PopulationConfig>,
    provinces: Res<ProvinceStore>,
    nation_tags: Option<Res<NationTags>>,
    mut pops: ResMut<ProvincePops>,
) {
    for (i, prov) in provinces.provinces.iter().enumerate() {
        let owner = match prov.owner {
            Some(o) => o,
            None => continue,
        };

        let (owner_culture, owner_religion) = if let Some(ref tags) = nation_tags {
            let culture = config.culture_tag_type.and_then(|ty| tags.get(owner, ty));
            let religion = config.religion_tag_type.and_then(|ty| tags.get(owner, ty));
            (culture, religion)
        } else {
            continue;
        };

        if let Some(groups) = pops.groups.get_mut(i) {
            for group in groups.iter_mut() {
                // Culture assimilation.
                if let Some(oc) = owner_culture {
                    if group.culture != oc && group.size > 0 {
                        let converts = (group.size as f64 * config.assimilation_rate) as u32;
                        if converts > 0 {
                            group.size = group.size.saturating_sub(converts);
                            // In a full implementation, add converts to the matching group
                            // or create a new group with the owner's culture.
                        }
                    }
                }

                // Religion conversion.
                if let Some(or) = owner_religion {
                    if group.religion != or && group.size > 0 {
                        let converts = (group.size as f64 * config.conversion_rate) as u32;
                        if converts > 0 {
                            group.size = group.size.saturating_sub(converts);
                        }
                    }
                }
            }

            // Remove empty groups.
            groups.retain(|g| g.size > 0);
        }
    }
}

/// Revolt check: provinces with unrest above threshold spawn rebel armies.
/// Returns list of (ProvinceId, rebel_strength) for the caller to handle.
pub fn check_revolts(
    config: &PopulationConfig,
    pops: &ProvincePops,
    province_count: u32,
) -> Vec<(ProvinceId, u32)> {
    let mut revolts = Vec::new();

    for i in 0..province_count as usize {
        let pid = ProvinceId::from_raw((i + 1) as u32);
        let avg_unrest = pops.average_unrest(pid);
        if avg_unrest >= config.revolt_threshold {
            let total_pop = pops.total_pop(pid);
            let rebel_strength = (total_pop as f64 * config.revolt_strength_ratio) as u32;
            if rebel_strength > 0 {
                revolts.push((pid, rebel_strength));
            }
        }
    }

    revolts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU32;

    fn make_tag(raw: u32) -> TagId {
        TagId(NonZeroU32::new(raw).unwrap())
    }

    #[test]
    fn province_pops_total() {
        let mut pops = ProvincePops::new(2);
        let pid = ProvinceId::from_raw(1);
        pops.groups[0].push(PopGroup {
            culture: make_tag(1),
            religion: make_tag(1),
            size: 5000,
            unrest: 0.0,
        });
        pops.groups[0].push(PopGroup {
            culture: make_tag(2),
            religion: make_tag(1),
            size: 3000,
            unrest: 10.0,
        });
        assert_eq!(pops.total_pop(pid), 8000);
    }

    #[test]
    fn average_unrest_weighted() {
        let mut pops = ProvincePops::new(1);
        let pid = ProvinceId::from_raw(1);
        pops.groups[0].push(PopGroup {
            culture: make_tag(1),
            religion: make_tag(1),
            size: 9000,
            unrest: 0.0,
        });
        pops.groups[0].push(PopGroup {
            culture: make_tag(2),
            religion: make_tag(1),
            size: 1000,
            unrest: 50.0,
        });
        // Weighted: (0*9000 + 50*1000) / 10000 = 5.0
        let avg = pops.average_unrest(pid);
        assert!((avg - 5.0).abs() < 0.01);
    }

    #[test]
    fn revolt_check() {
        let config = PopulationConfig {
            revolt_threshold: 75.0,
            revolt_strength_ratio: 0.01,
            ..Default::default()
        };
        let mut pops = ProvincePops::new(2);
        // Province 1: high unrest.
        pops.groups[0].push(PopGroup {
            culture: make_tag(1),
            religion: make_tag(1),
            size: 10000,
            unrest: 90.0,
        });
        // Province 2: low unrest.
        pops.groups[1].push(PopGroup {
            culture: make_tag(1),
            religion: make_tag(1),
            size: 5000,
            unrest: 10.0,
        });

        let revolts = check_revolts(&config, &pops, 2);
        assert_eq!(revolts.len(), 1);
        assert_eq!(revolts[0].0, ProvinceId::from_raw(1));
        assert_eq!(revolts[0].1, 100); // 10000 * 0.01
    }
}
