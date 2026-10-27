use crate::frustum::Frustum;
use crate::world::InstanceData;
use glam::{Mat4, Quat, Vec3};

pub const MAX_EFFECT_ENTRIES: usize = 64;

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum EffectKind {
    /// Flattened expanding ground ring (scale X/Z = radius, Y shrinks from 0.05 to 0).
    ExpandingRing { max_radius: f32 },
    /// Meteor Slam telegraph: pulsing target ring + falling meteor cube from 12 units above.
    MeteorTelegraph { radius: f32 },
    /// Pickup sparkle: 6 tiny cubes bursting outward when an orb is collected.
    PickupSparkle,
    /// Upward floating line of 4 tiny cubes over healed targets.
    HealParticles,
}

impl EffectKind {
    #[inline]
    pub fn priority(self) -> u8 {
        match self {
            Self::PickupSparkle | Self::HealParticles => 1,
            Self::ExpandingRing { .. } => 2,
            Self::MeteorTelegraph { .. } => 3,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct VisualEffect {
    pub kind: EffectKind,
    pub position: Vec3,
    pub age: f32,
    pub lifetime: f32,
    pub scale: f32,
    pub color: [f32; 3],
    pub active: bool,
}

impl Default for VisualEffect {
    fn default() -> Self {
        Self {
            kind: EffectKind::PickupSparkle,
            position: Vec3::ZERO,
            age: 0.0,
            lifetime: 0.5,
            scale: 1.0,
            color: [1.0, 1.0, 1.0],
            active: false,
        }
    }
}

/// Fixed-capacity 64-entry visual effects pool (no `wgpu` types).
pub struct EffectsPool {
    pub entries: [VisualEffect; MAX_EFFECT_ENTRIES],
}

impl Default for EffectsPool {
    fn default() -> Self {
        Self::new()
    }
}

impl EffectsPool {
    pub fn new() -> Self {
        Self {
            entries: [VisualEffect::default(); MAX_EFFECT_ENTRIES],
        }
    }

    pub fn clear(&mut self) {
        self.entries = [VisualEffect::default(); MAX_EFFECT_ENTRIES];
    }

    pub fn active_count(&self) -> u32 {
        self.entries.iter().filter(|e| e.active).count() as u32
    }

    /// Allocates a slot in the 64-entry pool. If full, overwrites the lowest-priority
    /// (and oldest among ties) entry so high-priority telegraphs are never dropped.
    pub fn spawn(
        &mut self,
        kind: EffectKind,
        position: Vec3,
        lifetime: f32,
        scale: f32,
        color: [f32; 3],
    ) {
        let slot = if let Some(free_idx) = self.entries.iter().position(|e| !e.active) {
            free_idx
        } else {
            let mut best_idx = 0;
            let mut best_prio = u8::MAX;
            let mut best_age = -1.0_f32;
            for (i, e) in self.entries.iter().enumerate() {
                let prio = e.kind.priority();
                if prio < best_prio || (prio == best_prio && e.age > best_age) {
                    best_prio = prio;
                    best_age = e.age;
                    best_idx = i;
                }
            }
            best_idx
        };

        self.entries[slot] = VisualEffect {
            kind,
            position,
            age: 0.0,
            lifetime: lifetime.max(0.05),
            scale,
            color,
            active: true,
        };
    }

    pub fn spawn_ring(&mut self, pos: Vec3, max_radius: f32, lifetime: f32, color: [f32; 3]) {
        self.spawn(
            EffectKind::ExpandingRing { max_radius },
            Vec3::new(pos.x, 0.03, pos.z),
            lifetime,
            max_radius,
            color,
        );
    }

    pub fn spawn_meteor_telegraph(&mut self, pos: Vec3, radius: f32, delay: f32, color: [f32; 3]) {
        self.spawn(
            EffectKind::MeteorTelegraph { radius },
            Vec3::new(pos.x, 0.04, pos.z),
            delay,
            radius,
            color,
        );
    }

    pub fn spawn_pickup_sparkle(&mut self, pos: Vec3, color: [f32; 3]) {
        self.spawn(
            EffectKind::PickupSparkle,
            pos + Vec3::new(0.0, 0.5, 0.0),
            0.45,
            1.0,
            color,
        );
    }

    pub fn spawn_heal_particles(&mut self, pos: Vec3) {
        self.spawn(
            EffectKind::HealParticles,
            pos + Vec3::new(0.0, 0.4, 0.0),
            0.65,
            1.0,
            [0.30, 0.98, 0.45],
        );
    }

    pub fn update(&mut self, dt: f32) {
        for e in &mut self.entries {
            if e.active {
                e.age += dt;
                if e.age >= e.lifetime {
                    e.active = false;
                }
            }
        }
    }

    pub fn append_visible_cube_instances<F>(
        &self,
        frustum: &Frustum,
        mut push_fn: F,
    ) where
        F: FnMut(InstanceData),
    {
        let cube_min = Vec3::splat(-0.5);
        let cube_max = Vec3::splat(0.5);

        // Pass 1: High & Medium priority effects (MeteorTelegraph, ExpandingRing)
        for pass_prio in [3u8, 2, 1] {
            for e in &self.entries {
                if !e.active || e.kind.priority() != pass_prio {
                    continue;
                }
                let t = (e.age / e.lifetime).clamp(0.0, 1.0);
                let fade = 1.0 - t;

                match e.kind {
                    EffectKind::ExpandingRing { max_radius } => {
                        let r = max_radius * (0.20 + 0.80 * t);
                        let h = (0.05 * fade).max(0.008);
                        let model = Mat4::from_scale_rotation_translation(
                            Vec3::new(r * 1.4, h, r * 1.4),
                            Quat::from_rotation_y(t * 1.5),
                            e.position,
                        );
                        if frustum.intersects_local_aabb(model, cube_min, cube_max) {
                            push_fn(InstanceData {
                                model,
                                color: [e.color[0], e.color[1], e.color[2], 1.0 + fade * 0.8],
                            });
                        }
                    }
                    EffectKind::MeteorTelegraph { radius } => {
                        let pulse = 0.85 + 0.15 * (t * 18.0).sin().abs();
                        let ring_r = radius * pulse * 1.35;
                        let ring_model = Mat4::from_scale_rotation_translation(
                            Vec3::new(ring_r, 0.05, ring_r),
                            Quat::from_rotation_y(t * 3.0),
                            e.position,
                        );
                        if frustum.intersects_local_aabb(ring_model, cube_min, cube_max) {
                            push_fn(InstanceData {
                                model: ring_model,
                                color: [1.0, 0.22, 0.08, 1.8],
                            });
                        }

                        // Tall thin falling cube from 12 units above dropping to ground at detonation
                        let drop_y = 12.0 * (1.0 - t);
                        let streak_model = Mat4::from_scale_rotation_translation(
                            Vec3::new(0.28, 2.2, 0.28),
                            Quat::IDENTITY,
                            Vec3::new(e.position.x, drop_y + 1.1, e.position.z),
                        );
                        if frustum.intersects_local_aabb(streak_model, cube_min, cube_max) {
                            push_fn(InstanceData {
                                model: streak_model,
                                color: [1.0, 0.55, 0.15, 1.95],
                            });
                        }
                    }
                    EffectKind::PickupSparkle => {
                        let dist = (0.25 + 1.35 * t) * e.scale;
                        let sz = (0.18 * fade * e.scale).max(0.04);
                        for i in 0..6 {
                            let angle = (i as f32) * (std::f32::consts::TAU / 6.0);
                            let p = e.position
                                + Vec3::new(angle.cos() * dist, t * 0.6, angle.sin() * dist);
                            let model = Mat4::from_scale_rotation_translation(
                                Vec3::splat(sz),
                                Quat::from_rotation_y(angle + t * 4.0),
                                p,
                            );
                            if frustum.intersects_local_aabb(model, cube_min, cube_max) {
                                push_fn(InstanceData {
                                    model,
                                    color: [e.color[0], e.color[1], e.color[2], 1.7],
                                });
                            }
                        }
                    }
                    EffectKind::HealParticles => {
                        let sz = (0.15 * fade).max(0.04);
                        for i in 0..4 {
                            let offset_y = (i as f32) * 0.38 + t * 1.4;
                            let p = e.position + Vec3::new(0.0, offset_y, 0.0);
                            let model = Mat4::from_scale_rotation_translation(
                                Vec3::splat(sz),
                                Quat::IDENTITY,
                                p,
                            );
                            if frustum.intersects_local_aabb(model, cube_min, cube_max) {
                                push_fn(InstanceData {
                                    model,
                                    color: [0.30, 0.98, 0.45, 1.6],
                                });
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effect_pool_allocation_expiry_and_priority_overflow() {
        let mut pool = EffectsPool::new();
        assert_eq!(pool.active_count(), 0);

        // Fill 32 slots with low-priority PickupSparkle (prio 1) and 32 slots with ExpandingRing (prio 2)
        for i in 0..32 {
            pool.spawn_pickup_sparkle(Vec3::new(i as f32, 0.0, 0.0), [1.0, 1.0, 1.0]);
        }
        for i in 0..32 {
            pool.spawn_ring(Vec3::new(i as f32, 0.0, 0.0), 3.0, 1.0, [1.0, 0.5, 0.0]);
        }
        assert_eq!(pool.active_count(), MAX_EFFECT_ENTRIES as u32);

        // Advance age slightly on slot 0 (which is a low-priority PickupSparkle)
        pool.entries[0].age = 0.20;

        // Spawning a high-priority MeteorTelegraph (prio 3) when full drops the oldest prio-1 sparkle (slot 0)
        pool.spawn_meteor_telegraph(Vec3::ZERO, 6.0, 0.6, [1.0, 0.2, 0.1]);
        assert_eq!(
            pool.entries[0].kind,
            EffectKind::MeteorTelegraph { radius: 6.0 }
        );

        // Step 2.0s -> all effects expire cleanly
        pool.update(2.0);
        assert_eq!(pool.active_count(), 0);
    }
}
