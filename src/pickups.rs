use crate::ally::next_unit_f32;
use crate::frustum::Frustum;
use crate::life::{FOOD_DROP_CHANCE, MAX_FOOD_STOCK};
use crate::skill_inv::{PickupOutcome, SkillInventory};
use crate::skills::{SkillId, TIER1_SKILLS};
use crate::world::InstanceData;
use glam::{Mat4, Quat, Vec2, Vec3};

pub const MAX_SKILL_ORBS: usize = 16;
pub const ORB_DROP_CHANCE: f32 = 0.25;
pub const ORB_PICKUP_RADIUS: f32 = 1.5;
pub const ORB_LIFETIME: f32 = 20.0;
pub const ORB_BLINK_TIME: f32 = 3.0;
pub const FOOD_ORB_TINT: [f32; 3] = [0.95, 0.24, 0.20];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OrbCollectEvent {
    Skill(PickupOutcome),
    FoodAdded(u32),
}

#[derive(Copy, Clone, Debug)]
pub struct SkillOrb {
    pub pos: Vec3,
    pub skill: SkillId,
    pub is_food: bool,
    pub timer: f32,
    pub phase: f32,
    pub active: bool,
}

impl Default for SkillOrb {
    fn default() -> Self {
        Self {
            pos: Vec3::ZERO,
            skill: SkillId::Fireball,
            is_food: false,
            timer: 0.0,
            phase: 0.0,
            active: false,
        }
    }
}

impl SkillOrb {
    pub fn tint(&self) -> [f32; 3] {
        if self.is_food {
            FOOD_ORB_TINT
        } else {
            self.skill.def().tint
        }
    }
}

pub struct PickupManager {
    pub orbs: [SkillOrb; MAX_SKILL_ORBS],
    next_slot: usize,
    pub rng_state: u64,
    pub first_wave_drop_pending: bool,
    pub first_wave_food_checked: bool,
}

impl PickupManager {
    pub fn new(seed: u64) -> Self {
        Self {
            orbs: [SkillOrb::default(); MAX_SKILL_ORBS],
            next_slot: 0,
            rng_state: seed ^ 0xC3C3_3C3C_9999_6666,
            first_wave_drop_pending: true,
            first_wave_food_checked: false,
        }
    }

    pub fn reset(&mut self, seed: u64) {
        *self = Self::new(seed);
    }

    pub fn active_orb_count(&self) -> u32 {
        self.orbs.iter().filter(|o| o.active).count() as u32
    }

    /// Called when an enemy dies. Handles skill orb drop (guaranteed first Wave 1 kill, else 25%)
    /// and food orb drop (guaranteed in Wave 1 only if `food_stock < 3`, else 15%).
    pub fn on_enemy_killed(
        &mut self,
        enemy_pos: Vec3,
        current_wave: u32,
        food_stock: u32,
    ) -> Option<SkillId> {
        let should_drop_skill = if current_wave == 1 && self.first_wave_drop_pending {
            self.first_wave_drop_pending = false;
            true
        } else {
            next_unit_f32(&mut self.rng_state) < ORB_DROP_CHANCE
        };

        let mut dropped_skill = None;
        if should_drop_skill {
            let idx = ((next_unit_f32(&mut self.rng_state) * (TIER1_SKILLS.len() as f32)) as usize)
                .min(TIER1_SKILLS.len() - 1);
            let skill = TIER1_SKILLS[idx];
            self.spawn_orb(Vec3::new(enemy_pos.x, 0.0, enemy_pos.z), skill);
            dropped_skill = Some(skill);
        }

        let should_drop_food = if current_wave == 1 && !self.first_wave_food_checked && food_stock < 3 {
            self.first_wave_food_checked = true;
            true
        } else {
            if current_wave == 1 {
                self.first_wave_food_checked = true;
            }
            next_unit_f32(&mut self.rng_state) < FOOD_DROP_CHANCE
        };

        if should_drop_food {
            self.spawn_food_orb(Vec3::new(enemy_pos.x + 0.45, 0.0, enemy_pos.z));
        }

        dropped_skill
    }

    pub fn spawn_orb(&mut self, ground_pos: Vec3, skill: SkillId) {
        let slot = self.alloc_slot();
        self.orbs[slot] = SkillOrb {
            pos: ground_pos,
            skill,
            is_food: false,
            timer: ORB_LIFETIME,
            phase: 0.0,
            active: true,
        };
    }

    pub fn spawn_food_orb(&mut self, ground_pos: Vec3) {
        let slot = self.alloc_slot();
        self.orbs[slot] = SkillOrb {
            pos: ground_pos,
            skill: SkillId::Fireball,
            is_food: true,
            timer: ORB_LIFETIME,
            phase: 0.0,
            active: true,
        };
    }

    fn alloc_slot(&mut self) -> usize {
        self.orbs
            .iter()
            .position(|o| !o.active)
            .unwrap_or_else(|| {
                let s = self.next_slot;
                self.next_slot = (self.next_slot + 1) % MAX_SKILL_ORBS;
                s
            })
    }

    /// Updates orb timers/bobbing and checks player pickup within `ORB_PICKUP_RADIUS` (1.5m).
    pub fn update_with_food(
        &mut self,
        dt: f32,
        player_pos: Vec3,
        player_alive: bool,
        inv: &mut SkillInventory,
        food_stock: &mut u32,
    ) -> Option<OrbCollectEvent> {
        let player_xz = Vec2::new(player_pos.x, player_pos.z);
        let pickup_r_sq = ORB_PICKUP_RADIUS * ORB_PICKUP_RADIUS;
        let mut last_event = None;

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
                    if orb.is_food {
                        *food_stock = (*food_stock + 1).min(MAX_FOOD_STOCK);
                        last_event = Some(OrbCollectEvent::FoodAdded(*food_stock));
                    } else {
                        last_event = Some(OrbCollectEvent::Skill(inv.add_skill_copy(orb.skill)));
                    }
                }
            }
        }

        last_event
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
            let t = orb.tint();
            if frustum.intersects_local_aabb(model, cube_min, cube_max) {
                out.push(InstanceData {
                    model,
                    color: [t[0], t[1], t[2], 1.5],
                });
            }

            let beam_model = Mat4::from_scale_rotation_translation(
                Vec3::new(0.10, 3.2, 0.10),
                Quat::IDENTITY,
                Vec3::new(orb.pos.x, 1.6, orb.pos.z),
            );
            if frustum.intersects_local_aabb(beam_model, cube_min, cube_max) {
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

        // First kill on wave 1 with food_stock = 2 (< 3) guarantees both a skill orb and a food orb!
        let dropped = pickups.on_enemy_killed(Vec3::new(3.0, 0.0, 0.0), 1, 2);
        assert!(dropped.is_some());
        assert_eq!(pickups.active_orb_count(), 2);

        let mut food = 6u32;
        pickups.clear_for_test();
        pickups.spawn_orb(Vec3::ZERO, SkillId::IceShard);
        let out1 = pickups
            .update_with_food(0.1, Vec3::new(0.5, 0.0, 0.0), true, &mut inv, &mut food)
            .expect("should pick up");
        assert_eq!(
            out1,
            OrbCollectEvent::Skill(PickupOutcome::Unlocked {
                skill: SkillId::IceShard,
                equipped_slot: Some(2)
            })
        );
        assert_eq!(inv.slots[2], Some(SkillId::IceShard));
        assert_eq!(inv.level(SkillId::IceShard), 1);

        pickups.spawn_orb(Vec3::ZERO, SkillId::IceShard);
        let out2 = pickups
            .update_with_food(0.1, Vec3::ZERO, true, &mut inv, &mut food)
            .expect("should pick up duplicate");
        assert_eq!(
            out2,
            OrbCollectEvent::Skill(PickupOutcome::LeveledUp {
                skill: SkillId::IceShard,
                new_level: 2
            })
        );

        // Food orb pickup increments food_stock up to MAX_FOOD_STOCK (20)
        pickups.spawn_food_orb(Vec3::ZERO);
        let out_food = pickups
            .update_with_food(0.1, Vec3::ZERO, true, &mut inv, &mut food)
            .expect("should pick up food");
        assert_eq!(out_food, OrbCollectEvent::FoodAdded(7));
        assert_eq!(food, 7);

        let _ = inv.add_skill_copy(SkillId::Shockwave);
        let out_full = inv.add_skill_copy(SkillId::WindDash);
        assert_eq!(
            out_full,
            PickupOutcome::Unlocked {
                skill: SkillId::WindDash,
                equipped_slot: None
            }
        );

        pickups.spawn_orb(Vec3::new(20.0, 0.0, 20.0), SkillId::Fireball);
        assert_eq!(pickups.active_orb_count(), 1);
        pickups.update_with_food(20.5, Vec3::ZERO, true, &mut inv, &mut food);
        assert_eq!(pickups.active_orb_count(), 0);
    }
}

#[cfg(test)]
impl PickupManager {
    fn clear_for_test(&mut self) {
        self.orbs = [SkillOrb::default(); MAX_SKILL_ORBS];
    }
}
