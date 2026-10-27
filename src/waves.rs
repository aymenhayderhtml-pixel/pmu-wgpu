use crate::collision::CollisionGrid;
use crate::enemy::{Enemy, ENEMY_HEIGHT, ENEMY_RADIUS};
use crate::nav::NavGrid;
use glam::{Vec2, Vec3};

pub const WAVE_1_ENEMIES: u32 = 3;
pub const WAVE_INCREMENT: u32 = 2;
pub const MAX_ALIVE_ENEMIES: usize = 12;
pub const MIN_SPAWN_DIST_FROM_PLAYER: f32 = 25.0;
pub const WAVE_INTERMISSION_TIME: f32 = 4.0;
pub const KILL_SCORE: u32 = 10;

pub struct WaveManager {
    pub current_wave: u32,
    pub pending_spawns: u32,
    pub intermission_timer: f32,
    pub score: u32,
    next_enemy_id: u32,
    rng_state: u64,
}

impl WaveManager {
    pub fn new(seed: u64) -> Self {
        Self {
            current_wave: 1,
            pending_spawns: Self::enemies_for_wave_with_allies(1, 0),
            intermission_timer: 0.0,
            score: 0,
            next_enemy_id: 1,
            rng_state: seed ^ 0xA5A5_5A5A_F0F0_0F0F,
        }
    }

    pub fn reset(&mut self, seed: u64) {
        *self = Self::new(seed);
    }

    #[inline]
    pub fn enemies_for_wave_with_allies(wave: u32, living_allies: u32) -> u32 {
        WAVE_1_ENEMIES + wave.saturating_sub(1) * WAVE_INCREMENT + living_allies
    }

    pub fn add_kill_score(&mut self) {
        self.score += KILL_SCORE;
    }

    /// Updates wave spawning and returns `true` if a new wave just started this step.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        dt: f32,
        enemies: &mut Vec<Enemy>,
        player_pos: Vec3,
        living_allies_if_regenerated: u32,
        collision: &CollisionGrid,
        nav: &NavGrid,
        skeleton_node_count: usize,
        initial_clip: usize,
    ) -> bool {
        let mut started_new_wave = false;
        let alive_count = enemies.iter().filter(|e| e.is_alive()).count();

        while self.pending_spawns > 0 && enemies.len() < MAX_ALIVE_ENEMIES {
            let spawn_pos =
                pick_enemy_spawn_point(&mut self.rng_state, player_pos, collision, nav);
            let id = self.next_enemy_id;
            self.next_enemy_id = self.next_enemy_id.wrapping_add(1);
            let enemy_seed = self.rng_state ^ (u64::from(id) * 0x9E37_79B9_7F4A_7C15);

            let mut enemy = Enemy::new(
                id,
                spawn_pos,
                enemy_seed,
                skeleton_node_count,
                initial_clip,
            );
            let to_player = Vec2::new(player_pos.x - spawn_pos.x, player_pos.z - spawn_pos.z);
            if to_player.length_squared() > 1e-4 {
                enemy.yaw = f32::atan2(-to_player.x, -to_player.y);
                enemy.prev_yaw = enemy.yaw;
            }
            enemies.push(enemy);
            self.pending_spawns -= 1;
        }

        if self.pending_spawns == 0 && alive_count == 0 {
            self.intermission_timer += dt;
            if self.intermission_timer >= WAVE_INTERMISSION_TIME {
                self.intermission_timer = 0.0;
                self.current_wave += 1;
                self.pending_spawns = Self::enemies_for_wave_with_allies(
                    self.current_wave,
                    living_allies_if_regenerated,
                );
                started_new_wave = true;
                println!(
                    "Wave {} started! Spawning {} enemies (living allies: {}).",
                    self.current_wave, self.pending_spawns, living_allies_if_regenerated
                );
            }
        }

        started_new_wave
    }
}

pub fn pick_enemy_spawn_point(
    rng_state: &mut u64,
    player_pos: Vec3,
    collision: &CollisionGrid,
    nav: &NavGrid,
) -> Vec3 {
    let player_xz = Vec2::new(player_pos.x, player_pos.z);
    let min_dist_sq = MIN_SPAWN_DIST_FROM_PLAYER * MIN_SPAWN_DIST_FROM_PLAYER;
    let clearance = ENEMY_RADIUS + 0.15;

    for _ in 0..512 {
        let rx = next_unit_f32(rng_state) * 56.0 - 28.0;
        let rz = next_unit_f32(rng_state) * 56.0 - 28.0;
        let cand = Vec2::new(rx, rz);
        if cand.distance_squared(player_xz) < min_dist_sq {
            continue;
        }
        let (cx, cz) = NavGrid::world_to_cell(cand);
        if !nav.is_blocked(cx, cz) && collision.is_circle_free(rx, rz, clearance, ENEMY_HEIGHT) {
            return Vec3::new(rx, 0.0, rz);
        }
    }

    for i in 0..64 {
        let angle = (i as f32) * (std::f32::consts::TAU / 64.0);
        for &r in &[26.0_f32, 27.5, 28.5] {
            let x = (player_pos.x + r * angle.cos()).clamp(-28.5, 28.5);
            let z = (player_pos.z + r * angle.sin()).clamp(-28.5, 28.5);
            let cand = Vec2::new(x, z);
            if cand.distance_squared(player_xz) >= min_dist_sq
                && collision.is_circle_free(x, z, clearance, ENEMY_HEIGHT)
            {
                return Vec3::new(x, 0.0, z);
            }
        }
    }

    Vec3::new(26.0, 0.0, 26.0)
}

fn next_unit_f32(state: &mut u64) -> f32 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    let u = ((z ^ (z >> 31)) >> 32) as u32;
    (u >> 8) as f32 / ((1u32 << 24) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::Aabb;

    #[test]
    fn test_wave_spawn_count_progression_with_living_allies() {
        assert_eq!(WaveManager::enemies_for_wave_with_allies(1, 0), 3);
        assert_eq!(WaveManager::enemies_for_wave_with_allies(2, 0), 5);
        assert_eq!(WaveManager::enemies_for_wave_with_allies(1, 2), 5);
        assert_eq!(WaveManager::enemies_for_wave_with_allies(3, 4), 11);
    }

    #[test]
    fn test_spawn_point_validity() {
        let cube = Aabb {
            min: Vec3::new(-27.0, 0.0, -27.0),
            max: Vec3::new(-24.0, 2.0, -24.0),
        };
        let grid = CollisionGrid::new(vec![cube]);
        let nav = NavGrid::new(grid.aabbs());
        let mut seed = 0x1234_5678_9ABC_DEF0;
        let player_pos = Vec3::ZERO;

        for _ in 0..30 {
            let p = pick_enemy_spawn_point(&mut seed, player_pos, &grid, &nav);
            let dist = Vec2::new(p.x, p.z).distance(Vec2::ZERO);
            assert!(dist >= MIN_SPAWN_DIST_FROM_PLAYER - 1e-3);
            assert!(grid.is_circle_free(p.x, p.z, ENEMY_RADIUS, ENEMY_HEIGHT));
        }
    }
}
