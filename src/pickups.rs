use crate::ally::next_unit_f32;
use crate::frustum::Frustum;
use crate::skill_inv::{PickupOutcome, SkillInventory};
use crate::skills::{SkillId, TIER1_SKILLS};
use crate::world::InstanceData;
use glam::{Mat4, Quat, Vec2, Vec3};

pub const MAX_SKILL_ORBS: usize = 16;
pub const ORB_DROP_CHANCE: f32 = 0.25;
pub const ORB_PICKUP_RADIUS: f32 = 1.5;
pub const ORB_LIFETIME: f32 = 20.0;
pub const ORB_BLINK_TIME: f32 = 3.0;

#[derive(Copy, Clone, Debug)]
pub struct SkillOrb {
    pub pos: Vec3,
    pub skill: SkillId,
    pub timer: f32,
    pub phase: f32,
    pub active: bool,
}

impl Default for SkillOrb {
    fn default() -> Self {
        Self {
            pos: Vec3::ZERO,
            skill: SkillId::Fireball,
            timer: 0.0,
            phase: 0.0,
            active: false,
        }
    }
}

pub struct PickupManager {
    pub orbs: [SkillOrb; MAX_SKILL_ORBS],
    next_slot: usize,
    pub rng_state: u64,
    pub first_wave_drop_pending: bool,
}

impl PickupManager {
    pub fn new(seed: u64) -> Self {
        Self {
            orbs: [SkillOrb::default(); MAX_SKILL_ORBS],
            next_slot: 0,
            rng_state: seed ^ 0xC3C3_3C3C_9999_6666,
            first_wave_drop_pending: true,
        }
    }

    pub fn reset(&mut self, seed: u64) {
        *self = Self::new(seed);
    }

    pub fn active_orb_count(&self) -> u32 {
        self.orbs.iter().filter(|o| o.active).count() as u32
    }

    /// Called when an enemy dies. First kill in wave 1 always drops an orb; otherwise 25% chance.
    pub fn on_enemy_killed(&mut self, enemy_pos: Vec3, current_wave: u32) -> Option<SkillId> {
        let should_drop = if current_wave == 1 && self.first_wave_drop_pending {
            self.first_wave_drop_pending = false;
            true
        } else {
            next_unit_f32(&mut self.rng_state) < ORB_DROP_CHANCE
        };

        if !should_drop {
            return None;
        }

        let idx = ((next_unit_f32(&mut self.rng_state) * (TIER1_SKILLS.len() as f32)) as usize)
            .min(TIER1_SKILLS.len() - 1);
        let skill = TIER1_SKILLS[idx];
        self.spawn_orb(Vec3::new(enemy_pos.x, 0.0, enemy_pos.z), skill);
        Some(skill)
    }

    pub fn spawn_orb(&mut self, ground_pos: Vec3, skill: SkillId) {
        let slot = self
            .orbs
            .iter()
            .position(|o| !o.active)
            .unwrap_or_else(|| {
                let s = self.next_slot;
                self.next_slot = (self.next_slot + 1) % MAX_SKILL_ORBS;
                s
            });

        self.orbs[slot] = SkillOrb {
            pos: ground_pos,
            skill,
            timer: ORB_LIFETIME,
            phase: 0.0,
            active: true,
        };
    }

    /// Updates orb timers/bobbing and checks player pickup within `ORB_PICKUP_RADIUS` (1.5m).
    pub fn update(
        &mut self,
        dt: f32,
        player_pos: Vec3,
        player_alive: bool,
        inv: &mut SkillInventory,
    ) -> Option<PickupOutcome> {
        let player_xz = Vec2::new(player_pos.x, player_pos.z);
        let pickup_r_sq = ORB_PICKUP_RADIUS * ORB_PICKUP_RADIUS;
        let mut last_outcome = None;

        for orb in &mut self.orbs {
            if !orb.active {
                continue;
            }
            orb.timer -= dt;
            orb.phase = (orb.phase + dt * 4.0).rem_euclid(std::f32::consts::TAU);
            if orb.timer <= 0.0 {
                orb.active = false;
                continue;
            }

            if player_alive {
                let d_sq = Vec2::new(orb.pos.x, orb.pos.z).distance_squared(player_xz);
                if d_sq <= pickup_r_sq {
                    orb.active = false;
                    last_outcome = Some(inv.add_skill_copy(orb.skill));
                }
            }
        }

        last_outcome
    }

    pub fn append_visible_cube_instances(&self, frustum: &Frustum, out: &mut Vec<InstanceData>) {
        let cube_min = Vec3::splat(-0.5);
        let cube_max = Vec3::splat(0.5);

        for orb in &self.orbs {
            if !orb.active {
                continue;
            }
            // Blink in the last 3.0 seconds before despawning
            if orb.timer <= ORB_BLINK_TIME && ((orb.timer * 10.0) as i32 % 2 == 0) {
                continue;
            }
            let bob_y = 0.55 + orb.phase.sin() * 0.14;
            let model = Mat4::from_scale_rotation_translation(
                Vec3::splat(0.42),
                Quat::from_rotation_y(orb.phase),
                Vec3::new(orb.pos.x, bob_y, orb.pos.z),
            );
            if frustum.intersects_local_aabb(model, cube_min, cube_max) {
                let t = orb.skill.def().tint;
                out.push(InstanceData {
                    model,
                    color: [t[0], t[1], t[2], 1.5],
                });
            }

            // Thin tall beacon beam above each orb so it is visible from afar
            let beam_model = Mat4::from_scale_rotation_translation(
                Vec3::new(0.10, 3.2, 0.10),
                Quat::IDENTITY,
                Vec3::new(orb.pos.x, 1.6, orb.pos.z),
            );
            if frustum.intersects_local_aabb(beam_model, cube_min, cube_max) {
                let t = orb.skill.def().tint;
                out.push(InstanceData {
                    model: beam_model,
                    color: [t[0], t[1], t[2], 1.75],
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_first_wave_guaranteed_drop_and_orb_pickup_rules() {
        let mut pickups = PickupManager::new(12345);
        let mut inv = SkillInventory::new();

        // First kill on wave 1 is guaranteed to drop a Tier 1 skill orb
        let dropped = pickups.on_enemy_killed(Vec3::new(3.0, 0.0, 0.0), 1);
        assert!(dropped.is_some());
        assert_eq!(pickups.active_orb_count(), 1);

        // Spawn a new locked skill (IceShard) at origin and walk player over it -> unlocks and equips into slot 2
        pickups.clear_for_test();
        pickups.spawn_orb(Vec3::ZERO, SkillId::IceShard);
        let out1 = pickups
            .update(0.1, Vec3::new(0.5, 0.0, 0.0), true, &mut inv)
            .expect("should pick up");
        assert_eq!(
            out1,
            PickupOutcome::Unlocked {
                skill: SkillId::IceShard,
                equipped_slot: Some(2)
            }
        );
        assert_eq!(inv.slots[2], Some(SkillId::IceShard));
        assert_eq!(inv.level(SkillId::IceShard), 1);

        // Picking up a second IceShard levels it up to level 2
        pickups.spawn_orb(Vec3::ZERO, SkillId::IceShard);
        let out2 = pickups
            .update(0.1, Vec3::ZERO, true, &mut inv)
            .expect("should pick up duplicate");
        assert_eq!(
            out2,
            PickupOutcome::LeveledUp {
                skill: SkillId::IceShard,
                new_level: 2
            }
        );
        assert_eq!(inv.level(SkillId::IceShard), 2);

        // Fill slot 3 with Shockwave, then pick up WindDash -> unlocks with equipped_slot: None (full slots)
        let _ = inv.add_skill_copy(SkillId::Shockwave);
        assert_eq!(inv.slots[3], Some(SkillId::Shockwave));
        let out_full = inv.add_skill_copy(SkillId::WindDash);
        assert_eq!(
            out_full,
            PickupOutcome::Unlocked {
                skill: SkillId::WindDash,
                equipped_slot: None
            }
        );

        // Orb despawns after 20.0s
        pickups.spawn_orb(Vec3::new(20.0, 0.0, 20.0), SkillId::Fireball);
        assert_eq!(pickups.active_orb_count(), 1);
        pickups.update(20.5, Vec3::ZERO, true, &mut inv);
        assert_eq!(pickups.active_orb_count(), 0);
    }
}

#[cfg(test)]
impl PickupManager {
    fn clear_for_test(&mut self) {
        self.orbs = [SkillOrb::default(); MAX_SKILL_ORBS];
    }
}
