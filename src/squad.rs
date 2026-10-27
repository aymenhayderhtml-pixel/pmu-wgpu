use crate::ally::{
    Ally, AllyState, Rarity, AGGRESSIVE_AGGRO_RANGE, ALLY_AGGRO_RANGE, ALLY_ATTACK_RANGE,
    ALLY_ATTACK_WINDUP, ALLY_CAST_STOP_TIME, ALLY_CATCHUP_SPEED, ALLY_FIREBALL_MAX_DIST,
    ALLY_FIREBALL_MIN_DIST, ALLY_HEAL_PULSE_AMOUNT, ALLY_HEAL_THRESHOLD_RATIO, ALLY_HEIGHT,
    ALLY_RADIUS, ALLY_RUN_SPEED, ALLY_WALK_SPEED, EPIC_FIREBALL_COOLDOWN, EPIC_HEAL_COOLDOWN,
    FOLLOW_DISTANCE, HOLD_DEFEND_RANGE, MAX_ALLIES, RARE_FIREBALL_COOLDOWN, REGROUP_DISTANCE,
    REGROUP_SAFE_DIST, REGROUP_TELEPORT_TIME, REVIVE_HOLD_TIME, REVIVE_RANGE, SLOT_STOP_RADIUS,
    STUCK_TIME_THRESHOLD, SUMMON_COST, SUMMON_SPAWN_OFFSET, TARGET_STICKY_TIME,
};
use crate::camera::lerp_angle;
use crate::collision::CollisionGrid;
use crate::combat::{compute_separation_force, is_in_attack_arc, ActorId, CombatantInfo};
use crate::enemy::{
    Enemy, EnemyState, REPATH_INTERVAL, REPATH_TARGET_DRIFT, SEPARATION_RADIUS, SEPARATION_WEIGHT,
};
use crate::nav::{NavGrid, NavSearchScratch};
use crate::world::GROUND_SIZE;
use glam::{Vec2, Vec3};
use std::fmt::Write;
use std::time::Instant;

pub const MAX_SHARED_ASTAR_BUDGET: usize = 3;
pub const STATUS_MESSAGE_DURATION: f32 = 2.0;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SquadMode {
    Follow,
    Hold,
    Aggressive,
}

impl SquadMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Follow => "FOLLOW",
            Self::Hold => "HOLD",
            Self::Aggressive => "AGGRESSIVE",
        }
    }
}

/// Round-robin A* pathfinding budget scheduler shared fairly between all allies and enemies.
pub struct AstarBudgetScheduler {
    pub remaining_this_step: usize,
    next_cursor: usize,
}

impl AstarBudgetScheduler {
    pub fn new() -> Self {
        Self {
            remaining_this_step: MAX_SHARED_ASTAR_BUDGET,
            next_cursor: 0,
        }
    }

    pub fn begin_step(&mut self) {
        self.remaining_this_step = MAX_SHARED_ASTAR_BUDGET;
    }

    /// Selects up to `MAX_SHARED_ASTAR_BUDGET` request indices in round-robin order across `requests`.
    pub fn grant_round_robin(&mut self, requests: &[bool], out_granted: &mut [bool]) {
        let n = requests.len().min(out_granted.len());
        for flag in &mut out_granted[..n] {
            *flag = false;
        }
        if n == 0 || self.remaining_this_step == 0 {
            return;
        }

        let start = self.next_cursor % n;
        for offset in 0..n {
            if self.remaining_this_step == 0 {
                break;
            }
            let idx = (start + offset) % n;
            if requests[idx] {
                out_granted[idx] = true;
                self.remaining_this_step -= 1;
                self.next_cursor = (idx + 1) % n;
            }
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct AllyAttackHit {
    pub ally_id: u32,
    pub enemy_id: u32,
    pub damage: f32,
    pub attacker_pos: Vec3,
}

impl Default for AllyAttackHit {
    fn default() -> Self {
        Self {
            ally_id: 0,
            enemy_id: 0,
            damage: 0.0,
            attacker_pos: Vec3::ZERO,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum AllySkillCast {
    Fireball { origin_xz: Vec2, dir_xz: Vec2 },
    HealPulse { caster_pos: Vec3, amount: f32 },
}

pub struct SquadUpdateOutput {
    pub hits: [AllyAttackHit; MAX_ALLIES],
    pub hit_count: usize,
    pub skill_casts: [Option<AllySkillCast>; MAX_ALLIES],
    pub skill_cast_count: usize,
    pub revived_ally_pos_and_heal: Option<(Vec3, u32)>,
    pub astar_calls: u32,
    pub astar_micros: u64,
}

/// Computes the world-space formation slot for `slot_index` (`0..4`) on a circle of radius
/// `FOLLOW_DISTANCE` (3.0m) behind and beside the player relative to `player_yaw`.
pub fn compute_formation_slot(player_pos: Vec3, player_yaw: f32, slot_index: usize) -> Vec3 {
    let local_angles_deg: [f32; 4] = [135.0, -135.0, 90.0, -90.0];
    let offset_rad = local_angles_deg[slot_index % 4].to_radians();
    let world_angle = player_yaw + offset_rad;
    let dir = Vec3::new(-world_angle.sin(), 0.0, -world_angle.cos());
    player_pos + dir * FOLLOW_DISTANCE
}

/// Selects the best visible enemy target for an ally, preferring enemies attacking the player
/// or an ally and respecting the 1.5s target stickiness window.
pub fn select_ally_target(
    ally_pos: Vec3,
    anchor_pos: Vec3,
    aggro_range: f32,
    current_target: Option<u32>,
    target_lock_timer: f32,
    enemies: &[Enemy],
    collision: &CollisionGrid,
) -> Option<u32> {
    if let Some(cur_id) = current_target {
        if target_lock_timer < TARGET_STICKY_TIME {
            if let Some(e) = enemies.iter().find(|e| e.id == cur_id && e.is_alive()) {
                let d_ally = Vec2::new(e.pos.x - ally_pos.x, e.pos.z - ally_pos.z).length();
                let d_anchor = Vec2::new(e.pos.x - anchor_pos.x, e.pos.z - anchor_pos.z).length();
                if d_ally <= aggro_range + 2.0 || d_anchor <= aggro_range + 2.0 {
                    return Some(cur_id);
                }
            }
        }
    }

    let mut best_attacker: Option<(u32, f32)> = None;
    let mut best_other: Option<(u32, f32)> = None;

    for e in enemies {
        if !e.is_alive() {
            continue;
        }
        let d_ally = Vec2::new(e.pos.x - ally_pos.x, e.pos.z - ally_pos.z).length();
        let d_anchor = Vec2::new(e.pos.x - anchor_pos.x, e.pos.z - anchor_pos.z).length();
        if d_ally > aggro_range && d_anchor > aggro_range {
            continue;
        }

        let visible = d_ally <= 3.0
            || collision.has_line_of_sight(
                ally_pos + Vec3::new(0.0, 1.3, 0.0),
                e.pos + Vec3::new(0.0, 1.1, 0.0),
            );
        if !visible {
            continue;
        }

        let is_attacker = e.state == EnemyState::Attack || e.state == EnemyState::Chase;
        if is_attacker {
            if best_attacker.map_or(true, |(_, best_d)| d_ally < best_d) {
                best_attacker = Some((e.id, d_ally));
            }
        } else if best_other.map_or(true, |(_, best_d)| d_ally < best_d) {
            best_other = Some((e.id, d_ally));
        }
    }

    best_attacker.or(best_other).map(|(id, _)| id)
}

pub struct Squad {
    pub allies: Vec<Ally>,
    pub mode: SquadMode,
    pub rng_state: u64,
    next_ally_id: u32,
    name_cursors: [usize; 3],
    pub status_message: String,
    pub status_color: [f32; 3],
    pub status_timer: f32,
    pub total_ally_casts: u32,
}

impl Squad {
    pub fn new(seed: u64) -> Self {
        Self {
            allies: Vec::with_capacity(MAX_ALLIES),
            mode: SquadMode::Follow,
            rng_state: seed ^ 0x55AA_AA55_1234_5678,
            next_ally_id: 1,
            name_cursors: [0; 3],
            status_message: String::with_capacity(64),
            status_color: [1.0, 0.95, 0.45],
            status_timer: 0.0,
            total_ally_casts: 0,
        }
    }

    pub fn reset(&mut self, seed: u64) {
        self.allies.clear();
        self.mode = SquadMode::Follow;
        self.rng_state = seed ^ 0x55AA_AA55_1234_5678;
        self.next_ally_id = 1;
        self.name_cursors = [0; 3];
        self.status_message.clear();
        self.status_color = [1.0, 0.95, 0.45];
        self.status_timer = 0.0;
        self.total_ally_casts = 0;
    }

    pub fn living_count(&self) -> u32 {
        self.allies.iter().filter(|a| a.is_living()).count() as u32
    }

    pub fn state_counts(&self) -> [u32; 6] {
        let mut counts = [0u32; 6];
        for a in &self.allies {
            match a.state {
                AllyState::Follow => counts[0] += 1,
                AllyState::Engage => counts[1] += 1,
                AllyState::Attack => counts[2] += 1,
                AllyState::Recover => counts[3] += 1,
                AllyState::Regroup => counts[4] += 1,
                AllyState::Down => counts[5] += 1,
            }
        }
        counts
    }

    pub fn set_mode(&mut self, new_mode: SquadMode) {
        self.mode = new_mode;
        if new_mode == SquadMode::Hold {
            for ally in &mut self.allies {
                ally.hold_pos = ally.pos;
                ally.path.clear();
            }
        }
        println!("Squad command mode set: {}", new_mode.label());
    }

    pub fn set_status_message(&mut self, msg: &str) {
        self.set_status_message_colored(msg, [1.0, 0.95, 0.45]);
    }

    pub fn set_status_message_colored(&mut self, msg: &str, color: [f32; 3]) {
        self.status_message.clear();
        self.status_message.push_str(msg);
        self.status_color = color;
        self.status_timer = STATUS_MESSAGE_DURATION;
    }

    pub fn try_summon(
        &mut self,
        score: &mut u32,
        player_pos: Vec3,
        player_yaw: f32,
        collision: &CollisionGrid,
        skeleton_node_count: usize,
        initial_clip: usize,
    ) -> bool {
        if self.allies.len() >= MAX_ALLIES {
            self.set_status_message("Squad full");
            return false;
        }
        if *score < SUMMON_COST {
            self.set_status_message("Not enough score");
            return false;
        }

        *score -= SUMMON_COST;
        let rarity = Rarity::roll(&mut self.rng_state);
        let name = self.pick_unique_name(rarity);

        let forward = Vec3::new(-player_yaw.sin(), 0.0, -player_yaw.cos());
        let mut spawn_pos = player_pos + forward * SUMMON_SPAWN_OFFSET;
        spawn_pos.y = 0.0;
        let mut dummy_vel = Vec3::ZERO;
        collision.resolve_player_xz(
            &mut spawn_pos,
            &mut dummy_vel,
            ALLY_RADIUS,
            ALLY_HEIGHT,
            GROUND_SIZE * 0.5,
        );

        let id = self.next_ally_id;
        self.next_ally_id = self.next_ally_id.wrapping_add(1);
        let ally = Ally::new(
            id,
            name,
            rarity,
            spawn_pos,
            player_yaw,
            skeleton_node_count,
            initial_clip,
        );
        self.allies.push(ally);

        self.status_message.clear();
        let _ = write!(
            &mut self.status_message,
            "Summoned: {} ({})",
            name,
            rarity.label()
        );
        self.status_color = rarity.tint();
        self.status_timer = STATUS_MESSAGE_DURATION;
        println!("{}", self.status_message);
        true
    }

    fn pick_unique_name(&mut self, rarity: Rarity) -> &'static str {
        let pool = rarity.name_pool();
        let r_idx = match rarity {
            Rarity::Common => 0,
            Rarity::Rare => 1,
            Rarity::Epic => 2,
        };
        for _ in 0..pool.len() {
            let candidate = pool[self.name_cursors[r_idx] % pool.len()];
            self.name_cursors[r_idx] += 1;
            if !self.allies.iter().any(|a| a.name == candidate) {
                return candidate;
            }
        }
        pool[0]
    }

    /// Revives all `Down` allies with 50% health at the start of a new wave.
    pub fn revive_all_down_for_new_wave(&mut self) -> Vec<(Vec3, u32)> {
        let mut revived = Vec::new();
        for ally in &mut self.allies {
            if ally.is_down() {
                let heal = ally.revive_half_health();
                revived.push((ally.pos, heal));
            }
        }
        revived
    }

    /// Checks which allies need an A* path calculation this step.
    pub fn collect_astar_requests(
        &self,
        player_pos: Vec3,
        enemies: &[Enemy],
        collision: &CollisionGrid,
        nav: &NavGrid,
        out_requests: &mut [bool; MAX_ALLIES],
    ) {
        *out_requests = [false; MAX_ALLIES];
        for (i, ally) in self.allies.iter().enumerate() {
            if ally.is_down() || ally.repath_timer > 0.0 {
                continue;
            }
            let goal_pos = match ally.state {
                AllyState::Engage => enemies
                    .iter()
                    .find(|e| Some(e.id) == ally.target_enemy_id && e.is_alive())
                    .map(|e| e.pos)
                    .unwrap_or(ally.slot_pos),
                AllyState::Follow => {
                    if self.mode == SquadMode::Hold {
                        ally.hold_pos
                    } else {
                        ally.slot_pos
                    }
                }
                AllyState::Regroup => player_pos,
                _ => continue,
            };

            let ally_xz = Vec2::new(ally.pos.x, ally.pos.z);
            let goal_xz = Vec2::new(goal_pos.x, goal_pos.z);
            if ally_xz.distance(goal_xz) <= 1.2 {
                continue;
            }

            let los = collision.has_line_of_sight(
                ally.pos + Vec3::new(0.0, 1.1, 0.0),
                goal_pos + Vec3::new(0.0, 1.1, 0.0),
            ) && nav.has_walkable_line_of_sight(ally_xz, goal_xz);

            if !los {
                let drifted = ally.path_goal.distance(goal_xz) > REPATH_TARGET_DRIFT;
                if ally.path.is_empty() || ally.path_index >= ally.path.len() || drifted {
                    out_requests[i] = true;
                }
            }
        }
    }

    pub fn update(
        &mut self,
        dt: f32,
        player_pos: Vec3,
        player_yaw: f32,
        player_hp_ratio: f32,
        holding_revive_key: bool,
        enemies: &[Enemy],
        actors: &[CombatantInfo],
        astar_granted: &[bool; MAX_ALLIES],
        collision: &CollisionGrid,
        nav: &NavGrid,
        nav_scratch: &mut NavSearchScratch,
    ) -> SquadUpdateOutput {
        let mut out = SquadUpdateOutput {
            hits: [AllyAttackHit::default(); MAX_ALLIES],
            hit_count: 0,
            skill_casts: [None; MAX_ALLIES],
            skill_cast_count: 0,
            revived_ally_pos_and_heal: None,
            astar_calls: 0,
            astar_micros: 0,
        };

        let any_squad_member_below_half = player_hp_ratio < ALLY_HEAL_THRESHOLD_RATIO
            || self
                .allies
                .iter()
                .any(|a| a.is_living() && (a.health / a.max_health.max(1.0)) < ALLY_HEAL_THRESHOLD_RATIO);

        if self.status_timer > 0.0 {
            self.status_timer = (self.status_timer - dt).max(0.0);
        }

        // Handle E-key revive on the nearest Down ally within REVIVE_RANGE (2.5m)
        let mut revived_name: Option<&'static str> = None;
        let nearest_down_idx = self
            .allies
            .iter()
            .enumerate()
            .filter(|(_, a)| a.is_down())
            .map(|(i, a)| (i, Vec2::new(a.pos.x - player_pos.x, a.pos.z - player_pos.z).length()))
            .filter(|(_, d)| *d <= REVIVE_RANGE)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i);

        for (i, ally) in self.allies.iter_mut().enumerate() {
            if ally.is_down() {
                ally.down_timer += dt;
                if Some(i) == nearest_down_idx && holding_revive_key {
                    ally.revive_progress += dt;
                    if ally.revive_progress >= REVIVE_HOLD_TIME {
                        let heal = ally.revive_half_health();
                        out.revived_ally_pos_and_heal = Some((ally.pos, heal));
                        revived_name = Some(ally.name);
                    }
                } else {
                    ally.revive_progress = 0.0;
                }
            }
        }

        if let Some(name) = revived_name {
            self.status_message.clear();
            let _ = write!(&mut self.status_message, "{} was revived", name);
            self.status_timer = STATUS_MESSAGE_DURATION;
        }

        let player_xz = Vec2::new(player_pos.x, player_pos.z);

        for (idx, ally) in self.allies.iter_mut().enumerate() {
            ally.prev_pos = ally.pos;
            ally.prev_yaw = ally.yaw;
            ally.hit_flash_timer = (ally.hit_flash_timer - dt).max(0.0);
            ally.spawn_fx_timer = (ally.spawn_fx_timer - dt).max(0.0);
            ally.fireball_cd = (ally.fireball_cd - dt).max(0.0);
            ally.heal_cd = (ally.heal_cd - dt).max(0.0);
            ally.repath_timer = (ally.repath_timer - dt).max(0.0);
            ally.target_lock_timer += dt;

            if ally.is_down() {
                let mut dummy_v = Vec3::ZERO;
                collision.resolve_player_xz(
                    &mut ally.pos,
                    &mut dummy_v,
                    ALLY_RADIUS,
                    ALLY_HEIGHT,
                    GROUND_SIZE * 0.5,
                );
                continue;
            }

            ally.slot_pos = compute_formation_slot(player_pos, player_yaw, idx);
            let ally_xz = Vec2::new(ally.pos.x, ally.pos.z);
            let dist_to_player = ally_xz.distance(player_xz);

            let (aggro_range, anchor_pos) = match self.mode {
                SquadMode::Follow => (ALLY_AGGRO_RANGE, player_pos),
                SquadMode::Hold => (HOLD_DEFEND_RANGE, ally.hold_pos),
                SquadMode::Aggressive => (AGGRESSIVE_AGGRO_RANGE, ally.pos),
            };

            let new_target = select_ally_target(
                ally.pos,
                anchor_pos,
                aggro_range,
                ally.target_enemy_id,
                ally.target_lock_timer,
                enemies,
                collision,
            );
            if new_target != ally.target_enemy_id {
                ally.target_enemy_id = new_target;
                ally.target_lock_timer = 0.0;
                ally.path.clear();
            }

            // Check regroup condition (far from player or stuck > 2s, except in Hold mode)
            if self.mode != SquadMode::Hold
                && ally.state != AllyState::Regroup
                && (dist_to_player > REGROUP_DISTANCE || ally.stuck_timer >= STUCK_TIME_THRESHOLD)
            {
                ally.state = AllyState::Regroup;
                ally.regroup_timer = 0.0;
                ally.target_enemy_id = None;
                ally.path.clear();
            }

            let sep = compute_separation_force(
                ActorId::Ally(ally.id),
                ally_xz,
                actors,
                SEPARATION_RADIUS,
            );
            let spd_mult = ally.rarity.speed_mult();
            let mut desired_vel_xz = Vec2::ZERO;
            let mut trying_to_move = false;

            // Rare and Epic ally skill casting in Engage and Attack states
            if (ally.state == AllyState::Engage || ally.state == AllyState::Attack)
                && ally.rarity != Rarity::Common
            {
                let mut casted = false;
                // Epic Heal Pulse takes priority when any living squad member is below 50% HP
                if ally.rarity == Rarity::Epic && ally.heal_cd <= 0.0 && any_squad_member_below_half
                {
                    ally.heal_cd = EPIC_HEAL_COOLDOWN;
                    ally.state = AllyState::Recover;
                    ally.state_timer = ALLY_CAST_STOP_TIME;
                    ally.vel = Vec3::ZERO;
                    if out.skill_cast_count < MAX_ALLIES {
                        out.skill_casts[out.skill_cast_count] = Some(AllySkillCast::HealPulse {
                            caster_pos: ally.pos,
                            amount: ALLY_HEAL_PULSE_AMOUNT,
                        });
                        out.skill_cast_count += 1;
                    }
                    self.total_ally_casts += 1;
                    casted = true;
                }

                if !casted && ally.fireball_cd <= 0.0 {
                    if let Some(enemy) = enemies
                        .iter()
                        .find(|e| Some(e.id) == ally.target_enemy_id && e.is_alive())
                    {
                        let to_e = Vec2::new(enemy.pos.x - ally.pos.x, enemy.pos.z - ally.pos.z);
                        let dist = to_e.length();
                        if (ALLY_FIREBALL_MIN_DIST..=ALLY_FIREBALL_MAX_DIST).contains(&dist)
                            && collision.has_line_of_sight(
                                ally.pos + Vec3::new(0.0, 1.2, 0.0),
                                enemy.pos + Vec3::new(0.0, 1.1, 0.0),
                            )
                        {
                            let dir = to_e / dist;
                            ally.yaw = f32::atan2(-dir.x, -dir.y);
                            ally.fireball_cd = match ally.rarity {
                                Rarity::Epic => EPIC_FIREBALL_COOLDOWN,
                                _ => RARE_FIREBALL_COOLDOWN,
                            };
                            ally.state = AllyState::Recover;
                            ally.state_timer = ALLY_CAST_STOP_TIME;
                            ally.vel = Vec3::ZERO;
                            if out.skill_cast_count < MAX_ALLIES {
                                out.skill_casts[out.skill_cast_count] =
                                    Some(AllySkillCast::Fireball {
                                        origin_xz: ally_xz + dir * 0.6,
                                        dir_xz: dir,
                                    });
                                out.skill_cast_count += 1;
                            }
                            self.total_ally_casts += 1;
                        }
                    }
                }
            }

            match ally.state {
                AllyState::Regroup => {
                    ally.regroup_timer += dt;
                    if dist_to_player <= REGROUP_SAFE_DIST && ally.stuck_timer < 0.5 {
                        ally.state = AllyState::Follow;
                        ally.regroup_timer = 0.0;
                    } else if ally.regroup_timer >= REGROUP_TELEPORT_TIME
                        && dist_to_player > REGROUP_SAFE_DIST
                    {
                        // Teleport next to player at formation slot with flash
                        let mut tp = ally.slot_pos;
                        tp.y = 0.0;
                        let mut dv = Vec3::ZERO;
                        collision.resolve_player_xz(
                            &mut tp,
                            &mut dv,
                            ALLY_RADIUS,
                            ALLY_HEIGHT,
                            GROUND_SIZE * 0.5,
                        );
                        ally.pos = tp;
                        ally.prev_pos = tp;
                        ally.vel = Vec3::ZERO;
                        ally.hit_flash_timer = 0.25;
                        ally.spawn_fx_timer = 0.25;
                        ally.regroup_timer = 0.0;
                        ally.stuck_timer = 0.0;
                        ally.state = AllyState::Follow;
                    } else {
                        trying_to_move = true;
                        let run_spd = ALLY_WALK_SPEED * 1.5 * spd_mult;
                        let steer_pt = Self::steer_toward_point(
                            ally,
                            Vec2::new(ally.slot_pos.x, ally.slot_pos.z),
                            astar_granted[idx],
                            collision,
                            nav,
                            nav_scratch,
                            &mut out,
                        );
                        let seek = (steer_pt - ally_xz).normalize_or_zero();
                        desired_vel_xz = seek * run_spd + sep * SEPARATION_WEIGHT;
                        if desired_vel_xz.length() > run_spd {
                            desired_vel_xz = desired_vel_xz.normalize() * run_spd;
                        }
                    }
                }
                AllyState::Follow => {
                    if ally.target_enemy_id.is_some() {
                        ally.state = AllyState::Engage;
                    } else {
                        let home_3d = if self.mode == SquadMode::Hold {
                            ally.hold_pos
                        } else {
                            ally.slot_pos
                        };
                        let home_xz = Vec2::new(home_3d.x, home_3d.z);
                        let dist_to_slot = ally_xz.distance(home_xz);

                        if dist_to_slot > SLOT_STOP_RADIUS {
                            trying_to_move = true;
                            let base_spd = if dist_to_slot > 10.0 {
                                ALLY_CATCHUP_SPEED
                            } else if dist_to_slot > 6.0 {
                                ALLY_RUN_SPEED
                            } else {
                                ALLY_WALK_SPEED
                            };
                            let move_spd = base_spd * spd_mult;
                            let steer_pt = Self::steer_toward_point(
                                ally,
                                home_xz,
                                astar_granted[idx],
                                collision,
                                nav,
                                nav_scratch,
                                &mut out,
                            );
                            let seek = (steer_pt - ally_xz).normalize_or_zero();
                            desired_vel_xz = seek * move_spd + sep * SEPARATION_WEIGHT;
                            if desired_vel_xz.length() > move_spd {
                                desired_vel_xz = desired_vel_xz.normalize() * move_spd;
                            }
                        } else {
                            desired_vel_xz = sep * (SEPARATION_WEIGHT * 0.5);
                        }
                    }
                }
                AllyState::Engage => {
                    let target_enemy = enemies
                        .iter()
                        .find(|e| Some(e.id) == ally.target_enemy_id && e.is_alive());
                    if let Some(enemy) = target_enemy {
                        let enemy_xz = Vec2::new(enemy.pos.x, enemy.pos.z);
                        let dist_to_enemy = ally_xz.distance(enemy_xz);
                        let has_los = collision.has_line_of_sight(
                            ally.pos + Vec3::new(0.0, 1.2, 0.0),
                            enemy.pos + Vec3::new(0.0, 1.1, 0.0),
                        );

                        if dist_to_enemy <= ALLY_ATTACK_RANGE && has_los {
                            ally.state = AllyState::Attack;
                            ally.state_timer = ALLY_ATTACK_WINDUP;
                            ally.attack_dealt = false;
                            ally.vel = Vec3::ZERO;
                        } else {
                            trying_to_move = true;
                            let engage_spd = ALLY_RUN_SPEED * spd_mult;
                            let steer_pt = Self::steer_toward_point(
                                ally,
                                enemy_xz,
                                astar_granted[idx],
                                collision,
                                nav,
                                nav_scratch,
                                &mut out,
                            );
                            let seek = (steer_pt - ally_xz).normalize_or_zero();
                            desired_vel_xz = seek * engage_spd + sep * SEPARATION_WEIGHT;
                            if desired_vel_xz.length() > engage_spd {
                                desired_vel_xz = desired_vel_xz.normalize() * engage_spd;
                            }
                        }
                    } else {
                        ally.target_enemy_id = None;
                        ally.state = AllyState::Follow;
                    }
                }
                AllyState::Attack => {
                    let target_enemy = enemies
                        .iter()
                        .find(|e| Some(e.id) == ally.target_enemy_id && e.is_alive());
                    if let Some(enemy) = target_enemy {
                        let to_e = Vec2::new(enemy.pos.x - ally.pos.x, enemy.pos.z - ally.pos.z);
                        if to_e.length_squared() > 1e-5 {
                            let target_yaw = f32::atan2(-to_e.x, -to_e.y);
                            ally.yaw = lerp_angle(ally.yaw, target_yaw, (16.0 * dt).min(1.0));
                        }
                    }

                    ally.state_timer -= dt;
                    if ally.state_timer <= 0.0 {
                        if !ally.attack_dealt {
                            if let Some(enemy) = target_enemy {
                                if is_in_attack_arc(
                                    ally.pos,
                                    ally.yaw,
                                    enemy.pos,
                                    ALLY_ATTACK_RANGE + 0.45,
                                    std::f32::consts::PI,
                                ) && out.hit_count < MAX_ALLIES
                                {
                                    out.hits[out.hit_count] = AllyAttackHit {
                                        ally_id: ally.id,
                                        enemy_id: enemy.id,
                                        damage: ally.rarity.damage(),
                                        attacker_pos: ally.pos,
                                    };
                                    out.hit_count += 1;
                                }
                            }
                            ally.attack_dealt = true;
                        }
                        ally.state = AllyState::Recover;
                        ally.state_timer = ally.rarity.attack_cooldown();
                    }
                }
                AllyState::Recover => {
                    ally.state_timer -= dt;
                    if ally.state_timer <= 0.0 {
                        if ally.target_enemy_id.is_some() {
                            ally.state = AllyState::Engage;
                        } else {
                            ally.state = AllyState::Follow;
                        }
                    }
                }
                AllyState::Down => {}
            }

            ally.vel.x = desired_vel_xz.x;
            ally.vel.z = desired_vel_xz.y;
            if desired_vel_xz.length_squared() > 0.05 {
                let target_yaw = f32::atan2(-desired_vel_xz.x, -desired_vel_xz.y);
                let turn_alpha = 1.0 - (-12.0 * dt).exp();
                ally.yaw = lerp_angle(ally.yaw, target_yaw, turn_alpha);
            }

            let before_xz = Vec2::new(ally.pos.x, ally.pos.z);
            ally.pos.x += ally.vel.x * dt;
            ally.pos.z += ally.vel.z * dt;
            collision.resolve_player_xz(
                &mut ally.pos,
                &mut ally.vel,
                ALLY_RADIUS,
                ALLY_HEIGHT,
                GROUND_SIZE * 0.5,
            );
            let moved_dist = Vec2::new(ally.pos.x, ally.pos.z).distance(before_xz);
            if trying_to_move && moved_dist < 0.25 * dt {
                ally.stuck_timer += dt;
            } else if moved_dist >= 0.25 * dt {
                ally.stuck_timer = (ally.stuck_timer - dt * 2.0).max(0.0);
            }
        }

        out
    }

    fn steer_toward_point(
        ally: &mut Ally,
        goal_xz: Vec2,
        can_run_astar: bool,
        collision: &CollisionGrid,
        nav: &NavGrid,
        nav_scratch: &mut NavSearchScratch,
        out: &mut SquadUpdateOutput,
    ) -> Vec2 {
        let ally_xz = Vec2::new(ally.pos.x, ally.pos.z);
        let direct_los = collision.has_line_of_sight(
            ally.pos + Vec3::new(0.0, 1.1, 0.0),
            Vec3::new(goal_xz.x, 1.1, goal_xz.y),
        ) && nav.has_walkable_line_of_sight(ally_xz, goal_xz);

        if direct_los {
            ally.path.clear();
            return goal_xz;
        }

        if can_run_astar && ally.repath_timer <= 0.0 {
            ally.repath_timer = REPATH_INTERVAL;
            ally.path_goal = goal_xz;
            ally.path_index = 0;
            let t0 = Instant::now();
            let _ = nav.find_path(ally_xz, goal_xz, nav_scratch, &mut ally.path);
            out.astar_calls += 1;
            out.astar_micros += t0.elapsed().as_micros() as u64;
        }

        while ally.path_index < ally.path.len()
            && ally_xz.distance(ally.path[ally.path_index]) < 0.65
        {
            ally.path_index += 1;
        }

        ally.path.get(ally.path_index).copied().unwrap_or(goal_xz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summon_cost_squad_full_and_failure_no_spend() {
        let grid = CollisionGrid::new(Vec::new());
        let mut squad = Squad::new(12345);
        let mut score = 20u32;

        // Insufficient score (< 30) fails and does not spend score
        assert!(!squad.try_summon(&mut score, Vec3::ZERO, 0.0, &grid, 1, 0));
        assert_eq!(score, 20);
        assert_eq!(squad.status_message, "Not enough score");

        // Summon 4 allies with sufficient score
        score = 150;
        for _ in 0..MAX_ALLIES {
            assert!(squad.try_summon(&mut score, Vec3::ZERO, 0.0, &grid, 1, 0));
        }
        assert_eq!(squad.allies.len(), MAX_ALLIES);
        assert_eq!(score, 30);

        // 5th summon fails with "Squad full" and does not spend the remaining 30 score
        assert!(!squad.try_summon(&mut score, Vec3::ZERO, 0.0, &grid, 1, 0));
        assert_eq!(score, 30);
        assert_eq!(squad.status_message, "Squad full");
    }

    #[test]
    fn test_formation_slots_distinct_and_rotate_with_player() {
        let p = Vec3::new(2.0, 0.0, -3.0);
        let slots_yaw0: Vec<Vec3> = (0..4).map(|i| compute_formation_slot(p, 0.0, i)).collect();
        for i in 0..4 {
            assert!((slots_yaw0[i].distance(p) - FOLLOW_DISTANCE).abs() < 1e-4);
            for j in (i + 1)..4 {
                assert!(slots_yaw0[i].distance(slots_yaw0[j]) > 1.0);
            }
        }

        let slots_yaw90: Vec<Vec3> = (0..4)
            .map(|i| compute_formation_slot(p, std::f32::consts::FRAC_PI_2, i))
            .collect();
        assert!(slots_yaw0[0].distance(slots_yaw90[0]) > 2.0);
    }

    #[test]
    fn test_ally_target_selection_nearest_attacker_preference_and_stickiness() {
        let grid = CollisionGrid::new(Vec::new());
        let mut e_passive_near = Enemy::new(1, Vec3::new(0.0, 0.0, -4.0), 1, 1, 0);
        e_passive_near.state = EnemyState::Wander;

        let mut e_attacker_far = Enemy::new(2, Vec3::new(0.0, 0.0, -8.0), 2, 1, 0);
        e_attacker_far.state = EnemyState::Attack;

        let enemies = vec![e_passive_near, e_attacker_far];

        // Prefers enemy 2 (attacker) over closer passive enemy 1
        let chosen = select_ally_target(
            Vec3::ZERO,
            Vec3::ZERO,
            ALLY_AGGRO_RANGE,
            None,
            2.0,
            &enemies,
            &grid,
        );
        assert_eq!(chosen, Some(2));

        // Stickiness: if locked onto enemy 1 with lock_timer = 0.5s (< 1.5s), keeps enemy 1
        let sticky = select_ally_target(
            Vec3::ZERO,
            Vec3::ZERO,
            ALLY_AGGRO_RANGE,
            Some(1),
            0.5,
            &enemies,
            &grid,
        );
        assert_eq!(sticky, Some(1));
    }

    #[test]
    fn test_ally_down_and_revive_hold_interrupt_proximity_and_wave_start() {
        let grid = CollisionGrid::new(Vec::new());
        let nav = NavGrid::new(&[]);
        let mut nav_scratch = NavSearchScratch::new();
        let mut squad = Squad::new(99);
        let mut score = 60;
        assert!(squad.try_summon(&mut score, Vec3::ZERO, 0.0, &grid, 1, 0));

        // Damage ally to 0 HP -> becomes Down
        squad.allies[0].apply_damage(500.0);
        assert!(squad.allies[0].is_down());
        let ally_pos = squad.allies[0].pos;

        let granted = [false; MAX_ALLIES];
        let dt = 0.5;

        // Hold E for 1.0s within range -> progress increases to 1.0s
        for _ in 0..2 {
            squad.update(dt, ally_pos, 0.0, 1.0, true, &[], &[], &granted, &grid, &nav, &mut nav_scratch);
        }
        assert!(squad.allies[0].is_down());
        assert!((squad.allies[0].revive_progress - 1.0).abs() < 1e-4);

        // Release E -> progress resets to 0.0
        squad.update(dt, ally_pos, 0.0, 1.0, false, &[], &[], &granted, &grid, &nav, &mut nav_scratch);
        assert_eq!(squad.allies[0].revive_progress, 0.0);

        // Hold E for full 2.0s within range -> revives with 50% health
        for _ in 0..4 {
            squad.update(dt, ally_pos, 0.0, 1.0, true, &[], &[], &granted, &grid, &nav, &mut nav_scratch);
        }
        assert!(squad.allies[0].is_living());
        assert!((squad.allies[0].health - squad.allies[0].max_health * 0.5).abs() < 1e-3);

        // Down again and test wave-start auto-revive
        squad.allies[0].apply_damage(500.0);
        assert!(squad.allies[0].is_down());
        let revived = squad.revive_all_down_for_new_wave();
        assert_eq!(revived.len(), 1);
        assert!(squad.allies[0].is_living());
    }

    #[test]
    fn test_regroup_teleport_after_stuck_or_distance() {
        let grid = CollisionGrid::new(Vec::new());
        let nav = NavGrid::new(&[]);
        let mut nav_scratch = NavSearchScratch::new();
        let mut squad = Squad::new(77);
        let mut score = 60;
        squad.try_summon(&mut score, Vec3::ZERO, 0.0, &grid, 1, 0);

        // Place ally 26m away (> REGROUP_DISTANCE = 22m) and force Regroup state
        squad.allies[0].pos = Vec3::new(26.0, 0.0, 0.0);
        squad.allies[0].state = AllyState::Regroup;
        squad.allies[0].regroup_timer = 5.95;

        let granted = [false; MAX_ALLIES];
        squad.update(0.1, Vec3::ZERO, 0.0, 1.0, false, &[], &[], &granted, &grid, &nav, &mut nav_scratch);
        // Teleported next to player (~3.0m formation slot)
        assert!(squad.allies[0].pos.distance(Vec3::ZERO) <= FOLLOW_DISTANCE + 0.5);
        assert_eq!(squad.allies[0].state, AllyState::Follow);
    }

    #[test]
    fn test_hold_and_aggressive_modes() {
        let grid = CollisionGrid::new(Vec::new());
        let nav = NavGrid::new(&[]);
        let mut nav_scratch = NavSearchScratch::new();
        let mut squad = Squad::new(55);
        let mut score = 60;
        squad.try_summon(&mut score, Vec3::ZERO, 0.0, &grid, 1, 0);
        squad.allies[0].pos = Vec3::new(10.0, 0.0, 10.0);

        // Set Hold mode -> hold_pos locked to (10, 0, 10)
        squad.set_mode(SquadMode::Hold);
        assert_eq!(squad.allies[0].hold_pos, Vec3::new(10.0, 0.0, 10.0));

        // Displace ally to (12, 0, 10) with no enemies -> walks back toward hold_pos (10, 0, 10)
        squad.allies[0].pos = Vec3::new(12.0, 0.0, 10.0);
        let granted = [false; MAX_ALLIES];
        for _ in 0..60 {
            squad.update(1.0 / 60.0, Vec3::ZERO, 0.0, 1.0, false, &[], &[], &granted, &grid, &nav, &mut nav_scratch);
        }
        assert!(squad.allies[0].pos.distance(Vec3::new(10.0, 0.0, 10.0)) < 1.0);

        // Aggressive mode targets an enemy 25m away (> 14m normal aggro, <= 30m aggressive aggro)
        squad.set_mode(SquadMode::Aggressive);
        let far_enemy = Enemy::new(9, Vec3::new(10.0, 0.0, -14.0), 1, 1, 0);
        let enemies = vec![far_enemy];
        squad.update(1.0 / 60.0, Vec3::ZERO, 0.0, 1.0, false, &enemies, &[], &granted, &grid, &nav, &mut nav_scratch);
        assert_eq!(squad.allies[0].target_enemy_id, Some(9));
    }

    #[test]
    fn test_ally_skill_rules_common_rare_and_epic_heal_priority() {
        let grid = CollisionGrid::new(Vec::new());
        let nav = NavGrid::new(&[]);
        let mut nav_scratch = NavSearchScratch::new();
        let granted = [false; MAX_ALLIES];
        let enemies = vec![Enemy::new(1, Vec3::new(0.0, 0.0, -8.0), 1, 1, 0)];

        let mut squad = Squad::new(101);
        squad.allies.push(Ally::new(1, "CommonOne", Rarity::Common, Vec3::ZERO, 0.0, 1, 0));
        squad.allies[0].state = AllyState::Engage;
        squad.allies[0].target_enemy_id = Some(1);

        // Common ally never casts skills
        let out_c = squad.update(1.0 / 60.0, Vec3::ZERO, 0.0, 0.3, false, &enemies, &[], &granted, &grid, &nav, &mut nav_scratch);
        assert_eq!(out_c.skill_cast_count, 0);

        // Rare ally casts Fireball when target is between 4m and 14m with LOS
        squad.allies[0].rarity = Rarity::Rare;
        squad.allies[0].state = AllyState::Engage;
        let out_r = squad.update(1.0 / 60.0, Vec3::ZERO, 0.0, 1.0, false, &enemies, &[], &granted, &grid, &nav, &mut nav_scratch);
        assert_eq!(out_r.skill_cast_count, 1);
        assert!(matches!(out_r.skill_casts[0], Some(AllySkillCast::Fireball { .. })));
        assert!((squad.allies[0].fireball_cd - RARE_FIREBALL_COOLDOWN).abs() < 1e-4);
        assert_eq!(squad.allies[0].state, AllyState::Recover);

        // Epic ally prioritizes Heal Pulse over Fireball when player is below 50% HP (player_hp_ratio = 0.4)
        squad.allies[0].rarity = Rarity::Epic;
        squad.allies[0].state = AllyState::Engage;
        squad.allies[0].fireball_cd = 0.0;
        squad.allies[0].heal_cd = 0.0;
        let out_e = squad.update(1.0 / 60.0, Vec3::ZERO, 0.0, 0.4, false, &enemies, &[], &granted, &grid, &nav, &mut nav_scratch);
        assert_eq!(out_e.skill_cast_count, 1);
        assert!(matches!(out_e.skill_casts[0], Some(AllySkillCast::HealPulse { .. })));
        assert!((squad.allies[0].heal_cd - EPIC_HEAL_COOLDOWN).abs() < 1e-4);
        assert_eq!(squad.allies[0].fireball_cd, 0.0);
    }

    #[test]
    fn test_shared_astar_budget_round_robin_fairness() {
        let mut sched = AstarBudgetScheduler::new();
        // 10 enemies + 4 allies = 14 actors all requesting A* simultaneously
        let requests = [true; 14];
        let mut granted = [false; 14];
        let mut served_counts = [0u32; 14];

        for _ in 0..5 {
            sched.begin_step();
            sched.grant_round_robin(&requests, &mut granted);
            let step_total: usize = granted.iter().filter(|&&b| b).count();
            assert_eq!(step_total, MAX_SHARED_ASTAR_BUDGET);
            for i in 0..14 {
                if granted[i] {
                    served_counts[i] += 1;
                }
            }
        }

        // Across 5 steps (15 total grants for 14 actors), every single actor was served at least once!
        for &c in &served_counts {
            assert!(c >= 1);
        }
    }
}
