use crate::collision::{circle_overlaps_aabb_xz, Aabb};
use glam::Vec2;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

pub const NAV_CELL_SIZE: f32 = 1.0;
pub const NAV_HALF_EXTENT: f32 = 30.0;
pub const NAV_GRID_DIM: usize = 60;
pub const NAV_TOTAL_CELLS: usize = NAV_GRID_DIM * NAV_GRID_DIM;
pub const ENEMY_NAV_RADIUS: f32 = 0.5;
pub const ASTAR_MAX_EXPANSIONS: usize = 2000;

#[derive(Copy, Clone, Debug)]
struct HeapNode {
    cell: u16,
    f_score: f32,
}

impl PartialEq for HeapNode {
    fn eq(&self, other: &Self) -> bool {
        self.cell == other.cell && self.f_score.to_bits() == other.f_score.to_bits()
    }
}

impl Eq for HeapNode {}

impl PartialOrd for HeapNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // Min-heap on f_score
        other
            .f_score
            .total_cmp(&self.f_score)
            .then_with(|| other.cell.cmp(&self.cell))
    }
}

/// Pre-sized scratch structures reused across all A* searches with zero per-call allocations.
pub struct NavSearchScratch {
    open_heap: BinaryHeap<HeapNode>,
    g_score: Vec<f32>,
    parent: Vec<u16>,
    visit_gen: Vec<u32>,
    closed_gen: Vec<u32>,
    current_gen: u32,
    raw_waypoints: Vec<Vec2>,
}

impl NavSearchScratch {
    pub fn new() -> Self {
        Self {
            open_heap: BinaryHeap::with_capacity(NAV_TOTAL_CELLS),
            g_score: vec![f32::INFINITY; NAV_TOTAL_CELLS],
            parent: vec![u16::MAX; NAV_TOTAL_CELLS],
            visit_gen: vec![0; NAV_TOTAL_CELLS],
            closed_gen: vec![0; NAV_TOTAL_CELLS],
            current_gen: 1,
            raw_waypoints: Vec::with_capacity(128),
        }
    }

    fn next_generation(&mut self) -> u32 {
        self.current_gen = self.current_gen.wrapping_add(1);
        if self.current_gen == 0 {
            self.visit_gen.fill(0);
            self.closed_gen.fill(0);
            self.current_gen = 1;
        }
        self.current_gen
    }
}

/// 60x60 navigation grid with 1.0m cells over `[-30, 30] x [-30, 30]`.
pub struct NavGrid {
    blocked: Vec<bool>,
}

impl NavGrid {
    pub fn new(aabbs: &[Aabb]) -> Self {
        let mut blocked = vec![false; NAV_TOTAL_CELLS];

        for cz in 0..NAV_GRID_DIM {
            for cx in 0..NAV_GRID_DIM {
                let center = Self::cell_to_world(cx, cz);
                for aabb in aabbs {
                    if aabb.max.y > 0.05
                        && aabb.min.y < 2.0
                        && circle_overlaps_aabb_xz(center.x, center.y, ENEMY_NAV_RADIUS + 0.35, aabb)
                    {
                        blocked[cz * NAV_GRID_DIM + cx] = true;
                        break;
                    }
                }
            }
        }

        Self { blocked }
    }

    #[inline]
    pub fn is_blocked(&self, cx: usize, cz: usize) -> bool {
        if cx >= NAV_GRID_DIM || cz >= NAV_GRID_DIM {
            return true;
        }
        self.blocked[cz * NAV_GRID_DIM + cx]
    }

    #[inline]
    pub fn set_blocked(&mut self, cx: usize, cz: usize, is_blocked: bool) {
        if cx < NAV_GRID_DIM && cz < NAV_GRID_DIM {
            self.blocked[cz * NAV_GRID_DIM + cx] = is_blocked;
        }
    }

    #[inline]
    pub fn world_to_cell(pos: Vec2) -> (usize, usize) {
        let x = ((pos.x + NAV_HALF_EXTENT) / NAV_CELL_SIZE).floor() as isize;
        let z = ((pos.y + NAV_HALF_EXTENT) / NAV_CELL_SIZE).floor() as isize;
        let cx = x.clamp(0, (NAV_GRID_DIM as isize) - 1) as usize;
        let cz = z.clamp(0, (NAV_GRID_DIM as isize) - 1) as usize;
        (cx, cz)
    }

    #[inline]
    pub fn cell_to_world(cx: usize, cz: usize) -> Vec2 {
        Vec2::new(
            -NAV_HALF_EXTENT + (cx as f32 + 0.5) * NAV_CELL_SIZE,
            -NAV_HALF_EXTENT + (cz as f32 + 0.5) * NAV_CELL_SIZE,
        )
    }

    /// Finds the nearest unblocked cell within a small search radius if `cell` itself is blocked.
    fn nearest_unblocked(&self, start_cx: usize, start_cz: usize) -> Option<(usize, usize)> {
        if !self.is_blocked(start_cx, start_cz) {
            return Some((start_cx, start_cz));
        }
        for r in 1..=4isize {
            for dz in -r..=r {
                for dx in -r..=r {
                    let nx = start_cx as isize + dx;
                    let nz = start_cz as isize + dz;
                    if nx >= 0
                        && nx < NAV_GRID_DIM as isize
                        && nz >= 0
                        && nz < NAV_GRID_DIM as isize
                        && !self.is_blocked(nx as usize, nz as usize)
                    {
                        return Some((nx as usize, nz as usize));
                    }
                }
            }
        }
        None
    }

    /// Checks whether a straight line on the nav grid from `a` to `b` passes only through unblocked cells.
    pub fn has_walkable_line_of_sight(&self, a: Vec2, b: Vec2) -> bool {
        let diff = b - a;
        let dist = diff.length();
        if dist <= 1e-4 {
            let (cx, cz) = Self::world_to_cell(a);
            return !self.is_blocked(cx, cz);
        }

        let steps = ((dist / (NAV_CELL_SIZE * 0.35)).ceil() as usize).max(2);
        for i in 0..=steps {
            let t = (i as f32) / (steps as f32);
            let p = a.lerp(b, t);
            let (cx, cz) = Self::world_to_cell(p);
            if self.is_blocked(cx, cz) {
                return false;
            }
        }
        true
    }

    /// Runs 8-neighbor A* with no corner-cutting and `ASTAR_MAX_EXPANSIONS` budget, then smooths the path.
    pub fn find_path(
        &self,
        start: Vec2,
        goal: Vec2,
        scratch: &mut NavSearchScratch,
        out_path: &mut Vec<Vec2>,
    ) -> bool {
        self.find_path_with_budget(start, goal, ASTAR_MAX_EXPANSIONS, scratch, out_path)
    }

    pub fn find_path_with_budget(
        &self,
        start: Vec2,
        goal: Vec2,
        max_expansions: usize,
        scratch: &mut NavSearchScratch,
        out_path: &mut Vec<Vec2>,
    ) -> bool {
        out_path.clear();
        let (raw_sx, raw_sz) = Self::world_to_cell(start);
        let (raw_gx, raw_gz) = Self::world_to_cell(goal);

        let Some((sx, sz)) = self.nearest_unblocked(raw_sx, raw_sz) else {
            return false;
        };
        let Some((gx, gz)) = self.nearest_unblocked(raw_gx, raw_gz) else {
            return false;
        };

        if (sx, sz) == (gx, gz) {
            out_path.push(Self::cell_to_world(gx, gz));
            return true;
        }

        let gen = scratch.next_generation();
        scratch.open_heap.clear();

        let start_idx = (sz * NAV_GRID_DIM + sx) as u16;
        let goal_idx = (gz * NAV_GRID_DIM + gx) as u16;

        scratch.visit_gen[start_idx as usize] = gen;
        scratch.g_score[start_idx as usize] = 0.0;
        scratch.parent[start_idx as usize] = start_idx;
        scratch.open_heap.push(HeapNode {
            cell: start_idx,
            f_score: octile_heuristic(sx, sz, gx, gz),
        });

        let neighbors: [(isize, isize, f32); 8] = [
            (-1, 0, 1.0),
            (1, 0, 1.0),
            (0, -1, 1.0),
            (0, 1, 1.0),
            (-1, -1, std::f32::consts::SQRT_2),
            (1, -1, std::f32::consts::SQRT_2),
            (-1, 1, std::f32::consts::SQRT_2),
            (1, 1, std::f32::consts::SQRT_2),
        ];

        let mut expansions = 0usize;
        let mut found = false;

        while let Some(current) = scratch.open_heap.pop() {
            let cur_idx = current.cell as usize;
            if scratch.closed_gen[cur_idx] == gen {
                continue;
            }
            if current.cell == goal_idx {
                found = true;
                break;
            }

            expansions += 1;
            if expansions > max_expansions {
                return false;
            }
            scratch.closed_gen[cur_idx] = gen;

            let cx = cur_idx % NAV_GRID_DIM;
            let cz = cur_idx / NAV_GRID_DIM;
            let cur_g = scratch.g_score[cur_idx];

            for &(dx, dz, step_cost) in &neighbors {
                let nx_i = cx as isize + dx;
                let nz_i = cz as isize + dz;
                if nx_i < 0
                    || nx_i >= NAV_GRID_DIM as isize
                    || nz_i < 0
                    || nz_i >= NAV_GRID_DIM as isize
                {
                    continue;
                }
                let nx = nx_i as usize;
                let nz = nz_i as usize;
                if self.is_blocked(nx, nz) {
                    continue;
                }

                // Prevent diagonal corner cutting through blocked orthogonal neighbors
                if dx != 0 && dz != 0 && (self.is_blocked(nx, cz) || self.is_blocked(cx, nz)) {
                    continue;
                }

                let n_idx = nz * NAV_GRID_DIM + nx;
                if scratch.closed_gen[n_idx] == gen {
                    continue;
                }

                let tentative_g = cur_g + step_cost;
                if scratch.visit_gen[n_idx] != gen || tentative_g < scratch.g_score[n_idx] {
                    scratch.visit_gen[n_idx] = gen;
                    scratch.g_score[n_idx] = tentative_g;
                    scratch.parent[n_idx] = current.cell;
                    let f = tentative_g + octile_heuristic(nx, nz, gx, gz);
                    scratch.open_heap.push(HeapNode {
                        cell: n_idx as u16,
                        f_score: f,
                    });
                }
            }
        }

        if !found {
            return false;
        }

        scratch.raw_waypoints.clear();
        let mut trace = goal_idx;
        let mut safety = 0usize;
        while trace != start_idx && safety < NAV_TOTAL_CELLS {
            let idx = trace as usize;
            let cx = idx % NAV_GRID_DIM;
            let cz = idx / NAV_GRID_DIM;
            scratch.raw_waypoints.push(Self::cell_to_world(cx, cz));
            trace = scratch.parent[idx];
            safety += 1;
        }
        scratch.raw_waypoints.push(start);
        scratch.raw_waypoints.reverse();

        self.smooth_path(&scratch.raw_waypoints, out_path);
        true
    }

    /// Greedy line-of-sight string-pulling path smoother.
    pub fn smooth_path(&self, raw: &[Vec2], out: &mut Vec<Vec2>) {
        out.clear();
        if raw.is_empty() {
            return;
        }
        if raw.len() == 1 {
            out.push(raw[0]);
            return;
        }

        let mut anchor_idx = 0usize;
        while anchor_idx + 1 < raw.len() {
            let mut furthest = anchor_idx + 1;
            for probe in (anchor_idx + 2)..raw.len() {
                if self.has_walkable_line_of_sight(raw[anchor_idx], raw[probe]) {
                    furthest = probe;
                }
            }
            out.push(raw[furthest]);
            anchor_idx = furthest;
        }
    }
}

#[inline]
fn octile_heuristic(x0: usize, z0: usize, x1: usize, z1: usize) -> f32 {
    let dx = (x0 as f32 - x1 as f32).abs();
    let dz = (z0 as f32 - z1 as f32).abs();
    let diag = dx.min(dz);
    let straight = dx.max(dz) - diag;
    diag * std::f32::consts::SQRT_2 + straight
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_astar_shortest_path_around_wall() {
        let mut nav = NavGrid::new(&[]);
        // Build a vertical wall at cx = 30 from cz = 20..=40
        for cz in 20..=40 {
            nav.set_blocked(30, cz, true);
        }

        let mut scratch = NavSearchScratch::new();
        let mut path = Vec::new();
        let start = NavGrid::cell_to_world(25, 30);
        let goal = NavGrid::cell_to_world(35, 30);

        assert!(nav.find_path(start, goal, &mut scratch, &mut path));
        assert!(!path.is_empty());
        // Final waypoint reaches the goal cell
        let last = *path.last().expect("non-empty path");
        assert!(last.distance(goal) < 0.1);
        // No segment of the smoothed path crosses a blocked cell
        let mut prev = start;
        for &wp in &path {
            assert!(nav.has_walkable_line_of_sight(prev, wp));
            prev = wp;
        }
    }

    #[test]
    fn test_astar_returns_none_when_fully_blocked() {
        let mut nav = NavGrid::new(&[]);
        // Completely seal a 5x5 box around goal (40, 40) so nearest_unblocked inside the box is isolated
        for cz in 34..=46 {
            for cx in 34..=46 {
                if cx == 34 || cx == 46 || cz == 34 || cz == 46 {
                    nav.set_blocked(cx, cz, true);
                }
            }
        }

        let mut scratch = NavSearchScratch::new();
        let mut path = Vec::new();
        let start = NavGrid::cell_to_world(10, 10);
        let goal = NavGrid::cell_to_world(40, 40);

        assert!(!nav.find_path(start, goal, &mut scratch, &mut path));
        assert!(path.is_empty());
    }

    #[test]
    fn test_astar_respects_expansion_budget() {
        let nav = NavGrid::new(&[]);
        let mut scratch = NavSearchScratch::new();
        let mut path = Vec::new();
        let start = NavGrid::cell_to_world(2, 2);
        let goal = NavGrid::cell_to_world(55, 55);

        // Budget of 5 expansions is too small to cross 53 cells
        assert!(!nav.find_path_with_budget(start, goal, 5, &mut scratch, &mut path));
    }

    #[test]
    fn test_path_smoothing_keeps_path_valid() {
        let nav = NavGrid::new(&[]);
        let raw = vec![
            NavGrid::cell_to_world(10, 10),
            NavGrid::cell_to_world(11, 10),
            NavGrid::cell_to_world(12, 10),
            NavGrid::cell_to_world(13, 11),
            NavGrid::cell_to_world(14, 12),
        ];
        let mut smoothed = Vec::new();
        nav.smooth_path(&raw, &mut smoothed);
        // On an open grid, smoothing collapses the intermediate waypoints to just the final destination
        assert_eq!(smoothed.len(), 1);
        assert!(nav.has_walkable_line_of_sight(raw[0], smoothed[0]));
    }
}
