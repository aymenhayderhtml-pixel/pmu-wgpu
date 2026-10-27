use crate::animation::{
    compute_hierarchy_and_joints, sample_clip_into, AnimationClip, PoseScratch, Skeleton,
};
use crate::camera::lerp_angle;
use crate::combat::HIT_FLASH_DURATION;
use crate::enemy::DEATH_FALL_DURATION;
use crate::player_anim::{AnimMode, ClipMap};
use glam::{Mat4, Quat, Vec2, Vec3};

pub const MAX_ALLIES: usize = 4;
pub const SUMMON_COST: u32 = 30;
pub const SUMMON_SPAWN_OFFSET: f32 = 2.5;
pub const ALLY_SPAWN_FX_DURATION: f32 = 0.60;

pub const ALLY_RADIUS: f32 = 0.45;
pub const ALLY_HEIGHT: f32 = 2.0;

pub const FOLLOW_DISTANCE: f32 = 3.0;
pub const SLOT_STOP_RADIUS: f32 = 0.6;
pub const ALLY_AGGRO_RANGE: f32 = 14.0;
pub const AGGRESSIVE_AGGRO_RANGE: f32 = 30.0;
pub const HOLD_DEFEND_RANGE: f32 = 6.0;

pub const ALLY_ATTACK_RANGE: f32 = 1.8;
pub const ALLY_ATTACK_WINDUP: f32 = 0.30;

pub const REGROUP_DISTANCE: f32 = 22.0;
pub const STUCK_TIME_THRESHOLD: f32 = 2.0;
pub const REGROUP_TELEPORT_TIME: f32 = 6.0;
pub const REGROUP_SAFE_DIST: f32 = 12.0;

pub const ALLY_WALK_SPEED: f32 = 4.0;
pub const ALLY_RUN_SPEED: f32 = 7.0;
pub const ALLY_CATCHUP_SPEED: f32 = 8.0;

pub const TARGET_STICKY_TIME: f32 = 1.5;
pub const REVIVE_HOLD_TIME: f32 = 2.0;
pub const REVIVE_RANGE: f32 = 2.5;

pub const RARE_FIREBALL_COOLDOWN: f32 = 6.0;
pub const EPIC_FIREBALL_COOLDOWN: f32 = 5.0;
pub const EPIC_HEAL_COOLDOWN: f32 = 10.0;
pub const ALLY_FIREBALL_MIN_DIST: f32 = 4.0;
pub const ALLY_FIREBALL_MAX_DIST: f32 = 14.0;
pub const ALLY_HEAL_PULSE_AMOUNT: f32 = 20.0;
pub const ALLY_HEAL_PULSE_RADIUS: f32 = 8.0;
pub const ALLY_HEAL_THRESHOLD_RATIO: f32 = 0.50;
pub const ALLY_CAST_STOP_TIME: f32 = 0.30;

pub const COMMON_NAMES: [&str; 6] = ["Rowan", "Pip", "Bram", "Kael", "Finn", "Toma"];
pub const RARE_NAMES: [&str; 6] = ["Lyra", "Vane", "Soren", "Freya", "Caelum", "Nyx"];
pub const EPIC_NAMES: [&str; 6] = ["Aurelia", "Zephyr", "Valerius", "Seraphina", "Orion", "Isolde"];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Rarity {
    Common,
    Rare,
    Epic,
}

impl Rarity {
    pub fn roll(rng_state: &mut u64) -> Self {
        let r = next_unit_f32(rng_state);
        if r < 0.70 {
            Self::Common
        } else if r < 0.95 {
            Self::Rare
        } else {
            Self::Epic
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Common => "Common",
            Self::Rare => "Rare",
            Self::Epic => "Epic",
        }
    }

    pub fn max_health(self) -> f32 {
        match self {
            Self::Common => 60.0,
            Self::Rare => 90.0,
            Self::Epic => 140.0,
        }
    }

    pub fn damage(self) -> f32 {
        match self {
            Self::Common => 8.0,
            Self::Rare => 12.0,
            Self::Epic => 20.0,
        }
    }

    pub fn speed_mult(self) -> f32 {
        match self {
            Self::Common => 1.0,
            Self::Rare => 1.1,
            Self::Epic => 1.2,
        }
    }

    pub fn attack_cooldown(self) -> f32 {
        match self {
            Self::Common => 0.90,
            Self::Rare => 0.75,
            Self::Epic => 0.60,
        }
    }

    pub fn tint(self) -> [f32; 3] {
        match self {
            Self::Common => [0.45, 0.96, 0.52], // Light green
            Self::Rare => [0.32, 0.68, 1.00],   // Blue
            Self::Epic => [0.80, 0.36, 1.00],   // Purple
        }
    }

    pub fn name_pool(self) -> &'static [&'static str; 6] {
        match self {
            Self::Common => &COMMON_NAMES,
            Self::Rare => &RARE_NAMES,
            Self::Epic => &EPIC_NAMES,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AllyState {
    Follow,
    Engage,
    Attack,
    Recover,
    Regroup,
    Down,
}

impl AllyState {
    pub fn hud_label(self) -> &'static str {
        match self {
            Self::Follow => "FOLLOW",
            Self::Engage => "FIGHT",
            Self::Attack => "ATTACK",
            Self::Recover => "READY",
            Self::Regroup => "REGROUP",
            Self::Down => "DOWN",
        }
    }
}

pub struct Ally {
    pub id: u32,
    pub name: &'static str,
    pub rarity: Rarity,
    pub prev_pos: Vec3,
    pub pos: Vec3,
    pub vel: Vec3,
    pub prev_yaw: f32,
    pub yaw: f32,
    pub health: f32,
    pub max_health: f32,
    pub state: AllyState,
    pub state_timer: f32,
    pub attack_dealt: bool,
    pub target_enemy_id: Option<u32>,
    pub target_lock_timer: f32,
    pub slot_pos: Vec3,
    pub hold_pos: Vec3,
    pub stuck_timer: f32,
    pub regroup_timer: f32,
    pub down_timer: f32,
    pub revive_progress: f32,
    pub spawn_fx_timer: f32,
    pub hit_flash_timer: f32,
    pub fireball_cd: f32,
    pub heal_cd: f32,
    pub repath_timer: f32,
    pub path: Vec<Vec2>,
    pub path_index: usize,
    pub path_goal: Vec2,
    // Animation state
    pub active_clip: usize,
    pub active_time: f32,
    pub prev_clip: Option<(usize, f32)>,
    pub fade_timer: f32,
    pub proc_phase: f32,
    pub pose_scratch: PoseScratch,
}

impl Ally {
    pub fn new(
        id: u32,
        name: &'static str,
        rarity: Rarity,
        spawn_pos: Vec3,
        yaw: f32,
        node_count: usize,
        initial_clip: usize,
    ) -> Self {
        let max_hp = rarity.max_health();
        Self {
            id,
            name,
            rarity,
            prev_pos: spawn_pos,
            pos: spawn_pos,
            vel: Vec3::ZERO,
            prev_yaw: yaw,
            yaw,
            health: max_hp,
            max_health: max_hp,
            state: AllyState::Follow,
            state_timer: 0.0,
            attack_dealt: false,
            target_enemy_id: None,
            target_lock_timer: 0.0,
            slot_pos: spawn_pos,
            hold_pos: spawn_pos,
            stuck_timer: 0.0,
            regroup_timer: 0.0,
            down_timer: 0.0,
            revive_progress: 0.0,
            spawn_fx_timer: ALLY_SPAWN_FX_DURATION,
            hit_flash_timer: 0.0,
            fireball_cd: 0.0,
            heal_cd: 0.0,
            repath_timer: 0.0,
            path: Vec::with_capacity(64),
            path_index: 0,
            path_goal: Vec2::new(spawn_pos.x, spawn_pos.z),
            active_clip: initial_clip,
            active_time: 0.0,
            prev_clip: None,
            fade_timer: 0.0,
            proc_phase: 0.0,
            pose_scratch: PoseScratch::new(node_count),
        }
    }

    pub fn is_living(&self) -> bool {
        self.state != AllyState::Down
    }

    pub fn is_down(&self) -> bool {
        self.state == AllyState::Down
    }

    pub fn apply_damage(&mut self, amount: f32) -> bool {
        if self.is_down() || amount <= 0.0 {
            return false;
        }
        self.health = (self.health - amount).max(0.0);
        self.hit_flash_timer = HIT_FLASH_DURATION;
        if self.health <= 0.0 {
            self.state = AllyState::Down;
            self.vel = Vec3::ZERO;
            self.target_enemy_id = None;
            self.down_timer = 0.0;
            self.revive_progress = 0.0;
            self.path.clear();
        }
        true
    }

    pub fn revive_half_health(&mut self) -> u32 {
        let heal = (self.max_health * 0.5).max(1.0);
        self.health = heal;
        self.state = AllyState::Follow;
        self.down_timer = 0.0;
        self.revive_progress = 0.0;
        self.stuck_timer = 0.0;
        self.regroup_timer = 0.0;
        self.spawn_fx_timer = 0.25;
        heal.round() as u32
    }

    pub fn interpolated_pos(&self, alpha: f32) -> Vec3 {
        self.prev_pos.lerp(self.pos, alpha.clamp(0.0, 1.0))
    }

    pub fn interpolated_yaw(&self, alpha: f32) -> f32 {
        lerp_angle(self.prev_yaw, self.yaw, alpha.clamp(0.0, 1.0))
    }

    pub fn tint_with_flash(&self) -> [f32; 4] {
        if self.is_down() {
            return [0.42, 0.44, 0.46, 1.0];
        }
        let base = self.rarity.tint();
        let hit_f = (self.hit_flash_timer / HIT_FLASH_DURATION).clamp(0.0, 1.0);
        let spawn_f = (self.spawn_fx_timer / ALLY_SPAWN_FX_DURATION).clamp(0.0, 1.0);
        let flash = hit_f.max(spawn_f);
        [base[0], base[1], base[2], 1.0 + flash]
    }

    pub fn update_animation(
        &mut self,
        dt_real: f32,
        skeleton: Option<&Skeleton>,
        clips: &[AnimationClip],
        clip_map: &ClipMap,
    ) {
        let dt = dt_real.clamp(0.0, 0.10);
        let spd = Vec2::new(self.vel.x, self.vel.z).length();

        let target_clip = match self.state {
            AllyState::Down | AllyState::Recover => clip_map.idle,
            AllyState::Attack | AllyState::Engage | AllyState::Regroup => clip_map.run,
            AllyState::Follow => {
                if spd > 5.0 {
                    clip_map.run
                } else if spd > 0.3 {
                    clip_map.walk
                } else {
                    clip_map.idle
                }
            }
        }
        .unwrap_or(0);

        if target_clip != self.active_clip && target_clip < clips.len() {
            self.prev_clip = Some((self.active_clip, self.active_time));
            self.active_clip = target_clip;
            self.active_time = 0.0;
            self.fade_timer = 0.0;
        }

        let rate = match self.state {
            AllyState::Down => 0.0,
            AllyState::Attack => 1.60,
            AllyState::Regroup => 1.35,
            AllyState::Engage => 1.15,
            AllyState::Follow if spd > 0.3 => (spd / ALLY_WALK_SPEED).clamp(0.6, 1.6),
            _ => 1.0,
        };

        self.active_time += dt * rate;
        if self.is_living() && spd > 0.3 {
            self.proc_phase = (self.proc_phase + dt * 9.5 * rate).rem_euclid(std::f32::consts::TAU);
        } else {
            self.proc_phase = 0.0;
        }

        if let Some((_, ref mut pt)) = self.prev_clip {
            *pt += dt * rate;
            self.fade_timer += dt;
            if self.fade_timer >= 0.20 {
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
                let alpha = (self.fade_timer / 0.20).clamp(0.0, 1.0);
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

        let spawn_scale = if self.spawn_fx_timer > 0.0 {
            let t = 1.0 - (self.spawn_fx_timer / ALLY_SPAWN_FX_DURATION).clamp(0.0, 1.0);
            let ease_out = 1.0 - (1.0 - t) * (1.0 - t);
            0.2 + 0.8 * ease_out
        } else {
            1.0
        };

        let mut pitch_lean = 0.0_f32;
        let mut roll_fall = 0.0_f32;
        let mut forward_lunge = 0.0_f32;

        match self.state {
            AllyState::Attack => {
                let progress =
                    1.0 - (self.state_timer / ALLY_ATTACK_WINDUP.max(1e-4)).clamp(0.0, 1.0);
                let pulse = (progress * std::f32::consts::PI).sin();
                pitch_lean = -0.26 * pulse;
                forward_lunge = 0.32 * pulse;
            }
            AllyState::Down => {
                let fall_t = (self.down_timer / DEATH_FALL_DURATION).clamp(0.0, 1.0);
                roll_fall = fall_t * std::f32::consts::FRAC_PI_2;
            }
            _ => {
                if mode == AnimMode::Procedural {
                    let spd = Vec2::new(self.vel.x, self.vel.z).length();
                    if spd > 0.25 {
                        pos.y += self.proc_phase.sin().abs() * 0.05;
                        pitch_lean = -(spd / ALLY_RUN_SPEED).clamp(0.0, 1.0) * 0.14;
                    }
                }
            }
        }

        let facing_rot = Quat::from_rotation_y(yaw);
        let local_pose_rot = Quat::from_rotation_z(roll_fall) * Quat::from_rotation_x(pitch_lean);
        let lunge_offset =
            Quat::from_rotation_y(self.interpolated_yaw(alpha)) * Vec3::new(0.0, 0.0, -forward_lunge);

        Mat4::from_scale_rotation_translation(
            Vec3::splat(spawn_scale),
            facing_rot * local_pose_rot,
            pos + lunge_offset,
        )
    }
}

pub fn next_unit_f32(state: &mut u64) -> f32 {
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

    #[test]
    fn test_rarity_roll_fixed_seed_and_distribution() {
        let mut seed = 0x504D_555F_3230_3236;
        let first_five = [
            Rarity::roll(&mut seed),
            Rarity::roll(&mut seed),
            Rarity::roll(&mut seed),
            Rarity::roll(&mut seed),
            Rarity::roll(&mut seed),
        ];
        assert_eq!(
            first_five,
            [
                Rarity::Rare,
                Rarity::Common,
                Rarity::Common,
                Rarity::Common,
                Rarity::Rare
            ]
        );

        let mut dist_seed = 0x504D_555F_3230_3236;
        let mut common = 0u32;
        let mut rare = 0u32;
        let mut epic = 0u32;
        for _ in 0..10_000 {
            match Rarity::roll(&mut dist_seed) {
                Rarity::Common => common += 1,
                Rarity::Rare => rare += 1,
                Rarity::Epic => epic += 1,
            }
        }
        assert!((common as f32 / 10_000.0 - 0.70).abs() < 0.03);
        assert!((rare as f32 / 10_000.0 - 0.25).abs() < 0.03);
        assert!((epic as f32 / 10_000.0 - 0.05).abs() < 0.03);
    }
}
