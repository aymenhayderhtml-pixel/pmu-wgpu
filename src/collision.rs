use glam::{Mat4, Vec2, Vec3};

pub const GRID_CELL_SIZE: f32 = 4.0;
pub const GRID_HALF_EXTENT: f32 = 30.0;
pub const GRID_DIM: usize = 15; // 15 x 15 cells of 4x4 units covering [-30, 30] on X and Z

#[derive(Copy, Clone, Debug)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn from_unit_cube_matrix(model: Mat4) -> Self {
        let center = model.w_axis.truncate();
        let half_extents = (model.x_axis.truncate().abs()
            + model.y_axis.truncate().abs()
            + model.z_axis.truncate().abs())
            * 0.5;
        Self {
            min: center - half_extents,
            max: center + half_extents,
        }
    }

    pub fn expanded(&self, radius: f32) -> Self {
        let r = Vec3::splat(radius);
        Self {
            min: self.min - r,
            max: self.max + r,
        }
    }
}

/// Uniform 2D spatial grid on the XZ plane with pre-sized buckets stored in a flat `Vec`.
pub struct CollisionGrid {
    aabbs: Vec<Aabb>,
    buckets: Vec<Vec<usize>>,
}

impl CollisionGrid {
    pub fn new(aabbs: Vec<Aabb>) -> Self {
        let mut buckets: Vec<Vec<usize>> = (0..(GRID_DIM * GRID_DIM))
            .map(|_| Vec::with_capacity(8))
            .collect();

        for (idx, aabb) in aabbs.iter().enumerate() {
            let (min_cx, max_cx) = cell_range(aabb.min.x, aabb.max.x);
            let (min_cz, max_cz) = cell_range(aabb.min.z, aabb.max.z);
            for cz in min_cz..=max_cz {
                for cx in min_cx..=max_cx {
                    buckets[cz * GRID_DIM + cx].push(idx);
                }
            }
        }

        Self { aabbs, buckets }
    }

    pub fn aabbs(&self) -> &[Aabb] {
        &self.aabbs
    }

    /// Calls `visitor` for each cube index in grid cells overlapping `[min_x..max_x] x [min_z..max_z]`.
    pub fn query_region<F: FnMut(usize, &Aabb)>(
        &self,
        min_x: f32,
        max_x: f32,
        min_z: f32,
        max_z: f32,
        mut visitor: F,
    ) {
        let (c_min_x, c_max_x) = cell_range(min_x, max_x);
        let (c_min_z, c_max_z) = cell_range(min_z, max_z);

        for cz in c_min_z..=c_max_z {
            for cx in c_min_x..=c_max_x {
                let bucket = &self.buckets[cz * GRID_DIM + cx];
                for &cube_idx in bucket {
                    visitor(cube_idx, &self.aabbs[cube_idx]);
                }
            }
        }
    }

    /// Checks whether a circle `(x, z, radius)` on the ground (`y = 0..height`) is completely free of cubes.
    pub fn is_circle_free(&self, x: f32, z: f32, radius: f32, height: f32) -> bool {
        let mut free = true;
        self.query_region(x - radius, x + radius, z - radius, z + radius, |_, aabb| {
            if aabb.max.y > 0.0
                && aabb.min.y < height
                && circle_overlaps_aabb_xz(x, z, radius, aabb)
            {
                free = false;
            }
        });
        free
    }

    /// Checks whether the 3D segment from `start` to `end` is unobstructed by any cube AABB.
    pub fn has_line_of_sight(&self, start: Vec3, end: Vec3) -> bool {
        let delta = end - start;
        let dist = delta.length();
        if dist <= 1e-5 {
            return true;
        }
        let dir = delta / dist;
        let min_x = start.x.min(end.x);
        let max_x = start.x.max(end.x);
        let min_z = start.z.min(end.z);
        let max_z = start.z.max(end.z);

        let mut clear = true;
        self.query_region(min_x, max_x, min_z, max_z, |_, aabb| {
            if clear && ray_intersects_aabb(start, dir, dist, aabb).is_some() {
                clear = false;
            }
        });
        clear
    }

    /// Resolves a circular entity (player or enemy) on XZ against nearby cube AABBs and world bounds.
    pub fn resolve_player_xz(
        &self,
        pos: &mut Vec3,
        vel: &mut Vec3,
        radius: f32,
        height: f32,
        ground_half_size: f32,
    ) {
        let step_margin = 0.04;
        let player_min_y = pos.y + step_margin;
        let player_max_y = pos.y + height - 0.02;

        for _ in 0..2 {
            let query_r = radius + 0.25;
            self.query_region(
                pos.x - query_r,
                pos.x + query_r,
                pos.z - query_r,
                pos.z + query_r,
                |_, aabb| {
                    if player_max_y <= aabb.min.y || player_min_y >= aabb.max.y {
                        return;
                    }
                    if let Some((push, normal)) =
                        push_circle_out_of_aabb_xz(Vec2::new(pos.x, pos.z), radius, aabb)
                    {
                        pos.x += push.x;
                        pos.z += push.y;

                        let vn = vel.x * normal.x + vel.z * normal.y;
                        if vn < 0.0 {
                            vel.x -= normal.x * vn;
                            vel.z -= normal.y * vn;
                        }
                    }
                },
            );
        }

        let bound = (ground_half_size - radius).max(0.0);
        if pos.x < -bound {
            pos.x = -bound;
            if vel.x < 0.0 {
                vel.x = 0.0;
            }
        } else if pos.x > bound {
            pos.x = bound;
            if vel.x > 0.0 {
                vel.x = 0.0;
            }
        }

        if pos.z < -bound {
            pos.z = -bound;
            if vel.z < 0.0 {
                vel.z = 0.0;
            }
        } else if pos.z > bound {
            pos.z = bound;
            if vel.z > 0.0 {
                vel.z = 0.0;
            }
        }
    }

    /// Returns the highest supporting Y surface (`0.0` for ground, or `aabb.max.y` of any cube top)
    /// underneath the circle `(x, z, radius)` given previous foot height `prev_y`.
    pub fn support_height_at(&self, x: f32, z: f32, radius: f32, prev_y: f32) -> f32 {
        let mut support_y = 0.0_f32;
        let ledge_r = (radius - 0.02).max(0.05);
        self.query_region(x - ledge_r, x + ledge_r, z - ledge_r, z + ledge_r, |_, aabb| {
            if aabb.max.y <= prev_y + 0.08 && circle_overlaps_aabb_xz(x, z, ledge_r, aabb) {
                support_y = support_y.max(aabb.max.y);
            }
        });
        support_y
    }

    /// Casts a sphere-padded ray from `origin` along `dir` up to `max_dist` against ground and cubes.
    pub fn cast_camera_ray(
        &self,
        origin: Vec3,
        dir: Vec3,
        max_dist: f32,
        pad_radius: f32,
    ) -> f32 {
        let mut closest = max_dist;

        if dir.y < -1e-6 && origin.y > pad_radius {
            let t_ground = (pad_radius - origin.y) / dir.y;
            if t_ground >= 0.0 && t_ground < closest {
                closest = t_ground;
            }
        }

        let end = origin + dir * closest;
        let min_x = origin.x.min(end.x) - pad_radius;
        let max_x = origin.x.max(end.x) + pad_radius;
        let min_z = origin.z.min(end.z) - pad_radius;
        let max_z = origin.z.max(end.z) + pad_radius;

        self.query_region(min_x, max_x, min_z, max_z, |_, aabb| {
            let padded = aabb.expanded(pad_radius);
            let target_box = if point_inside_aabb(origin, &padded) {
                *aabb
            } else {
                padded
            };

            if let Some(t) = ray_intersects_aabb(origin, dir, closest, &target_box) {
                if t < closest {
                    closest = t;
                }
            }
        });

        closest
    }
}

pub fn push_circle_out_of_aabb_xz(
    center: Vec2,
    radius: f32,
    aabb: &Aabb,
) -> Option<(Vec2, Vec2)> {
    let closest_x = center.x.clamp(aabb.min.x, aabb.max.x);
    let closest_z = center.y.clamp(aabb.min.z, aabb.max.z);
    let diff = Vec2::new(center.x - closest_x, center.y - closest_z);
    let dist_sq = diff.length_squared();

    if dist_sq >= radius * radius {
        return None;
    }

    if dist_sq > 1e-10 {
        let dist = dist_sq.sqrt();
        let normal = diff / dist;
        let pen = radius - dist;
        Some((normal * pen, normal))
    } else {
        let d_min_x = (center.x - aabb.min.x).abs();
        let d_max_x = (aabb.max.x - center.x).abs();
        let d_min_z = (center.y - aabb.min.z).abs();
        let d_max_z = (aabb.max.z - center.y).abs();

        let min_d = d_min_x.min(d_max_x).min(d_min_z).min(d_max_z);
        if (min_d - d_min_x).abs() < 1e-6 {
            let n = Vec2::new(-1.0, 0.0);
            Some((n * (d_min_x + radius), n))
        } else if (min_d - d_max_x).abs() < 1e-6 {
            let n = Vec2::new(1.0, 0.0);
            Some((n * (d_max_x + radius), n))
        } else if (min_d - d_min_z).abs() < 1e-6 {
            let n = Vec2::new(0.0, -1.0);
            Some((n * (d_min_z + radius), n))
        } else {
            let n = Vec2::new(0.0, 1.0);
            Some((n * (d_max_z + radius), n))
        }
    }
}

#[inline]
pub fn circle_overlaps_aabb_xz(x: f32, z: f32, radius: f32, aabb: &Aabb) -> bool {
    let cx = x.clamp(aabb.min.x, aabb.max.x);
    let cz = z.clamp(aabb.min.z, aabb.max.z);
    let dx = x - cx;
    let dz = z - cz;
    dx * dx + dz * dz < radius * radius
}

#[inline]
fn point_inside_aabb(p: Vec3, aabb: &Aabb) -> bool {
    p.x >= aabb.min.x
        && p.x <= aabb.max.x
        && p.y >= aabb.min.y
        && p.y <= aabb.max.y
        && p.z >= aabb.min.z
        && p.z <= aabb.max.z
}

pub fn ray_intersects_aabb(origin: Vec3, dir: Vec3, max_t: f32, aabb: &Aabb) -> Option<f32> {
    let mut t_min = 0.0_f32;
    let mut t_max = max_t;

    for axis in 0..3 {
        let o = origin[axis];
        let d = dir[axis];
        let b_min = aabb.min[axis];
        let b_max = aabb.max[axis];

        if d.abs() < 1e-7 {
            if o < b_min || o > b_max {
                return None;
            }
        } else {
            let inv_d = 1.0 / d;
            let mut t1 = (b_min - o) * inv_d;
            let mut t2 = (b_max - o) * inv_d;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            t_min = t_min.max(t1);
            t_max = t_max.min(t2);
            if t_min > t_max {
                return None;
            }
        }
    }

    Some(t_min)
}

#[inline]
fn cell_index(coord: f32) -> usize {
    let shifted = (coord + GRID_HALF_EXTENT) / GRID_CELL_SIZE;
    let idx = shifted.floor() as isize;
    idx.clamp(0, (GRID_DIM as isize) - 1) as usize
}

#[inline]
fn cell_range(min_c: f32, max_c: f32) -> (usize, usize) {
    (cell_index(min_c), cell_index(max_c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circle_vs_aabb_push_out() {
        let aabb = Aabb {
            min: Vec3::new(-1.0, 0.0, -1.0),
            max: Vec3::new(1.0, 2.0, 1.0),
        };
        let radius = 0.4;

        let (push, normal) =
            push_circle_out_of_aabb_xz(Vec2::new(1.25, 0.0), radius, &aabb).expect("should overlap");
        assert!((push.x - 0.15).abs() < 1e-5);
        assert!(push.y.abs() < 1e-5);
        assert!((normal.x - 1.0).abs() < 1e-5);

        assert!(push_circle_out_of_aabb_xz(Vec2::new(1.5, 0.0), radius, &aabb).is_none());
    }

    #[test]
    fn test_wall_sliding() {
        let wall = Aabb {
            min: Vec3::new(-5.0, 0.0, -1.0),
            max: Vec3::new(5.0, 2.0, 1.0),
        };
        let grid = CollisionGrid::new(vec![wall]);

        let mut pos = Vec3::new(-1.0, 0.0, 1.25);
        let mut vel = Vec3::new(-4.0, 0.0, -3.0);
        grid.resolve_player_xz(&mut pos, &mut vel, 0.4, 2.0, 30.0);

        assert!((pos.z - 1.4).abs() < 1e-4);
        assert!((pos.x - (-1.0)).abs() < 1e-4);
        assert!((vel.x - (-4.0)).abs() < 1e-4);
        assert!(vel.z.abs() < 1e-4);
    }

    #[test]
    fn test_grid_lookups_find_right_cubes() {
        let aabb_a = Aabb {
            min: Vec3::new(-22.0, 0.0, -22.0),
            max: Vec3::new(-20.0, 2.0, -20.0),
        };
        let aabb_b = Aabb {
            min: Vec3::new(14.0, 0.0, 14.0),
            max: Vec3::new(16.0, 2.0, 16.0),
        };
        let grid = CollisionGrid::new(vec![aabb_a, aabb_b]);

        let mut found_near_a = Vec::new();
        grid.query_region(-23.0, -19.0, -23.0, -19.0, |idx, _| found_near_a.push(idx));
        assert!(found_near_a.contains(&0));
        assert!(!found_near_a.contains(&1));

        let mut found_near_b = Vec::new();
        grid.query_region(13.5, 16.5, 13.5, 16.5, |idx, _| found_near_b.push(idx));
        assert!(found_near_b.contains(&1));
        assert!(!found_near_b.contains(&0));
    }

    #[test]
    fn test_camera_ray_hits_cube_and_ground() {
        let obstacle = Aabb {
            min: Vec3::new(-1.0, 0.0, 3.0),
            max: Vec3::new(1.0, 4.0, 4.5),
        };
        let grid = CollisionGrid::new(vec![obstacle]);

        let head = Vec3::new(0.0, 2.0, 0.0);
        let hit_dist = grid.cast_camera_ray(head, Vec3::Z, 8.5, 0.2);
        assert!((hit_dist - 2.8).abs() < 1e-4);

        let clear_dist = grid.cast_camera_ray(head, Vec3::NEG_Z, 8.5, 0.2);
        assert!((clear_dist - 8.5).abs() < 1e-4);
    }
}
