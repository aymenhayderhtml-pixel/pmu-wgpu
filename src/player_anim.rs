use crate::animation::{
    compute_hierarchy_and_joints, sample_clip_into, AnimationClip, PoseScratch, Skeleton,
};
use glam::{Mat4, Quat, Vec3};

pub const CROSSFADE_DURATION: f32 = 0.20;
pub const WALK_SPEED_THRESHOLD: f32 = 0.25;
pub const RUN_SPEED_THRESHOLD: f32 = 6.2;
pub const WALK_REFERENCE_SPEED: f32 = 5.0;
pub const RUN_REFERENCE_SPEED: f32 = 8.5;

pub const PROC_BOB_FREQ: f32 = 10.5;
pub const PROC_BOB_AMP: f32 = 0.065;
pub const PROC_LEAN_MAX_RAD: f32 = 0.16;
pub const PROC_JUMP_STRETCH: f32 = 0.12;
pub const PROC_LAND_SQUASH: f32 = 0.18;
pub const PROC_SQUASH_DECAY: f32 = 8.0;
pub const PROC_ATTACK_LEAN_RAD: f32 = 0.34;
pub const PROC_ATTACK_DASH_DIST: f32 = 0.32;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AnimState {
    Idle,
    Walk,
    Run,
    Jump,
    Fall,
}

impl AnimState {
    pub fn is_looping(self) -> bool {
        !matches!(self, Self::Jump)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Walk => "Walk",
            Self::Run => "Run",
            Self::Jump => "Jump",
            Self::Fall => "Fall",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AnimMode {
    Skeletal,
    Procedural,
}

impl AnimMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Skeletal => "skeletal",
            Self::Procedural => "procedural",
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ClipMap {
    pub idle: Option<usize>,
    pub walk: Option<usize>,
    pub run: Option<usize>,
    pub jump: Option<usize>,
    pub fall: Option<usize>,
}

impl ClipMap {
    pub fn from_clips(clips: &[AnimationClip]) -> Self {
        let find_sub = |sub: &str| -> Option<usize> {
            clips
                .iter()
                .position(|c| c.name.to_ascii_lowercase().contains(sub))
        };

        let raw_idle = find_sub("idle").or_else(|| if clips.is_empty() { None } else { Some(0) });
        let raw_walk = find_sub("walk");
        let raw_run = find_sub("run").or_else(|| find_sub("sprint"));
        let raw_jump = find_sub("jump");
        let raw_fall = find_sub("fall").or_else(|| find_sub("air"));

        let idle = raw_idle.or(raw_walk).or(raw_run);
        let walk = raw_walk.or(idle);
        let run = raw_run.or(walk).or(idle);
        let jump = raw_jump.or(idle);
        let fall = raw_fall.or(jump).or(idle);

        Self {
            idle,
            walk,
            run,
            jump,
            fall,
        }
    }

    pub fn clip_for_state(&self, state: AnimState) -> Option<usize> {
        match state {
            AnimState::Idle => self.idle,
            AnimState::Walk => self.walk,
            AnimState::Run => self.run,
            AnimState::Jump => self.jump,
            AnimState::Fall => self.fall,
        }
    }
}

pub fn select_state(horiz_speed: f32, grounded: bool, vel_y: f32) -> AnimState {
    if !grounded {
        if vel_y > 0.25 {
            AnimState::Jump
        } else {
            AnimState::Fall
        }
    } else if horiz_speed >= RUN_SPEED_THRESHOLD {
        AnimState::Run
    } else if horiz_speed >= WALK_SPEED_THRESHOLD {
        AnimState::Walk
    } else {
        AnimState::Idle
    }
}

pub struct PlayerAnimator {
    mode: AnimMode,
    skeleton: Option<Skeleton>,
    clips: Vec<AnimationClip>,
    clip_map: ClipMap,
    state: AnimState,
    active_clip: usize,
    active_time: f32,
    prev_clip: Option<(usize, f32, bool)>,
    fade_timer: f32,
    manual_clip_override: Option<usize>,
    scratch: PoseScratch,
    proc_phase: f32,
    proc_squash: f32,
    was_grounded: bool,
}

impl PlayerAnimator {
    pub fn new(skeleton: Option<Skeleton>, clips: Vec<AnimationClip>) -> Self {
        let has_skeletal = skeleton
            .as_ref()
            .map_or(false, |s| !s.joint_nodes.is_empty())
            && !clips.is_empty();
        let mode = if has_skeletal {
            AnimMode::Skeletal
        } else {
            AnimMode::Procedural
        };
        println!("PMU Player Animation Mode: {}", mode.label());

        let node_count = skeleton.as_ref().map_or(0, |s| s.nodes.len());
        let clip_map = ClipMap::from_clips(&clips);
        let initial_clip = clip_map.idle.unwrap_or(0);

        let mut anim = Self {
            mode,
            skeleton,
            clips,
            clip_map,
            state: AnimState::Idle,
            active_clip: initial_clip,
            active_time: 0.0,
            prev_clip: None,
            fade_timer: 0.0,
            manual_clip_override: None,
            scratch: PoseScratch::new(node_count),
            proc_phase: 0.0,
            proc_squash: 0.0,
            was_grounded: true,
        };
        anim.evaluate_current_pose();
        anim
    }

    pub fn mode(&self) -> AnimMode {
        self.mode
    }

    pub fn joint_count(&self) -> u32 {
        self.skeleton
            .as_ref()
            .map_or(0, |s| s.joint_count() as u32)
    }

    pub fn current_clip_name(&self) -> &str {
        if self.mode == AnimMode::Procedural {
            return self.state.label();
        }
        self.clips
            .get(self.active_clip)
            .map(|c| c.name.as_str())
            .unwrap_or("none")
    }

    pub fn current_clip_time(&self) -> f32 {
        if self.mode == AnimMode::Procedural {
            self.proc_phase
        } else {
            self.active_time
        }
    }

    pub fn is_manual_override(&self) -> bool {
        self.manual_clip_override.is_some()
    }

    pub fn joint_matrices(&self) -> &[[[f32; 4]; 4]] {
        &self.scratch.joint_matrices
    }

    pub fn bone_segments(&self) -> &[(Vec3, Vec3)] {
        &self.scratch.bone_segments
    }

    pub fn cycle_manual_clip(&mut self) {
        if self.clips.is_empty() {
            println!("Animation clip cycle (F6): no glTF clips loaded (procedural mode active)");
            return;
        }

        match self.manual_clip_override {
            None => {
                self.manual_clip_override = Some(0);
                self.transition_to_clip(0);
                println!(
                    "Animation override (F6): Manual clip [0/{}] '{}'",
                    self.clips.len(),
                    self.clips[0].name
                );
            }
            Some(idx) if idx + 1 < self.clips.len() => {
                let next = idx + 1;
                self.manual_clip_override = Some(next);
                self.transition_to_clip(next);
                println!(
                    "Animation override (F6): Manual clip [{}/{}] '{}'",
                    next,
                    self.clips.len(),
                    self.clips[next].name
                );
            }
            Some(_) => {
                self.manual_clip_override = None;
                if let Some(auto_clip) = self.clip_map.clip_for_state(self.state) {
                    self.transition_to_clip(auto_clip);
                }
                println!("Animation override (F6): Returned to Automatic state-machine mode");
            }
        }
    }

    pub fn update(&mut self, dt_real: f32, horiz_speed: f32, grounded: bool, vel_y: f32) {
        let dt = dt_real.clamp(0.0, 0.10);

        if grounded && !self.was_grounded {
            self.proc_squash = 1.0;
        }
        self.was_grounded = grounded;
        self.proc_squash = (self.proc_squash - PROC_SQUASH_DECAY * dt).max(0.0);

        let desired_state = select_state(horiz_speed, grounded, vel_y);
        if desired_state != self.state {
            self.state = desired_state;
            if self.manual_clip_override.is_none() {
                if let Some(target_clip) = self.clip_map.clip_for_state(desired_state) {
                    self.transition_to_clip(target_clip);
                }
            }
        }

        if self.mode == AnimMode::Procedural {
            if grounded && horiz_speed > WALK_SPEED_THRESHOLD {
                let rate = (horiz_speed / WALK_REFERENCE_SPEED).clamp(0.4, 2.2);
                self.proc_phase =
                    (self.proc_phase + dt * PROC_BOB_FREQ * rate).rem_euclid(std::f32::consts::TAU);
            } else {
                self.proc_phase = 0.0;
            }
            return;
        }

        let playback_rate = if self.manual_clip_override.is_some() {
            1.0
        } else {
            match self.state {
                AnimState::Walk => (horiz_speed / WALK_REFERENCE_SPEED).clamp(0.5, 1.8),
                AnimState::Run => (horiz_speed / RUN_REFERENCE_SPEED).clamp(0.6, 1.8),
                _ => 1.0,
            }
        };

        self.active_time += dt * playback_rate;
        if let Some((_, ref mut prev_t, _)) = self.prev_clip {
            *prev_t += dt * playback_rate;
            self.fade_timer += dt;
            if self.fade_timer >= CROSSFADE_DURATION {
                self.prev_clip = None;
                self.fade_timer = 0.0;
            }
        }

        self.evaluate_current_pose();
    }

    fn transition_to_clip(&mut self, new_clip: usize) {
        if new_clip == self.active_clip || new_clip >= self.clips.len() {
            return;
        }
        let prev_looping = self.manual_clip_override.is_some() || self.state.is_looping();
        self.prev_clip = Some((self.active_clip, self.active_time, prev_looping));
        self.active_clip = new_clip;
        self.active_time = 0.0;
        self.fade_timer = 0.0;
    }

    fn evaluate_current_pose(&mut self) {
        let Some(skeleton) = self.skeleton.as_ref() else {
            return;
        };
        let Some(clip_curr) = self.clips.get(self.active_clip) else {
            return;
        };

        self.scratch.ensure_capacity(skeleton.nodes.len());
        let curr_looping = self.manual_clip_override.is_some() || self.state.is_looping();

        sample_clip_into(
            skeleton,
            clip_curr,
            self.active_time,
            curr_looping,
            &mut self.scratch.local_a,
        );

        if let Some((prev_idx, prev_time, prev_loop)) = self.prev_clip {
            if let Some(clip_prev) = self.clips.get(prev_idx) {
                sample_clip_into(
                    skeleton,
                    clip_prev,
                    prev_time,
                    prev_loop,
                    &mut self.scratch.local_b,
                );
                let alpha = (self.fade_timer / CROSSFADE_DURATION).clamp(0.0, 1.0);
                for i in 0..skeleton.nodes.len() {
                    self.scratch.local_a[i] =
                        self.scratch.local_b[i].blend(self.scratch.local_a[i], alpha);
                }
            }
        }

        compute_hierarchy_and_joints(skeleton, &mut self.scratch);
    }

    /// Computes whole-body procedural motion transform (plus procedural attack lunge when swinging).
    pub fn procedural_local_matrix(
        &self,
        horiz_speed: f32,
        grounded: bool,
        vel_y: f32,
        swing_lunge_factor: f32,
    ) -> Mat4 {
        let lunge_pitch = -swing_lunge_factor * PROC_ATTACK_LEAN_RAD;
        let lunge_z = -swing_lunge_factor * PROC_ATTACK_DASH_DIST;

        if self.mode != AnimMode::Procedural {
            if swing_lunge_factor <= 1e-4 {
                return Mat4::IDENTITY;
            }
            return Mat4::from_scale_rotation_translation(
                Vec3::ONE,
                Quat::from_rotation_x(lunge_pitch),
                Vec3::new(0.0, 0.0, lunge_z),
            );
        }

        let speed_ratio = (horiz_speed / RUN_REFERENCE_SPEED).clamp(0.0, 1.2);
        let bob_y = if grounded && horiz_speed > WALK_SPEED_THRESHOLD {
            self.proc_phase.sin().abs() * PROC_BOB_AMP * speed_ratio
        } else {
            0.0
        };

        let lean_pitch = -speed_ratio * PROC_LEAN_MAX_RAD + lunge_pitch;

        let mut scale_y = 1.0;
        let mut scale_xz = 1.0;
        if !grounded {
            let stretch = (vel_y * 0.025).clamp(-0.08, PROC_JUMP_STRETCH);
            scale_y += stretch;
            scale_xz -= stretch * 0.45;
        } else if self.proc_squash > 0.0 {
            let sq = self.proc_squash * PROC_LAND_SQUASH;
            scale_y -= sq;
            scale_xz += sq * 0.55;
        }

        Mat4::from_scale_rotation_translation(
            Vec3::new(scale_xz, scale_y, scale_xz),
            Quat::from_rotation_x(lean_pitch),
            Vec3::new(0.0, bob_y, lunge_z),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_clip(name: &str) -> AnimationClip {
        AnimationClip {
            name: name.to_string(),
            duration: 1.0,
            channels: Vec::new(),
        }
    }

    #[test]
    fn test_state_transitions() {
        assert_eq!(select_state(0.0, true, 0.0), AnimState::Idle);
        assert_eq!(select_state(4.5, true, 0.0), AnimState::Walk);
        assert_eq!(select_state(8.0, true, 0.0), AnimState::Run);
        assert_eq!(select_state(3.0, false, 5.0), AnimState::Jump);
        assert_eq!(select_state(3.0, false, -2.5), AnimState::Fall);
        assert_eq!(select_state(0.0, true, 0.0), AnimState::Idle);
    }

    #[test]
    fn test_clip_matching_and_fallback_order() {
        let clips = vec![dummy_clip("Hero_Idle_01"), dummy_clip("Hero_Walk_Loop")];
        let map = ClipMap::from_clips(&clips);
        assert_eq!(map.idle, Some(0));
        assert_eq!(map.walk, Some(1));
        assert_eq!(map.run, Some(1));
        assert_eq!(map.jump, Some(0));
        assert_eq!(map.fall, Some(0));

        let idle_only = vec![dummy_clip("Stand_Idle")];
        let map2 = ClipMap::from_clips(&idle_only);
        assert_eq!(map2.walk, Some(0));
        assert_eq!(map2.run, Some(0));
    }
}
