use crate::skills::{
    SkillId, CAST_DURATION, MANA_REGEN_PAUSE, MANA_REGEN_RATE, PLAYER_MAX_MANA, SKILL_COUNT,
};
use glam::Vec2;

pub const SLOT_COUNT: usize = 4;
pub const MAX_SKILL_COPIES: u8 = 9;
pub const MAX_SKILL_LEVEL: u8 = 5;

pub const FUSION_RECIPES: [(SkillId, SkillId, SkillId); 4] = [
    (SkillId::Fireball, SkillId::IceShard, SkillId::SteamBurst),
    (SkillId::Fireball, SkillId::Shockwave, SkillId::MeteorSlam),
    (SkillId::IceShard, SkillId::WindDash, SkillId::FrostDash),
    (SkillId::HealPulse, SkillId::Shockwave, SkillId::HolyNova),
];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PickupOutcome {
    Unlocked { skill: SkillId, equipped_slot: Option<usize> },
    LeveledUp { skill: SkillId, new_level: u8 },
    ExtraCopy { skill: SkillId, count: u8 },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CastError {
    SlotEmpty,
    DeadOrDodging,
    MouseNotCaptured,
    OnCooldown,
    NotEnoughMana,
}

impl CastError {
    pub fn message(self) -> &'static str {
        match self {
            Self::SlotEmpty => "Slot empty",
            Self::DeadOrDodging => "Cannot cast now",
            Self::MouseNotCaptured => "Capture mouse to cast",
            Self::OnCooldown => "Skill on cooldown",
            Self::NotEnoughMana => "Not enough mana",
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct SkillHudSlot {
    pub skill: Option<SkillId>,
    pub level: u8,
    pub cooldown_ratio: f32,
    pub cooldown_remaining: f32,
    pub fail_flash: f32,
}

#[derive(Copy, Clone, Debug)]
pub struct SkillHudInfo {
    pub mana: f32,
    pub max_mana: f32,
    pub slots: [SkillHudSlot; SLOT_COUNT],
    pub counts: [u8; SKILL_COUNT],
    pub active_projectiles: u32,
    pub active_effects: u32,
    pub active_orbs: u32,
}

pub struct SkillInventory {
    pub counts: [u8; SKILL_COUNT],
    pub slots: [Option<SkillId>; SLOT_COUNT],
    pub cooldowns: [f32; SLOT_COUNT],
    pub fail_flash: [f32; SLOT_COUNT],
    pub mana: f32,
    pub regen_pause_timer: f32,
    pub cast_timer: f32,
    pub dash_timer: f32,
    pub dash_vel_xz: Vec2,
    pub dash_leaves_slow: bool,
    pub slow_drop_accum: f32,
}

impl SkillInventory {
    pub fn new() -> Self {
        let mut counts = [0u8; SKILL_COUNT];
        counts[SkillId::Fireball.index()] = 1;
        counts[SkillId::HealPulse.index()] = 1;

        let slots = [
            Some(SkillId::Fireball),
            Some(SkillId::HealPulse),
            None,
            None,
        ];

        Self {
            counts,
            slots,
            cooldowns: [0.0; SLOT_COUNT],
            fail_flash: [0.0; SLOT_COUNT],
            mana: PLAYER_MAX_MANA,
            regen_pause_timer: 0.0,
            cast_timer: 0.0,
            dash_timer: 0.0,
            dash_vel_xz: Vec2::ZERO,
            dash_leaves_slow: false,
            slow_drop_accum: 0.0,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn count(&self, skill: SkillId) -> u8 {
        self.counts[skill.index()]
    }

    pub fn level(&self, skill: SkillId) -> u8 {
        self.counts[skill.index()].min(MAX_SKILL_LEVEL)
    }

    pub fn is_casting(&self) -> bool {
        self.cast_timer > 0.0
    }

    pub fn is_skill_dashing(&self) -> bool {
        self.dash_timer > 0.0
    }

    pub fn add_skill_copy(&mut self, skill: SkillId) -> PickupOutcome {
        let idx = skill.index();
        let prev = self.counts[idx];
        let next = (prev + 1).min(MAX_SKILL_COPIES);
        self.counts[idx] = next;

        if prev == 0 {
            let mut equipped_slot = None;
            if !self.slots.contains(&Some(skill)) {
                if let Some(empty_idx) = self.slots.iter().position(|s| s.is_none()) {
                    self.slots[empty_idx] = Some(skill);
                    equipped_slot = Some(empty_idx);
                }
            }
            PickupOutcome::Unlocked {
                skill,
                equipped_slot,
            }
        } else if next <= MAX_SKILL_LEVEL {
            PickupOutcome::LeveledUp {
                skill,
                new_level: next,
            }
        } else {
            PickupOutcome::ExtraCopy { skill, count: next }
        }
    }

    /// Validates and initiates a cast from `slot_idx` (`0..4`).
    pub fn try_cast_slot(
        &mut self,
        slot_idx: usize,
        player_dead: bool,
        player_dodging: bool,
        mouse_captured: bool,
    ) -> Result<(SkillId, u8), CastError> {
        if slot_idx >= SLOT_COUNT {
            return Err(CastError::SlotEmpty);
        }
        let Some(skill) = self.slots[slot_idx] else {
            self.fail_flash[slot_idx] = 0.35;
            return Err(CastError::SlotEmpty);
        };
        if self.count(skill) == 0 {
            self.fail_flash[slot_idx] = 0.35;
            return Err(CastError::SlotEmpty);
        }
        if player_dead || player_dodging || self.is_skill_dashing() {
            self.fail_flash[slot_idx] = 0.35;
            return Err(CastError::DeadOrDodging);
        }
        if !mouse_captured {
            self.fail_flash[slot_idx] = 0.35;
            return Err(CastError::MouseNotCaptured);
        }
        if self.cooldowns[slot_idx] > 0.0 {
            self.fail_flash[slot_idx] = 0.35;
            return Err(CastError::OnCooldown);
        }

        let def = skill.def();
        if self.mana < def.mana_cost {
            self.fail_flash[slot_idx] = 0.35;
            return Err(CastError::NotEnoughMana);
        }

        self.mana = (self.mana - def.mana_cost).max(0.0);
        self.regen_pause_timer = MANA_REGEN_PAUSE;
        self.cooldowns[slot_idx] = def.cooldown;
        self.cast_timer = CAST_DURATION;

        Ok((skill, self.level(skill).max(1)))
    }

    /// Key T: searches `FUSION_RECIPES` in table order for the first recipe where both source
    /// skills have `count >= 2`, spends 1 copy of each, grants 1 copy of the Tier 2 result,
    /// and auto-equips it into the first empty slot if not already equipped.
    pub fn try_fuse_first_available(&mut self) -> Result<(SkillId, SkillId, SkillId), &'static str> {
        for &(a, b, result) in &FUSION_RECIPES {
            if a.def().tier != 1 || b.def().tier != 1 {
                continue;
            }
            if self.count(a) >= 2 && self.count(b) >= 2 {
                self.counts[a.index()] -= 1;
                self.counts[b.index()] -= 1;
                let _ = self.add_skill_copy(result);
                return Ok((a, b, result));
            }
        }
        Err("Need 2 copies of each skill to fuse")
    }

    pub fn update(&mut self, dt: f32) {
        for cd in &mut self.cooldowns {
            *cd = (*cd - dt).max(0.0);
        }
        for fl in &mut self.fail_flash {
            *fl = (*fl - dt).max(0.0);
        }
        self.cast_timer = (self.cast_timer - dt).max(0.0);
        if self.dash_timer > 0.0 {
            self.dash_timer = (self.dash_timer - dt).max(0.0);
        }

        if self.regen_pause_timer > 0.0 {
            let active_pause = dt.min(self.regen_pause_timer);
            self.regen_pause_timer = (self.regen_pause_timer - dt).max(0.0);
            let rem_dt = dt - active_pause;
            if rem_dt > 0.0 && self.mana < PLAYER_MAX_MANA {
                self.mana = (self.mana + MANA_REGEN_RATE * rem_dt).min(PLAYER_MAX_MANA);
            }
        } else if self.mana < PLAYER_MAX_MANA {
            self.mana = (self.mana + MANA_REGEN_RATE * dt).min(PLAYER_MAX_MANA);
        }
    }

    pub fn hud_snapshot(
        &self,
        active_projectiles: u32,
        active_effects: u32,
        active_orbs: u32,
    ) -> SkillHudInfo {
        let mut hud_slots = [SkillHudSlot {
            skill: None,
            level: 0,
            cooldown_ratio: 0.0,
            cooldown_remaining: 0.0,
            fail_flash: 0.0,
        }; SLOT_COUNT];

        for i in 0..SLOT_COUNT {
            if let Some(sk) = self.slots[i] {
                let cd_max = sk.def().cooldown.max(1e-4);
                hud_slots[i] = SkillHudSlot {
                    skill: Some(sk),
                    level: self.level(sk),
                    cooldown_ratio: (self.cooldowns[i] / cd_max).clamp(0.0, 1.0),
                    cooldown_remaining: self.cooldowns[i],
                    fail_flash: (self.fail_flash[i] / 0.35).clamp(0.0, 1.0),
                };
            } else {
                hud_slots[i].fail_flash = (self.fail_flash[i] / 0.35).clamp(0.0, 1.0);
            }
        }

        SkillHudInfo {
            mana: self.mana,
            max_mana: PLAYER_MAX_MANA,
            slots: hud_slots,
            counts: self.counts,
            active_projectiles,
            active_effects,
            active_orbs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mana_regen_and_pause_after_cast() {
        let mut inv = SkillInventory::new();
        // Cast Fireball from slot 0 (costs 20 mana)
        assert!(inv.try_cast_slot(0, false, false, true).is_ok());
        assert!((inv.mana - 80.0).abs() < 1e-4);

        // During first 1.0s pause, mana does not regenerate
        inv.update(0.8);
        assert!((inv.mana - 80.0).abs() < 1e-4);

        // Remaining 0.2s of pause + 1.0s of regen at 6/s -> 86.0 mana
        inv.update(1.2);
        assert!((inv.mana - 86.0).abs() < 1e-3);
    }

    #[test]
    fn test_cast_validity_checks() {
        let mut inv = SkillInventory::new();
        // Empty slot 2 fails
        assert_eq!(
            inv.try_cast_slot(2, false, false, true),
            Err(CastError::SlotEmpty)
        );
        // Mouse not captured fails
        assert_eq!(
            inv.try_cast_slot(0, false, false, false),
            Err(CastError::MouseNotCaptured)
        );
        // Dead or dodging fails
        assert_eq!(
            inv.try_cast_slot(0, true, false, true),
            Err(CastError::DeadOrDodging)
        );
        // Valid cast succeeds, immediate recast fails with OnCooldown
        assert!(inv.try_cast_slot(0, false, false, true).is_ok());
        assert_eq!(
            inv.try_cast_slot(0, false, false, true),
            Err(CastError::OnCooldown)
        );
        // Insufficient mana fails
        inv.cooldowns[0] = 0.0;
        inv.mana = 5.0;
        assert_eq!(
            inv.try_cast_slot(0, false, false, true),
            Err(CastError::NotEnoughMana)
        );
    }

    #[test]
    fn test_all_four_fusion_recipes_and_copy_rules() {
        for &(src_a, src_b, expected_res) in &FUSION_RECIPES {
            let mut inv = SkillInventory::new();
            inv.counts = [0; SKILL_COUNT];
            inv.slots = [Some(src_a), Some(src_b), None, None];

            // With only 1 copy of each, fusion fails
            inv.counts[src_a.index()] = 1;
            inv.counts[src_b.index()] = 1;
            assert!(inv.try_fuse_first_available().is_err());

            // With 2 copies of each, fusion succeeds, leaves 1 copy of each source,
            // adds 1 copy of Tier 2 result, and equips into empty slot 2
            inv.counts[src_a.index()] = 2;
            inv.counts[src_b.index()] = 2;
            let (a, b, res) = inv.try_fuse_first_available().expect("fusion should succeed");
            assert_eq!((a, b, res), (src_a, src_b, expected_res));
            assert_eq!(inv.count(src_a), 1);
            assert_eq!(inv.count(src_b), 1);
            assert_eq!(inv.count(expected_res), 1);
            assert_eq!(inv.slots[2], Some(expected_res));

            // Tier 2 skills cannot be fused again even with 4 copies
            inv.counts[expected_res.index()] = 4;
            assert!(inv.try_fuse_first_available().is_err());
        }
    }
}
