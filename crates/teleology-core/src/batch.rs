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
    store.provinces.par_iter_mut().enumerate().for_each(|(i, p)| {
        if i < count {
            let id = ProvinceId(NonZeroU32::new((i + 1) as u32).unwrap());
            f(id, p);
        }
    });
}
