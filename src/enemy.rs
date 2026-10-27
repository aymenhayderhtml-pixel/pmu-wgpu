use crate::animation::{
    compute_hierarchy_and_joints, sample_clip_into, AnimationClip, PoseScratch, Skeleton,
};
use crate::camera::lerp_angle;
use crate::combat::{ActorId, HIT_FLASH_DURATION};
use crate::player_anim::{AnimMode, ClipMap};
use crate::status::StatusEffects;
use glam::{Mat4, Quat, Vec2, Vec3};

pub const ENEMY_MAX_HEALTH: f32 = 50.0;
pub const ENEMY_RADIUS: f32 = 0.5;
pub const ENEMY_HEIGHT: f32 = 2.0;

pub const SIGHT_RANGE: f32 = 18.0;
pub const SIGHT_HALF_ANGLE_RAD: f32 = 70.0_f32.to_radians();
pub const HEARING_RANGE: f32 = 6.0;
pub const LOSE_INTEREST_RANGE: f32 = 28.0;
pub const LOSE_INTEREST_TIME: f32 = 4.0;
pub const THREAT_MEMORY_TIME: f32 = 3.0;

pub const ATTACK_RANGE: f32 = 1.8;
pub const ATTACK_WINDUP: f32 = 0.35;
pub const ATTACK_DAMAGE: f32 = 10.0;
pub const ATTACK_COOLDOWN: f32 = 1.1;

pub const WANDER_SPEED: f32 = 1.6;
pub const CHASE_SPEED: f32 = 4.2;
pub const HIT_STAGGER_TIME: f32 = 0.30;
pub const KNOCKBACK_SPEED: f32 = 6.0;

pub const SEPARATION_RADIUS: f32 = 1.2;
pub const SEPARATION_WEIGHT: f32 = 3.5;
pub const REPATH_INTERVAL: f32 = 0.50;
pub const REPATH_TARGET_DRIFT: f32 = 3.0;
pub const MAX_ASTAR_PER_STEP: usize = 2;

pub const DEATH_FALL_DURATION: f32 = 0.50;
pub const DEATH_SINK_DURATION: f32 = 1.50;
pub const DEATH_TOTAL_DURATION: f32 = DEATH_FALL_DURATION + DEATH_SINK_DURATION;

pub const ENEMY_TINT: [f32; 3] = [1.0, 0.38, 0.35];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EnemyState {
    Wander,
    Chase,
    Attack,
    Recover,
    Dead,
}

pub struct Enemy {
    pub id: u32,
    pub prev_pos: Vec3,
    pub pos: Vec3,
    pub vel: Vec3,
    pub prev_yaw: f32,
    pub yaw: f32,
    pub health: f32,
    pub max_health: f32,
    pub state: EnemyState,
    pub state_timer: f32,
    pub was_staggered: bool,
    pub attack_dealt: bool,
    pub target_actor: ActorId,
    pub target_pos: Vec3,
    pub last_attacker: Option<ActorId>,
    pub threat_timer: f32,
    pub time_since_seen: f32,
    pub repath_timer: f32,
    pub path: Vec<Vec2>,
    pub path_index: usize,
    pub path_goal: Vec2,
    pub hit_flash_timer: f32,
    pub death_timer: f32,
    pub status: StatusEffects,
    pub spawn_seed: u64,
    pub wander_target: Vec2,
    pub wander_timer: f32,
    pub active_clip: usize,
    pub active_time: f32,
    pub prev_clip: Option<(usize, f32)>,
    pub fade_timer: f32,
    pub proc_phase: f32,
    pub pose_scratch: PoseScratch,
}

impl Enemy {
    pub fn new(
        id: u32,
        spawn_pos: Vec3,
        spawn_seed: u64,
        node_count: usize,
        initial_clip: usize,
    ) -> Self {
        Self {
            id,
            prev_pos: spawn_pos,
            pos: spawn_pos,
            vel: Vec3::ZERO,
            prev_yaw: 0.0,
            yaw: 0.0,
            health: ENEMY_MAX_HEALTH,
            max_health: ENEMY_MAX_HEALTH,
            state: EnemyState::Wander,
            state_timer: 0.0,
            was_staggered: false,
            attack_dealt: false,
            target_actor: ActorId::Player,
            target_pos: spawn_pos,
            last_attacker: None,
            threat_timer: 0.0,
            time_since_seen: 0.0,
            repath_timer: 0.0,
            path: Vec::with_capacity(64),
            path_index: 0,
            path_goal: Vec2::new(spawn_pos.x, spawn_pos.z),
            hit_flash_timer: 0.0,
            death_timer: 0.0,
            status: StatusEffects::default(),
            spawn_seed,
            wander_target: Vec2::new(spawn_pos.x, spawn_pos.z),
            wander_timer: 0.0,
            active_clip: initial_clip,
            active_time: 0.0,
            prev_clip: None,
            fade_timer: 0.0,
            proc_phase: 0.0,
            pose_scratch: PoseScratch::new(node_count),
        }
    }

    pub fn is_alive(&self) -> bool {
        self.state != EnemyState::Dead
    }

    pub fn is_removable(&self) -> bool {
        self.state == EnemyState::Dead && self.death_timer >= DEATH_TOTAL_DURATION
    }

    pub fn apply_hit(&mut self, damage: f32, attacker_pos: Vec3) -> bool {
        self.apply_hit_from(damage, attacker_pos, ActorId::Player)
    }

    pub fn apply_hit_from(
        &mut self,
        damage: f32,
        attacker_pos: Vec3,
        attacker_id: ActorId,
    ) -> bool {
        if !self.is_alive() {
            return false;
        }

        self.health = (self.health - damage).max(0.0);
        self.hit_flash_timer = HIT_FLASH_DURATION;
        self.last_attacker = Some(attacker_id);
        self.threat_timer = THREAT_MEMORY_TIME;
        self.target_actor = attacker_id;

        let away = Vec2::new(self.pos.x - attacker_pos.x, self.pos.z - attacker_pos.z);
        let dir = if away.length_squared() > 1e-5 {
            away.normalize()
        } else {
            Vec2::new(0.0, 1.0)
        };
        self.vel.x = dir.x * KNOCKBACK_SPEED;
        self.vel.z = dir.y * KNOCKBACK_SPEED;

        if self.health <= 0.0 {
            self.state = EnemyState::Dead;
            self.state_timer = 0.0;
            self.death_timer = 0.0;
            self.vel = Vec3::ZERO;
            true
        } else {
            self.state = EnemyState::Recover;
            self.state_timer = HIT_STAGGER_TIME;
            self.was_staggered = true;
            false
        }
    }

    pub fn interpolated_pos(&self, alpha: f32) -> Vec3 {
        self.prev_pos.lerp(self.pos, alpha.clamp(0.0, 1.0))
    }

    pub fn interpolated_yaw(&self, alpha: f32) -> f32 {
        lerp_angle(self.prev_yaw, self.yaw, alpha.clamp(0.0, 1.0))
    }

    pub fn tint_with_flash(&self) -> [f32; 4] {
        let flash = (self.hit_flash_timer / HIT_FLASH_DURATION).clamp(0.0, 1.0);
        let tinted = self.status.mix_tint(ENEMY_TINT);
        [tinted[0], tinted[1], tinted[2], 1.0 + flash]
    }

    pub fn update_animation(
        &mut self,
        dt_real: f32,
        skeleton: Option<&Skeleton>,
        clips: &[AnimationClip],
        clip_map: &ClipMap,
    ) {
        let dt = dt_real.clamp(0.0, 0.10);

        let target_clip = match self.state {
            EnemyState::Wander => clip_map.walk,
            EnemyState::Chase | EnemyState::Attack => clip_map.run,
            EnemyState::Recover | EnemyState::Dead => clip_map.idle,
        }
        .unwrap_or(0);

        if target_clip != self.active_clip && target_clip < clips.len() {
            self.prev_clip = Some((self.active_clip, self.active_time));
            self.active_clip = target_clip;
            self.active_time = 0.0;
            self.fade_timer = 0.0;
        }

        let base_rate = match self.state {
            EnemyState::Wander => 0.85,
            EnemyState::Chase => 1.15,
            EnemyState::Attack => 1.60,
            EnemyState::Recover => 1.0,
            EnemyState::Dead => 0.0,
        };
        let rate = base_rate * self.status.speed_multiplier();

        self.active_time += dt * rate;
        if self.state == EnemyState::Wander || self.state == EnemyState::Chase {
            self.proc_phase = (self.proc_phase + dt * 9.0 * rate).rem_euclid(std::f32::consts::TAU);
        } else {
            self.proc_phase = 0.0;
        }

        if let Some((_, ref mut pt)) = self.prev_clip {
            *pt += dt * rate;
            self.fade_timer += dt;
            if self.fade_timer >= 0.18 {
                self.prev_clip = None;
                self.fade_timer = 0.0;
            }
        }

        let (Some(skel), Some(clip_curr)) = (skeleton, clips.get(self.active_clip)) else {
            return;
        };

        self.pose_scratch.ensure_capacity(skel.nodes.len());
        sample_clip_into(
            skel,
            clip_curr,
            self.active_time,
            true,
            &mut self.pose_scratch.local_a,
        );

        if let Some((prev_idx, prev_time)) = self.prev_clip {
            if let Some(clip_prev) = clips.get(prev_idx) {
                sample_clip_into(
                    skel,
                    clip_prev,
                    prev_time,
                    true,
                    &mut self.pose_scratch.local_b,
                );
                let alpha = (self.fade_timer / 0.18).clamp(0.0, 1.0);
                for i in 0..skel.nodes.len() {
                    self.pose_scratch.local_a[i] = self.pose_scratch.local_b[i]
                        .blend(self.pose_scratch.local_a[i], alpha);
                }
            }
        }

        compute_hierarchy_and_joints(skel, &mut self.pose_scratch);
    }

    pub fn world_matrix(&self, alpha: f32, yaw_offset: f32, mode: AnimMode) -> Mat4 {
        let mut pos = self.interpolated_pos(alpha);
        let yaw = self.interpolated_yaw(alpha) + yaw_offset;

        let mut pitch_lean = 0.0_f32;
        let mut roll_fall = 0.0_f32;
        let mut forward_lunge = 0.0_f32;

        match self.state {
            EnemyState::Attack => {
                let progress =
                    1.0 - (self.state_timer / ATTACK_WINDUP.max(1e-4)).clamp(0.0, 1.0);
                let pulse = (progress * std::f32::consts::PI).sin();
                pitch_lean = -0.28 * pulse;
                forward_lunge = 0.35 * pulse;
            }
            EnemyState::Recover if self.was_staggered => {
                let ratio = (self.state_timer / HIT_STAGGER_TIME.max(1e-4)).clamp(0.0, 1.0);
                pitch_lean = 0.32 * ratio;
            }
            EnemyState::Dead => {
                let fall_t = (self.death_timer / DEATH_FALL_DURATION).clamp(0.0, 1.0);
                roll_fall = fall_t * std::f32::consts::FRAC_PI_2;
                if self.death_timer > DEATH_FALL_DURATION {
                    let sink_t = ((self.death_timer - DEATH_FALL_DURATION) / DEATH_SINK_DURATION)
                        .clamp(0.0, 1.0);
                    pos.y -= sink_t * 1.6;
                }
            }
            _ => {
                if mode == AnimMode::Procedural {
                    let spd = Vec2::new(self.vel.x, self.vel.z).length();
                    if spd > 0.2 {
                        pos.y += self.proc_phase.sin().abs() * 0.05;
                        pitch_lean = -(spd / CHASE_SPEED).clamp(0.0, 1.0) * 0.14;
                    }
                }
            }
        }

        let facing_rot = Quat::from_rotation_y(yaw);
        let local_pose_rot = Quat::from_rotation_z(roll_fall) * Quat::from_rotation_x(pitch_lean);
        let lunge_offset =
            Quat::from_rotation_y(self.interpolated_yaw(alpha)) * Vec3::new(0.0, 0.0, -forward_lunge);

        Mat4::from_scale_rotation_translation(
            Vec3::ONE,
            facing_rot * local_pose_rot,
            pos + lunge_offset,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enemy_damage_knockback_and_death() {
        let mut enemy = Enemy::new(1, Vec3::new(0.0, 0.0, -3.0), 123, 1, 0);
        let killed = enemy.apply_hit_from(25.0, Vec3::ZERO, ActorId::Ally(2));
        assert!(!killed);
        assert_eq!(enemy.health, 25.0);
        assert_eq!(enemy.state, EnemyState::Recover);
        assert_eq!(enemy.last_attacker, Some(ActorId::Ally(2)));
        assert!((enemy.threat_timer - THREAT_MEMORY_TIME).abs() < 1e-5);
        assert!(enemy.vel.z < -5.0);

        let killed2 = enemy.apply_hit(25.0, Vec3::ZERO);
        assert!(killed2);
        assert_eq!(enemy.state, EnemyState::Dead);
        assert_eq!(enemy.health, 0.0);
    }
}
