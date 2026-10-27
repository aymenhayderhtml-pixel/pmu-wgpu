use crate::camera::lerp_angle;
use crate::collision::CollisionGrid;
use crate::combat::{
    compute_separation_force, is_in_attack_arc, ActorId, CombatantInfo, Faction,
};
use crate::enemy::{
    Enemy, EnemyState, ATTACK_COOLDOWN, ATTACK_DAMAGE, ATTACK_RANGE, ATTACK_WINDUP, CHASE_SPEED,
    ENEMY_HEIGHT, ENEMY_RADIUS, HEARING_RANGE, LOSE_INTEREST_RANGE, LOSE_INTEREST_TIME,
    MAX_ASTAR_PER_STEP, REPATH_INTERVAL, REPATH_TARGET_DRIFT, SEPARATION_RADIUS, SEPARATION_WEIGHT,
    SIGHT_HALF_ANGLE_RAD, SIGHT_RANGE, WANDER_SPEED,
};
use crate::nav::{NavGrid, NavSearchScratch};
use crate::waves::MAX_ALIVE_ENEMIES;
use crate::world::GROUND_SIZE;
use glam::{Vec2, Vec3};
use std::time::Instant;

#[derive(Copy, Clone, Debug)]
pub struct EnemyAttackEvent {
    pub target: ActorId,
    pub damage: f32,
}

impl Default for EnemyAttackEvent {
    fn default() -> Self {
        Self {
            target: ActorId::Player,
            damage: 0.0,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct AiStepStats {
    pub astar_calls: u32,
    pub astar_micros: u64,
    pub player_damage_taken: f32,
    pub attacks: [EnemyAttackEvent; MAX_ALIVE_ENEMIES],
    pub attack_count: usize,
}

impl Default for AiStepStats {
    fn default() -> Self {
        Self {
            astar_calls: 0,
            astar_micros: 0,
            player_damage_taken: 0.0,
            attacks: [EnemyAttackEvent::default(); MAX_ALIVE_ENEMIES],
            attack_count: 0,
        }
    }
}

pub fn can_perceive_player(
    enemy_pos: Vec3,
    enemy_yaw: f32,
    player_pos: Vec3,
    player_sprinting: bool,
    collision: &CollisionGrid,
) -> bool {
    let to_player_xz = Vec2::new(player_pos.x - enemy_pos.x, player_pos.z - enemy_pos.z);
    let dist_sq = to_player_xz.length_squared();

    if player_sprinting && dist_sq <= HEARING_RANGE * HEARING_RANGE {
        return true;
    }

    if dist_sq > SIGHT_RANGE * SIGHT_RANGE {
        return false;
    }

    if dist_sq > 1e-6 {
        let dir_to_player = to_player_xz / dist_sq.sqrt();
        let forward = Vec2::new(-enemy_yaw.sin(), -enemy_yaw.cos());
        if forward.dot(dir_to_player) < SIGHT_HALF_ANGLE_RAD.cos() {
            return false;
        }
    }

    let eye = enemy_pos + Vec3::new(0.0, 1.4, 0.0);
    let target = player_pos + Vec3::new(0.0, 1.1, 0.0);
    collision.has_line_of_sight(eye, target)
}

/// Selects the enemy's target among the player and all living allies, preferring `last_attacker`
/// if `threat_timer > 0.0` and the attacker is still alive and within lose-interest range.
pub fn select_enemy_target(
    enemy: &Enemy,
    targets: &[CombatantInfo],
    collision: &CollisionGrid,
) -> Option<(ActorId, Vec3)> {
    // 1. Threat memory preference (3.0s)
    if enemy.threat_timer > 0.0 {
        if let Some(pref_id) = enemy.last_attacker {
            if let Some(t) = targets
                .iter()
                .find(|t| t.active && t.faction == Faction::PlayerSide && t.id == pref_id)
            {
                let dist = Vec2::new(t.pos.x - enemy.pos.x, t.pos.z - enemy.pos.z).length();
                if dist <= LOSE_INTEREST_RANGE {
                    return Some((t.id, t.pos));
                }
            }
        }
    }

    // 2. Nearest valid target within sight (or hearing if sprinting)
    let mut best: Option<(ActorId, Vec3, f32)> = None;
    for t in targets {
        if !t.active || t.faction != Faction::PlayerSide {
            continue;
        }
        let dist = Vec2::new(t.pos.x - enemy.pos.x, t.pos.z - enemy.pos.z).length();
        let can_see_or_hear =
            can_perceive_player(enemy.pos, enemy.yaw, t.pos, t.sprinting, collision)
                || (enemy.state != EnemyState::Wander
                    && t.id == enemy.target_actor
                    && dist <= LOSE_INTEREST_RANGE);

        if can_see_or_hear && best.map_or(true, |(_, _, bd)| dist < bd) {
            best = Some((t.id, t.pos, dist));
        }
    }

    best.map(|(id, pos, _)| (id, pos))
}

/// Checks which enemies request an A* path calculation this step.
pub fn collect_enemy_astar_requests(
    enemies: &[Enemy],
    actors: &[CombatantInfo],
    collision: &CollisionGrid,
    nav: &NavGrid,
    out_requests: &mut [bool; MAX_ALIVE_ENEMIES],
) {
    *out_requests = [false; MAX_ALIVE_ENEMIES];
    for (i, enemy) in enemies.iter().enumerate().take(MAX_ALIVE_ENEMIES) {
        if !enemy.is_alive() || enemy.state != EnemyState::Chase || enemy.repath_timer > 0.0 {
            continue;
        }
        let target_pos = actors
            .iter()
            .find(|a| a.active && a.id == enemy.target_actor)
            .map(|a| a.pos)
            .unwrap_or(enemy.target_pos);
        let enemy_xz = Vec2::new(enemy.pos.x, enemy.pos.z);
        let target_xz = Vec2::new(target_pos.x, target_pos.z);

        let has_los = collision.has_line_of_sight(
            enemy.pos + Vec3::new(0.0, 1.2, 0.0),
            target_pos + Vec3::new(0.0, 1.1, 0.0),
        ) && nav.has_walkable_line_of_sight(enemy_xz, target_xz);

        if !has_los {
            let drifted = enemy.path_goal.distance(target_xz) > REPATH_TARGET_DRIFT;
            if (enemy.path.is_empty() || enemy.path_index >= enemy.path.len() || drifted)
                && out_requests.iter().filter(|&&r| r).count() < MAX_ASTAR_PER_STEP
            {
                out_requests[i] = true;
            }
        }
    }
}

pub fn update_enemies_with_targets(
    enemies: &mut Vec<Enemy>,
    actors: &[CombatantInfo],
    astar_granted: &[bool; MAX_ALIVE_ENEMIES],
    collision: &CollisionGrid,
    nav: &NavGrid,
    nav_scratch: &mut NavSearchScratch,
    dt: f32,
) -> AiStepStats {
    let mut stats = AiStepStats::default();

    let fallback_player_pos = actors
        .iter()
        .find(|a| a.id == ActorId::Player)
        .map(|a| a.pos)
        .unwrap_or(Vec3::ZERO);

    for idx in 0..enemies.len() {
        let enemy = &mut enemies[idx];
        enemy.prev_pos = enemy.pos;
        enemy.prev_yaw = enemy.yaw;
        enemy.hit_flash_timer = (enemy.hit_flash_timer - dt).max(0.0);
        enemy.repath_timer = (enemy.repath_timer - dt).max(0.0);
        enemy.threat_timer = (enemy.threat_timer - dt).max(0.0);

        if enemy.state == EnemyState::Dead {
            enemy.death_timer += dt;
            continue;
        }

        let enemy_xz = Vec2::new(enemy.pos.x, enemy.pos.z);
        let sep = compute_separation_force(
            ActorId::Enemy(enemy.id),
            enemy_xz,
            actors,
            SEPARATION_RADIUS,
        );

        let selected = select_enemy_target(enemy, actors, collision);
        if let Some((t_id, t_pos)) = selected {
            if enemy.target_actor != t_id {
                enemy.target_actor = t_id;
                enemy.path.clear();
            }
            enemy.target_pos = t_pos;
        }

        let active_target = actors
            .iter()
            .find(|a| a.active && a.faction == Faction::PlayerSide && a.id == enemy.target_actor);

        let target_pos = active_target.map(|a| a.pos).unwrap_or(fallback_player_pos);
        let target_xz = Vec2::new(target_pos.x, target_pos.z);
        let dist_to_target = enemy_xz.distance(target_xz);
        let has_los = active_target.is_some()
            && collision.has_line_of_sight(
                enemy.pos + Vec3::new(0.0, 1.2, 0.0),
                target_pos + Vec3::new(0.0, 1.1, 0.0),
            );

        let mut desired_vel_xz = Vec2::ZERO;

        match enemy.state {
            EnemyState::Wander => {
                if selected.is_some() {
                    enemy.state = EnemyState::Chase;
                    enemy.time_since_seen = 0.0;
                } else {
                    enemy.wander_timer -= dt;
                    if enemy.wander_timer <= 0.0 || enemy_xz.distance(enemy.wander_target) < 1.0 {
                        enemy.wander_timer = 3.5;
                        enemy.wander_target = next_wander_target(
                            &mut enemy.spawn_seed,
                            enemy_xz,
                            Vec2::new(fallback_player_pos.x, fallback_player_pos.z),
                        );
                    }
                    let w_spd = WANDER_SPEED * enemy.status.speed_multiplier();
                    let seek = (enemy.wander_target - enemy_xz).normalize_or_zero();
                    desired_vel_xz = seek * w_spd + sep * SEPARATION_WEIGHT;
                    if desired_vel_xz.length() > w_spd {
                        desired_vel_xz = desired_vel_xz.normalize() * w_spd;
                    }
                }
            }
            EnemyState::Chase => {
                if active_target.is_none() {
                    if let Some((new_id, new_pos)) = select_enemy_target(enemy, actors, collision) {
                        enemy.target_actor = new_id;
                        enemy.target_pos = new_pos;
                    } else {
                        enemy.state = EnemyState::Wander;
                    }
                } else {
                    if selected.is_some() || (has_los && dist_to_target <= LOSE_INTEREST_RANGE) {
                        enemy.time_since_seen = 0.0;
                    } else {
                        enemy.time_since_seen += dt;
                    }

                    if dist_to_target > LOSE_INTEREST_RANGE
                        || enemy.time_since_seen >= LOSE_INTEREST_TIME
                    {
                        enemy.state = EnemyState::Wander;
                        enemy.path.clear();
                    } else if dist_to_target <= ATTACK_RANGE && has_los {
                        enemy.state = EnemyState::Attack;
                        enemy.state_timer = ATTACK_WINDUP;
                        enemy.attack_dealt = false;
                        enemy.vel = Vec3::ZERO;
                    } else {
                        let steer_target = if has_los
                            && nav.has_walkable_line_of_sight(enemy_xz, target_xz)
                        {
                            enemy.path.clear();
                            target_xz
                        } else {
                            let can_astar =
                                idx < MAX_ALIVE_ENEMIES && astar_granted[idx];
                            if can_astar && enemy.repath_timer <= 0.0 {
                                enemy.repath_timer = REPATH_INTERVAL;
                                enemy.path_goal = target_xz;
                                enemy.path_index = 0;

                                let t0 = Instant::now();
                                let _ = nav.find_path(
                                    enemy_xz,
                                    target_xz,
                                    nav_scratch,
                                    &mut enemy.path,
                                );
                                stats.astar_calls += 1;
                                stats.astar_micros += t0.elapsed().as_micros() as u64;
                            }

                            while enemy.path_index < enemy.path.len()
                                && enemy_xz.distance(enemy.path[enemy.path_index]) < 0.65
                            {
                                enemy.path_index += 1;
                            }

                            enemy
                                .path
                                .get(enemy.path_index)
                                .copied()
                                .unwrap_or(target_xz)
                        };

                        let c_spd = CHASE_SPEED * enemy.status.speed_multiplier();
                        let seek = (steer_target - enemy_xz).normalize_or_zero();
                        desired_vel_xz = seek * c_spd + sep * SEPARATION_WEIGHT;
                        if desired_vel_xz.length() > c_spd {
                            desired_vel_xz = desired_vel_xz.normalize() * c_spd;
                        }
                    }
                }
            }
            EnemyState::Attack => {
                let to_t = target_xz - enemy_xz;
                if to_t.length_squared() > 1e-5 {
                    let target_yaw = f32::atan2(-to_t.x, -to_t.y);
                    enemy.yaw = lerp_angle(enemy.yaw, target_yaw, (15.0 * dt).min(1.0));
                }

                enemy.state_timer -= dt;
                if enemy.state_timer <= 0.0 {
                    if !enemy.attack_dealt
                        && active_target.is_some()
                        && has_los
                        && is_in_attack_arc(
                            enemy.pos,
                            enemy.yaw,
                            target_pos,
                            ATTACK_RANGE + 0.45,
                            std::f32::consts::PI,
                        )
                    {
                        if enemy.target_actor == ActorId::Player {
                            stats.player_damage_taken += ATTACK_DAMAGE;
                        }
                        if stats.attack_count < MAX_ALIVE_ENEMIES {
                            stats.attacks[stats.attack_count] = EnemyAttackEvent {
                                target: enemy.target_actor,
                                damage: ATTACK_DAMAGE,
                            };
                            stats.attack_count += 1;
                        }
                    }
                    enemy.attack_dealt = true;
                    enemy.state = EnemyState::Recover;
                    enemy.state_timer = ATTACK_COOLDOWN;
                    enemy.was_staggered = false;
                }
            }
            EnemyState::Recover => {
                let damp = (-10.0 * dt).exp();
                desired_vel_xz = Vec2::new(enemy.vel.x, enemy.vel.z) * damp;
                enemy.state_timer -= dt;
                if enemy.state_timer <= 0.0 {
                    enemy.was_staggered = false;
                    if active_target.is_some() && dist_to_target <= LOSE_INTEREST_RANGE {
                        enemy.state = EnemyState::Chase;
                        enemy.time_since_seen = 0.0;
                    } else {
                        enemy.state = EnemyState::Wander;
                    }
                }
            }
            EnemyState::Dead => {}
        }

        if enemy.state == EnemyState::Wander || enemy.state == EnemyState::Chase {
            enemy.vel.x = desired_vel_xz.x;
            enemy.vel.z = desired_vel_xz.y;
            if desired_vel_xz.length_squared() > 0.04 {
                let target_yaw = f32::atan2(-desired_vel_xz.x, -desired_vel_xz.y);
                let turn_alpha = 1.0 - (-10.0 * dt).exp();
                enemy.yaw = lerp_angle(enemy.yaw, target_yaw, turn_alpha);
            }
        } else if enemy.state == EnemyState::Recover {
            enemy.vel.x = desired_vel_xz.x;
            enemy.vel.z = desired_vel_xz.y;
        } else {
            enemy.vel.x = 0.0;
            enemy.vel.z = 0.0;
        }

        enemy.pos.x += enemy.vel.x * dt;
        enemy.pos.z += enemy.vel.z * dt;
        collision.resolve_player_xz(
            &mut enemy.pos,
            &mut enemy.vel,
            ENEMY_RADIUS,
            ENEMY_HEIGHT,
            GROUND_SIZE * 0.5,
        );
    }

    let mut i = 0;
    while i < enemies.len() {
        if enemies[i].is_removable() {
            enemies.swap_remove(i);
        } else {
            i += 1;
        }
    }

    stats
}

fn next_wander_target(seed: &mut u64, current_pos: Vec2, player_pos: Vec2) -> Vec2 {
    *seed = seed
        .wrapping_add(0x9E37_79B9_7F4A_7C15)
        .wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let angle = ((*seed >> 16) as u32 as f32 / (u32::MAX as f32)) * std::f32::consts::TAU;
    let jitter = Vec2::new(angle.cos(), angle.sin()) * 3.0;
    let to_player = (player_pos - current_pos).normalize_or_zero() * 8.0;
    let target = current_pos + to_player + jitter;
    Vec2::new(target.x.clamp(-27.0, 27.0), target.y.clamp(-27.0, 27.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::Aabb;

    #[test]
    fn test_sight_cone_and_los_blocking() {
        let wall = Aabb {
            min: Vec3::new(-2.0, 0.0, -5.0),
            max: Vec3::new(2.0, 3.0, -4.0),
        };
        let empty_grid = CollisionGrid::new(Vec::new());
        let blocked_grid = CollisionGrid::new(vec![wall]);

        let enemy_pos = Vec3::ZERO;
        let enemy_yaw = 0.0;
        let player_ahead = Vec3::new(0.0, 0.0, -10.0);

        assert!(can_perceive_player(
            enemy_pos,
            enemy_yaw,
            player_ahead,
            false,
            &empty_grid
        ));
        assert!(!can_perceive_player(
            enemy_pos,
            enemy_yaw,
            player_ahead,
            false,
            &blocked_grid
        ));
        assert!(!can_perceive_player(
            enemy_pos,
            enemy_yaw,
            Vec3::new(10.0, 0.0, 0.0),
            false,
            &empty_grid
        ));
    }

    #[test]
    fn test_hearing_sprinting_player_behind_enemy() {
        let grid = CollisionGrid::new(Vec::new());
        let enemy_pos = Vec3::ZERO;
        let enemy_yaw = 0.0;
        let player_behind = Vec3::new(0.0, 0.0, 4.5);

        assert!(!can_perceive_player(
            enemy_pos,
            enemy_yaw,
            player_behind,
            false,
            &grid
        ));
        assert!(can_perceive_player(
            enemy_pos,
            enemy_yaw,
            player_behind,
            true,
            &grid
        ));
    }

    #[test]
    fn test_enemy_target_selection_among_player_and_allies_and_threat_memory() {
        let grid = CollisionGrid::new(Vec::new());
        let mut enemy = Enemy::new(1, Vec3::ZERO, 1, 1, 0);
        enemy.yaw = 0.0; // Facing -Z

        let actors = [
            CombatantInfo {
                id: ActorId::Player,
                faction: Faction::PlayerSide,
                pos: Vec3::new(0.0, 0.0, -10.0),
                active: true,
                sprinting: false,
            },
            CombatantInfo {
                id: ActorId::Ally(1),
                faction: Faction::PlayerSide,
                pos: Vec3::new(0.0, 0.0, -4.0),
                active: true,
                sprinting: false,
            },
        ];

        // Without threat memory, selects the nearer visible Ally(1) at 4m over Player at 10m
        let (chosen, _) = select_enemy_target(&enemy, &actors, &grid).expect("should find target");
        assert_eq!(chosen, ActorId::Ally(1));

        // When damaged by Player, 3.0s threat memory overrides distance and selects Player
        enemy.apply_hit_from(10.0, Vec3::new(0.0, 0.0, -10.0), ActorId::Player);
        let (threat_chosen, _) =
            select_enemy_target(&enemy, &actors, &grid).expect("should find threat target");
        assert_eq!(threat_chosen, ActorId::Player);
    }
}
