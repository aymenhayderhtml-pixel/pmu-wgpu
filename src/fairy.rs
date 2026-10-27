use crate::camp::CAMP_CENTER;
use crate::frustum::Frustum;
use crate::world::InstanceData;
use glam::{Mat4, Quat, Vec3};

pub const FAIRY_FLIGHT_DURATION: f32 = 2.5;
pub const FAIRY_FLIGHT_HEIGHT: f32 = 1.2;
pub const FAIRY_STARVING_TRIGGER_SECS: f32 = 10.0;
pub const FAIRY_TINT: [f32; 3] = [1.0, 0.65, 0.88];

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum FairyTask {
    StopQuarrel {
        ally_a_id: u32,
        ally_b_id: u32,
        target_pos: Vec3,
    },
    FeedStarving {
        ally_id: u32,
        target_pos: Vec3,
    },
}

#[derive(Copy, Clone, Debug)]
pub struct Fairy {
    pub active_task: Option<FairyTask>,
    pub start_pos: Vec3,
    pub pos: Vec3,
    pub flight_timer: f32,
    pub wing_phase: f32,
}

impl Default for Fairy {
    fn default() -> Self {
        Self::new()
    }
}

impl Fairy {
    pub fn new() -> Self {
        let flag = CAMP_CENTER + Vec3::new(0.0, FAIRY_FLIGHT_HEIGHT, 0.0);
        Self {
            active_task: None,
            start_pos: flag,
            pos: flag,
            flight_timer: 0.0,
            wing_phase: 0.0,
        }
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }

    pub fn is_active(&self) -> bool {
        self.active_task.is_some()
    }

    pub fn state_label(&self) -> &'static str {
        match self.active_task {
            None => "Idle",
            Some(FairyTask::StopQuarrel { .. }) => "Quarrel",
            Some(FairyTask::FeedStarving { .. }) => "Feeding",
        }
    }

    pub fn start_quarrel_intervention(&mut self, ally_a_id: u32, ally_b_id: u32, target_pos: Vec3) -> bool {
        if self.active_task.is_some() {
            return false;
        }
        self.start_pos = CAMP_CENTER + Vec3::new(0.0, FAIRY_FLIGHT_HEIGHT, 0.0);
        self.pos = self.start_pos;
        self.flight_timer = 0.0;
        self.active_task = Some(FairyTask::StopQuarrel {
            ally_a_id,
            ally_b_id,
            target_pos: Vec3::new(target_pos.x, FAIRY_FLIGHT_HEIGHT, target_pos.z),
        });
        true
    }

    pub fn start_feed_intervention(&mut self, ally_id: u32, target_pos: Vec3) -> bool {
        if self.active_task.is_some() {
            return false;
        }
        self.start_pos = CAMP_CENTER + Vec3::new(0.0, FAIRY_FLIGHT_HEIGHT, 0.0);
        self.pos = self.start_pos;
        self.flight_timer = 0.0;
        self.active_task = Some(FairyTask::FeedStarving {
            ally_id,
            target_pos: Vec3::new(target_pos.x, FAIRY_FLIGHT_HEIGHT, target_pos.z),
        });
        true
    }

    /// Advances fairy flight and returns `Some(completed_task)` when the 2.5s flight reaches the target.
    pub fn update(&mut self, dt: f32, updated_target_pos: Option<Vec3>) -> Option<FairyTask> {
        let mut task = self.active_task?;
        if let Some(new_p) = updated_target_pos {
            let p = Vec3::new(new_p.x, FAIRY_FLIGHT_HEIGHT, new_p.z);
            match &mut task {
                FairyTask::StopQuarrel { target_pos, .. } => *target_pos = p,
                FairyTask::FeedStarving { target_pos, .. } => *target_pos = p,
            }
            self.active_task = Some(task);
        }

        self.flight_timer += dt;
        self.wing_phase = (self.wing_phase + dt * 24.0).rem_euclid(std::f32::consts::TAU);

        let dest = match task {
            FairyTask::StopQuarrel { target_pos, .. } => target_pos,
            FairyTask::FeedStarving { target_pos, .. } => target_pos,
        };

        let t = (self.flight_timer / FAIRY_FLIGHT_DURATION).clamp(0.0, 1.0);
        let smooth_t = t * t * (3.0 - 2.0 * t);
        self.pos = self.start_pos.lerp(dest, smooth_t);
        self.pos.y = FAIRY_FLIGHT_HEIGHT + (self.wing_phase * 0.5).sin() * 0.12;

        if self.flight_timer >= FAIRY_FLIGHT_DURATION {
            self.active_task = None;
            Some(task)
        } else {
            None
        }
    }

    pub fn append_visible_cube_instances<F>(&self, frustum: &Frustum, mut push_fn: F)
    where
        F: FnMut(InstanceData),
    {
        if !self.is_active() {
            return;
        }
        let c_min = Vec3::splat(-0.5);
        let c_max = Vec3::splat(0.5);

        // Glowing light-pink core cube
        let body = Mat4::from_scale_rotation_translation(
            Vec3::splat(0.28),
            Quat::from_rotation_y(self.wing_phase * 0.25),
            self.pos,
        );
        if frustum.intersects_local_aabb(body, c_min, c_max) {
            push_fn(InstanceData {
                model: body,
                color: [FAIRY_TINT[0], FAIRY_TINT[1], FAIRY_TINT[2], 1.85],
            });
        }

        // Two flapping wing cubes
        let flap = self.wing_phase.sin() * 0.55;
        for side in [-1.0_f32, 1.0] {
            let wing_offset = Vec3::new(side * 0.24, flap * 0.08, 0.0);
            let wing = Mat4::from_scale_rotation_translation(
                Vec3::new(0.22, 0.06, 0.16),
                Quat::from_rotation_z(side * flap),
                self.pos + wing_offset,
            );
            if frustum.intersects_local_aabb(wing, c_min, c_max) {
                push_fn(InstanceData {
                    model: wing,
                    color: [1.0, 0.85, 0.96, 1.95],
                });
            }
        }
    }
}

/// Restores health after a quarrel: if the ally only lost health from the quarrel (`only_quarrel_damage`),
/// restores `pre_quarrel_hp`; otherwise heals by `20%` of `max_hp`.
#[inline]
pub fn compute_post_quarrel_health(
    current_hp: f32,
    pre_quarrel_hp: f32,
    max_hp: f32,
    only_quarrel_damage: bool,
) -> f32 {
    if only_quarrel_damage {
        pre_quarrel_hp.clamp(current_hp, max_hp)
    } else {
        (current_hp + max_hp * 0.20).min(max_hp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fairy_flight_quarrel_restore_and_single_intervention_limit() {
        let mut fairy = Fairy::new();
        assert!(fairy.start_quarrel_intervention(1, 2, Vec3::new(-18.0, 0.0, -18.0)));
        // At most one fairy intervention at a time
        assert!(!fairy.start_feed_intervention(3, Vec3::ZERO));

        assert!(fairy.update(1.5, None).is_none());
        let done = fairy.update(1.1, None);
        assert!(matches!(done, Some(FairyTask::StopQuarrel { ally_a_id: 1, ally_b_id: 2, .. })));
        assert!(!fairy.is_active());

        // Health restore rules
        assert!((compute_post_quarrel_health(42.0, 60.0, 60.0, true) - 60.0).abs() < 1e-4);
        assert!((compute_post_quarrel_health(30.0, 60.0, 60.0, false) - 42.0).abs() < 1e-4);
    }
}
