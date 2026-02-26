//! Batch processing helpers for grand strategy: process provinces/nations in
//! cache-friendly chunks. Use with rayon for parallel iteration over SoA columns.

use std::num::NonZeroU32;
use rayon::prelude::*;
use crate::world::{ProvinceId, ProvinceStore, WorldBounds};

/// Process all provinces in parallel batches. Optimal when each province
/// can be updated independently (e.g. local development, population growth).
#[inline]
pub fn par_provinces_mut<F>(bounds: &WorldBounds, store: &mut ProvinceStore, f: F)
where
    F: Fn(ProvinceId, &mut crate::archetypes::Province) + Send + Sync,
{
    let count = bounds.province_count as usize;
    store.items.par_iter_mut().enumerate().for_each(|(i, p)| {
        if i < count {
            let id = ProvinceId(NonZeroU32::new((i + 1) as u32).unwrap());
            f(id, p);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn par_provinces_mut_updates_all() {
        let bounds = WorldBounds {
            province_count: 3,
            nation_count: 1,
        };
        let mut store = ProvinceStore::new(3);

        par_provinces_mut(&bounds, &mut store, |_id, p| {
            p.population += 100;
        });

        for p in &store.items {
            assert_eq!(p.population, 100);
        }
    }

    #[test]
    fn par_provinces_mut_receives_correct_ids() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let bounds = WorldBounds {
            province_count: 4,
            nation_count: 1,
        };
        let mut store = ProvinceStore::new(4);

        let sum = AtomicU32::new(0);
        par_provinces_mut(&bounds, &mut store, |id, _p| {
            sum.fetch_add(id.0.get(), Ordering::Relaxed);
        });

        // Sum of 1+2+3+4 = 10
        assert_eq!(sum.load(Ordering::Relaxed), 10);
    }
}
