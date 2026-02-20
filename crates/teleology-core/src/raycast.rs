//! Raycasting and coordinate conversion for map hit-testing.
//!
//! Converts screen coordinates to tile/province coordinates across all map types
//! (square grid, hex grid, irregular vector polygons). The host (editor) feeds
//! viewport state each frame; scripts query via the C API.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::world::{MapKind, VectorMapLayout};

/// Viewport state: zoom, pan, and canvas rect fed by the host each frame.
/// Scripts and engine systems use this to convert screen ↔ world coordinates.
#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Viewport {
    /// Base tile size in pixels (before zoom).
    pub base_cell: f32,
    /// Zoom factor (1.0 = default).
    pub zoom: f32,
    /// Pan offset in screen pixels.
    pub pan_x: f32,
    pub pan_y: f32,
    /// Canvas origin in screen coords (top-left of the map area).
    pub canvas_x: f32,
    pub canvas_y: f32,
    /// Canvas size in screen pixels.
    pub canvas_w: f32,
    pub canvas_h: f32,
}

impl Viewport {
    /// Effective cell size after zoom.
    #[inline]
    pub fn cell_size(&self) -> f32 {
        self.base_cell * self.zoom
    }

    /// Map origin in screen coords (canvas origin + pan).
    #[inline]
    pub fn origin(&self) -> (f32, f32) {
        (self.canvas_x + self.pan_x, self.canvas_y + self.pan_y)
    }
}

/// Result of a raycast query.
#[derive(Clone, Copy, Debug, Default)]
pub struct RaycastHit {
    /// Province raw ID at the hit point (0 = no province / miss).
    pub province_raw: u32,
    /// Tile coordinates (for grid maps). (-1, -1) if not applicable.
    pub tile_x: i32,
    pub tile_y: i32,
    /// World-space coordinates of the hit point.
    pub world_x: f32,
    pub world_y: f32,
}

/// Convert screen coordinates to tile coordinates on a square grid.
pub fn screen_to_tile_square(
    screen_x: f32,
    screen_y: f32,
    viewport: &Viewport,
    width: u32,
    height: u32,
) -> Option<(u32, u32)> {
    let cell = viewport.cell_size();
    if cell <= 0.0 {
        return None;
    }
    let (ox, oy) = viewport.origin();
    let lx = (screen_x - ox) / cell;
    let ly = (screen_y - oy) / cell;
    if lx < 0.0 || ly < 0.0 {
        return None;
    }
    let tx = lx as u32;
    let ty = ly as u32;
    if tx < width && ty < height {
        Some((tx, ty))
    } else {
        None
    }
}

/// Convert screen coordinates to hex axial coordinates (q, r).
pub fn screen_to_tile_hex(
    screen_x: f32,
    screen_y: f32,
    viewport: &Viewport,
    width: u32,
    height: u32,
) -> Option<(u32, u32)> {
    let cell = viewport.cell_size();
    if cell <= 0.0 {
        return None;
    }
    let hex_w = cell * 1.732;
    let hex_h = cell * 2.0;
    let (ox, oy) = viewport.origin();
    let px = (screen_x - ox) / hex_w;
    let py = (screen_y - oy) / (hex_h * 0.5);
    let r_f = (py - 0.5).floor();
    if r_f < 0.0 {
        return None;
    }
    let r = r_f as u32;
    let q_f = (px - 0.5 * (r % 2) as f32).floor();
    if q_f < 0.0 {
        return None;
    }
    let q = q_f as u32;
    if q < width && r < height {
        Some((q, r))
    } else {
        None
    }
}

/// Convert tile coordinates to world-space center (for square grid).
pub fn tile_to_world_square(tx: u32, ty: u32, viewport: &Viewport) -> (f32, f32) {
    let cell = viewport.cell_size();
    let (ox, oy) = viewport.origin();
    (ox + (tx as f32 + 0.5) * cell, oy + (ty as f32 + 0.5) * cell)
}

/// Convert hex axial coordinates to world-space center.
pub fn tile_to_world_hex(q: u32, r: u32, viewport: &Viewport) -> (f32, f32) {
    let cell = viewport.cell_size();
    let hex_w = cell * 1.732;
    let hex_h = cell * 2.0;
    let (ox, oy) = viewport.origin();
    let cx = ox + (q as f32 + 0.5 * (r % 2) as f32) * hex_w;
    let cy = oy + (r as f32 + 0.5) * hex_h * 0.5;
    (cx, cy)
}

/// Convert screen coordinates to world space (inverse of viewport transform).
pub fn screen_to_world(screen_x: f32, screen_y: f32, viewport: &Viewport) -> (f32, f32) {
    let cell = viewport.cell_size();
    if cell <= 0.0 {
        return (0.0, 0.0);
    }
    let (ox, oy) = viewport.origin();
    ((screen_x - ox) / cell, (screen_y - oy) / cell)
}

/// Convert world coordinates to screen space.
pub fn world_to_screen(world_x: f32, world_y: f32, viewport: &Viewport) -> (f32, f32) {
    let cell = viewport.cell_size();
    let (ox, oy) = viewport.origin();
    (ox + world_x * cell, oy + world_y * cell)
}

/// Point-in-polygon test (ray casting algorithm).
/// Returns true if the point (px, py) lies inside the polygon defined by `vertices`.
pub fn point_in_polygon(px: f64, py: f64, vertices: &[[f64; 2]]) -> bool {
    let n = vertices.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (vertices[i][0], vertices[i][1]);
        let (xj, yj) = (vertices[j][0], vertices[j][1]);
        if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Find which polygon (province) contains the given world point on an irregular map.
pub fn point_to_province_irregular(
    world_x: f64,
    world_y: f64,
    layout: &VectorMapLayout,
) -> u32 {
    for polygon in &layout.polygons {
        if point_in_polygon(world_x, world_y, &polygon.vertices) {
            return polygon.province_id;
        }
    }
    0
}

/// Perform a full raycast: screen coordinates → province.
/// Works for all map types (square, hex, irregular).
pub fn raycast(
    screen_x: f32,
    screen_y: f32,
    viewport: &Viewport,
    map_kind: &MapKind,
) -> RaycastHit {
    match map_kind {
        MapKind::Square(map) => {
            if let Some((tx, ty)) = screen_to_tile_square(
                screen_x, screen_y, viewport, map.width, map.height,
            ) {
                let province_raw = map.get(tx, ty);
                let (wx, wy) = tile_to_world_square(tx, ty, viewport);
                RaycastHit {
                    province_raw,
                    tile_x: tx as i32,
                    tile_y: ty as i32,
                    world_x: wx,
                    world_y: wy,
                }
            } else {
                RaycastHit::default()
            }
        }
        MapKind::Hex(map) => {
            if let Some((q, r)) = screen_to_tile_hex(
                screen_x, screen_y, viewport, map.width, map.height,
            ) {
                let province_raw = map.get(q, r);
                let (wx, wy) = tile_to_world_hex(q, r, viewport);
                RaycastHit {
                    province_raw,
                    tile_x: q as i32,
                    tile_y: r as i32,
                    world_x: wx,
                    world_y: wy,
                }
            } else {
                RaycastHit::default()
            }
        }
        MapKind::Irregular(layout) => {
            let (wx, wy) = screen_to_world(screen_x, screen_y, viewport);
            let province_raw = point_to_province_irregular(
                wx as f64, wy as f64, layout,
            );
            RaycastHit {
                province_raw,
                tile_x: -1,
                tile_y: -1,
                world_x: wx,
                world_y: wy,
            }
        }
    }
}

/// Distance between two tiles on a square grid (Chebyshev distance).
pub fn tile_distance_square(x0: u32, y0: u32, x1: u32, y1: u32) -> u32 {
    let dx = (x0 as i32 - x1 as i32).unsigned_abs();
    let dy = (y0 as i32 - y1 as i32).unsigned_abs();
    dx.max(dy)
}

/// Axial distance between two hexes.
pub fn tile_distance_hex(q0: u32, r0: u32, q1: u32, r1: u32) -> u32 {
    let dq = (q0 as i32 - q1 as i32).unsigned_abs();
    let dr = (r0 as i32 - r1 as i32).unsigned_abs();
    let ds = ((q0 as i32 + r0 as i32) - (q1 as i32 + r1 as i32)).unsigned_abs();
    dq.max(dr).max(ds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::MapLayout;

    fn test_viewport() -> Viewport {
        Viewport {
            base_cell: 14.0,
            zoom: 2.0,
            pan_x: 0.0,
            pan_y: 0.0,
            canvas_x: 0.0,
            canvas_y: 0.0,
            canvas_w: 800.0,
            canvas_h: 600.0,
        }
    }

    #[test]
    fn square_screen_to_tile() {
        let vp = test_viewport();
        // cell_size = 28.0, origin = (0,0)
        // screen (14,14) → tile (0,0)
        assert_eq!(screen_to_tile_square(14.0, 14.0, &vp, 10, 10), Some((0, 0)));
        // screen (42,14) → tile (1,0)
        assert_eq!(screen_to_tile_square(42.0, 14.0, &vp, 10, 10), Some((1, 0)));
        // out of bounds
        assert_eq!(screen_to_tile_square(-5.0, 14.0, &vp, 10, 10), None);
        assert_eq!(screen_to_tile_square(300.0, 14.0, &vp, 10, 10), None);
    }

    #[test]
    fn square_raycast_hit() {
        let vp = test_viewport();
        let mut map = MapLayout::new(10, 10);
        map.set(1, 0, 5); // province 5 at tile (1,0)
        let kind = MapKind::Square(map);
        let hit = raycast(42.0, 14.0, &vp, &kind);
        assert_eq!(hit.province_raw, 5);
        assert_eq!(hit.tile_x, 1);
        assert_eq!(hit.tile_y, 0);
    }

    #[test]
    fn hex_screen_to_tile() {
        let vp = test_viewport();
        // hex_w = 28 * 1.732 ≈ 48.5, hex_h = 56
        // screen (24, 20) → should be q=0, r=0
        let result = screen_to_tile_hex(24.0, 20.0, &vp, 10, 10);
        assert!(result.is_some());
        let (q, r) = result.unwrap();
        assert_eq!(q, 0);
        assert_eq!(r, 0);
    }

    #[test]
    fn point_in_polygon_triangle() {
        let tri = vec![[0.0, 0.0], [10.0, 0.0], [5.0, 10.0]];
        assert!(point_in_polygon(5.0, 5.0, &tri));   // inside
        assert!(!point_in_polygon(0.0, 10.0, &tri));  // outside
        assert!(!point_in_polygon(-1.0, 0.0, &tri));  // left of
    }

    #[test]
    fn point_in_polygon_square() {
        let sq = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        assert!(point_in_polygon(5.0, 5.0, &sq));    // center
        assert!(point_in_polygon(1.0, 1.0, &sq));    // near corner
        assert!(!point_in_polygon(11.0, 5.0, &sq));  // outside
        assert!(!point_in_polygon(-1.0, 5.0, &sq));  // outside
    }

    #[test]
    fn irregular_raycast() {
        let vp = Viewport {
            base_cell: 1.0,
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
            canvas_x: 0.0,
            canvas_y: 0.0,
            canvas_w: 100.0,
            canvas_h: 100.0,
        };
        let mut layout = VectorMapLayout::new();
        layout.add(1, vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]);
        layout.add(2, vec![[10.0, 0.0], [20.0, 0.0], [20.0, 10.0], [10.0, 10.0]]);
        let kind = MapKind::Irregular(layout);

        let hit1 = raycast(5.0, 5.0, &vp, &kind);
        assert_eq!(hit1.province_raw, 1);

        let hit2 = raycast(15.0, 5.0, &vp, &kind);
        assert_eq!(hit2.province_raw, 2);

        let miss = raycast(25.0, 5.0, &vp, &kind);
        assert_eq!(miss.province_raw, 0);
    }

    #[test]
    fn screen_world_roundtrip() {
        let vp = test_viewport();
        let (wx, wy) = screen_to_world(56.0, 56.0, &vp);
        let (sx, sy) = world_to_screen(wx, wy, &vp);
        assert!((sx - 56.0).abs() < 0.01);
        assert!((sy - 56.0).abs() < 0.01);
    }

    #[test]
    fn tile_distances() {
        assert_eq!(tile_distance_square(0, 0, 3, 4), 4);
        assert_eq!(tile_distance_square(5, 5, 5, 5), 0);
        assert_eq!(tile_distance_hex(0, 0, 2, 1), 3);
    }
}
