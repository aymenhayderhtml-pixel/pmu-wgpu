use crate::ally::{next_unit_f32, Rarity};

pub const MAX_ALLY_LEVEL: u8 = 10;
pub const XP_PER_LEVEL_BASE: u32 = 100;
pub const LEVEL_HP_BONUS_PER_LVL: f32 = 0.04;
pub const LEVEL_DMG_BONUS_PER_LVL: f32 = 0.04;

pub const MAX_PASSIVE_LEVEL: u8 = 5;
pub const SKILL_PROGRESS_PER_LEVEL: u8 = 3;
pub const PAIN_DAMAGE_PER_POINT: f32 = 150.0;
pub const PAIN_POINTS_PER_LEVEL: u32 = 3;

pub const SWORDSMANSHIP_DMG_PER_LVL: f32 = 0.06;
pub const ARCHERY_DMG_PER_LVL: f32 = 0.06;
pub const SHIELD_MITIGATION_PER_LVL: f32 = 0.04;
pub const PAIN_TOLERANCE_HP_PER_LVL: f32 = 0.06;
pub const SWORD_SHIELD_DMG_PER_LVL: f32 = 0.05;
pub const SWORD_SHIELD_MITIGATION_PER_LVL: f32 = 0.04;

pub const HUNGRY_RATE_MULT: f32 = 1.3;
pub const LIGHT_EATER_RATE_MULT: f32 = 0.7;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AllyClass {
    Fighter = 0,
    Archer = 1,
}

impl AllyClass {
    pub fn from_u8(v: u8) -> Self {
        if v == 1 {
            Self::Archer
        } else {
            Self::Fighter
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Fighter => "Fighter",
            Self::Archer => "Archer",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Fighter => "FTR",
            Self::Archer => "ARC",
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PersonalityTrait {
    Brave = 0,
    Cautious = 1,
    Diligent = 2,
    Lazy = 3,
    Hotheaded = 4,
    Calm = 5,
    HungryType = 6,
    LightEater = 7,
}

impl PersonalityTrait {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Brave,
            1 => Self::Cautious,
            2 => Self::Diligent,
            3 => Self::Lazy,
            4 => Self::Hotheaded,
            5 => Self::Calm,
            6 => Self::HungryType,
            _ => Self::LightEater,
        }
    }

    pub fn short_tag(self) -> &'static str {
        match self {
            Self::Brave => "Brv",
            Self::Cautious => "Cau",
            Self::Diligent => "Dil",
            Self::Lazy => "Lzy",
            Self::Hotheaded => "Hot",
            Self::Calm => "Clm",
            Self::HungryType => "Hng",
            Self::LightEater => "Lgt",
        }
    }
}

#[repr(usize)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PassiveSkill {
    BasicSwordsmanship = 0,
    BasicArchery = 1,
    BasicShield = 2,
    PainTolerance = 3,
    BasicSwordShield = 4,
}

pub const PASSIVE_SKILL_COUNT: usize = 5;

pub const ALL_PASSIVE_SKILLS: [PassiveSkill; PASSIVE_SKILL_COUNT] = [
    PassiveSkill::BasicSwordsmanship,
    PassiveSkill::BasicArchery,
    PassiveSkill::BasicShield,
    PassiveSkill::PainTolerance,
    PassiveSkill::BasicSwordShield,
];

impl PassiveSkill {
    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::BasicSwordsmanship => "Basic Swordsmanship",
            Self::BasicArchery => "Basic Archery",
            Self::BasicShield => "Basic Shield",
            Self::PainTolerance => "Pain Tolerance",
            Self::BasicSwordShield => "Basic Sword-Shield",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Self::BasicSwordsmanship => "Swd",
            Self::BasicArchery => "Arc",
            Self::BasicShield => "Shd",
            Self::PainTolerance => "Pain",
            Self::BasicSwordShield => "SwSh",
        }
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct PassiveSkillSet {
    pub levels: [u8; PASSIVE_SKILL_COUNT],
    pub progress: [u8; PASSIVE_SKILL_COUNT],
    pub fighter_train_shield_next: bool,
    pub total_damage_taken: f32,
    pub total_rest_healed: f32,
    pub pain_points: u32,
}

impl PassiveSkillSet {
    #[inline]
    pub fn level(&self, skill: PassiveSkill) -> u8 {
        self.levels[skill.index()]
    }

    pub fn record_damage_taken(&mut self, dmg: f32) {
        if dmg > 0.0 {
            self.total_damage_taken += dmg;
        }
    }

    /// Records health restored via resting and updates `Pain Tolerance` (1 pain point per 150 damage
    /// that has later been healed by resting, never double-counting; 3 pain points -> +1 level up to 5).
    /// Returns `Some(new_level)` if `Pain Tolerance` leveled up.
    pub fn record_rest_healed(&mut self, healed: f32) -> Option<u8> {
        if healed <= 0.0 {
            return None;
        }
        self.total_rest_healed = (self.total_rest_healed + healed).min(self.total_damage_taken);
        self.recompute_pain_tolerance()
    }

    pub fn recompute_pain_tolerance(&mut self) -> Option<u8> {
        let covered = self.total_damage_taken.min(self.total_rest_healed);
        let new_points = (covered / PAIN_DAMAGE_PER_POINT).floor() as u32;
        if new_points > self.pain_points {
            self.pain_points = new_points;
        }
        let target_lvl = ((self.pain_points / PAIN_POINTS_PER_LEVEL) as u8).min(MAX_PASSIVE_LEVEL);
        let prev = self.levels[PassiveSkill::PainTolerance.index()];
        if target_lvl > prev {
            self.levels[PassiveSkill::PainTolerance.index()] = target_lvl;
            Some(target_lvl)
        } else {
            None
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AllyProfile {
    pub class: AllyClass,
    pub traits: [PersonalityTrait; 2],
    pub stars: u8,
    pub level: u8,
    pub xp: u32,
    pub passives: PassiveSkillSet,
}

impl AllyProfile {
    pub fn roll(rng_state: &mut u64, rarity: Rarity) -> Self {
        let class = if next_unit_f32(rng_state) < 0.5 {
            AllyClass::Fighter
        } else {
            AllyClass::Archer
        };

        // Pick 2 distinct trait axes out of 4 (0=Courage, 1=Work, 2=Temper, 3=Appetite)
        let axis_a = ((next_unit_f32(rng_state) * 4.0) as usize).min(3);
        let offset = 1 + ((next_unit_f32(rng_state) * 3.0) as usize).min(2);
        let axis_b = (axis_a + offset) % 4;

        let pick_on_axis = |axis: usize, rng: &mut u64| -> PersonalityTrait {
            let second = next_unit_f32(rng) >= 0.5;
            PersonalityTrait::from_u8((axis * 2 + usize::from(second)) as u8)
        };

        let t0 = pick_on_axis(axis_a.min(axis_b), rng_state);
        let t1 = pick_on_axis(axis_a.max(axis_b), rng_state);

        Self {
            class,
            traits: [t0, t1],
            stars: stars_for_rarity(rarity),
            level: 1,
            xp: 0,
            passives: PassiveSkillSet::default(),
        }
    }

    pub fn from_deterministic_v1(rarity: Rarity, name_index: usize) -> Self {
        let mut seed = 0x517C_C1B7_2722_0A95_u64
            ^ ((name_index as u64 + 1) * 0x9E37_79B9_7F4A_7C15)
            ^ ((stars_for_rarity(rarity) as u64) << 32);
        Self::roll(&mut seed, rarity)
    }

    #[inline]
    pub fn has_trait(&self, t: PersonalityTrait) -> bool {
        self.traits[0] == t || self.traits[1] == t
    }

    #[inline]
    pub fn hunger_rate_multiplier(&self) -> f32 {
        if self.has_trait(PersonalityTrait::HungryType) {
            HUNGRY_RATE_MULT
        } else if self.has_trait(PersonalityTrait::LightEater) {
            LIGHT_EATER_RATE_MULT
        } else {
            1.0
        }
    }

    #[inline]
    pub fn xp_needed_for_next_level(&self) -> u32 {
        xp_for_next_level(self.level)
    }

    /// Adds `amount` XP and levels up (up to `MAX_ALLY_LEVEL = 10`). Returns `true` if level increased.
    pub fn add_xp(&mut self, amount: u32) -> bool {
        if self.level >= MAX_ALLY_LEVEL || amount == 0 {
            return false;
        }
        self.xp += amount;
        let mut leveled_up = false;
        while self.level < MAX_ALLY_LEVEL {
            let req = xp_for_next_level(self.level);
            if self.xp >= req {
                self.xp -= req;
                self.level += 1;
                leveled_up = true;
            } else {
                break;
            }
        }
        if self.level >= MAX_ALLY_LEVEL {
            self.xp = 0;
        }
        leveled_up
    }

    /// Max health multiplier combining level (`+4%` per level above 1) and `Pain Tolerance` (`+6%` per level).
    pub fn max_health_multiplier(&self) -> f32 {
        let lvl_bonus = f32::from(self.level.saturating_sub(1)) * LEVEL_HP_BONUS_PER_LVL;
        let pain_bonus =
            f32::from(self.passives.level(PassiveSkill::PainTolerance)) * PAIN_TOLERANCE_HP_PER_LVL;
        (1.0 + lvl_bonus) * (1.0 + pain_bonus)
    }

    /// Outgoing damage multiplier combining level (`+4%` per level above 1) and class/combined passive skills.
    pub fn damage_multiplier(&self) -> f32 {
        let lvl_bonus = f32::from(self.level.saturating_sub(1)) * LEVEL_DMG_BONUS_PER_LVL;
        let swd = f32::from(self.passives.level(PassiveSkill::BasicSwordsmanship))
            * SWORDSMANSHIP_DMG_PER_LVL;
        let arc = if self.class == AllyClass::Archer {
            f32::from(self.passives.level(PassiveSkill::BasicArchery)) * ARCHERY_DMG_PER_LVL
        } else {
            0.0
        };
        let sw_shd = f32::from(self.passives.level(PassiveSkill::BasicSwordShield))
            * SWORD_SHIELD_DMG_PER_LVL;
        (1.0 + lvl_bonus) * (1.0 + swd + arc + sw_shd)
    }

    /// Incoming damage multiplier from `Basic Shield` (`-4%/lvl`) and `Basic Sword-Shield` (`-4%/lvl`).
    pub fn incoming_damage_multiplier(&self) -> f32 {
        let shd = f32::from(self.passives.level(PassiveSkill::BasicShield))
            * SHIELD_MITIGATION_PER_LVL;
        let sw_shd = f32::from(self.passives.level(PassiveSkill::BasicSwordShield))
            * SWORD_SHIELD_MITIGATION_PER_LVL;
        (1.0 - (shd + sw_shd).min(0.60)).max(0.20)
    }
}

#[inline]
pub fn stars_for_rarity(rarity: Rarity) -> u8 {
    match rarity {
        Rarity::Common => 1,
        Rarity::Rare => 2,
        Rarity::Epic => 3,
    }
}

#[inline]
pub fn xp_for_next_level(level: u8) -> u32 {
    XP_PER_LEVEL_BASE * u32::from(level.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level_xp_progression_and_stat_multipliers() {
        let mut seed = 42u64;
        let mut prof = AllyProfile::roll(&mut seed, Rarity::Common);
        assert_eq!(prof.stars, 1);
        assert_eq!(prof.level, 1);
        assert_eq!(prof.xp_needed_for_next_level(), 100);

        // 100 XP reaches level 2; next level requires 200 XP
        assert!(prof.add_xp(100));
        assert_eq!(prof.level, 2);
        assert_eq!(prof.xp, 0);
        assert_eq!(prof.xp_needed_for_next_level(), 200);
        assert!((prof.max_health_multiplier() - 1.04).abs() < 1e-5);
        assert!((prof.damage_multiplier() - 1.04).abs() < 1e-5);

        // Cap at level 10
        assert!(prof.add_xp(50_000));
        assert_eq!(prof.level, MAX_ALLY_LEVEL);
        assert_eq!(prof.xp, 0);
        assert!(!prof.add_xp(500));
    }

    #[test]
    fn test_pain_tolerance_counting_without_double_counting() {
        let mut set = PassiveSkillSet::default();
        // Take 500 damage, heal 150 by resting -> 1 pain point (level 0)
        set.record_damage_taken(500.0);
        assert_eq!(set.record_rest_healed(150.0), None);
        assert_eq!(set.pain_points, 1);

        // Heal another 300 by resting (total covered = 450 = 3 pain points) -> Pain Tolerance Lv 1!
        assert_eq!(set.record_rest_healed(300.0), Some(1));
        assert_eq!(set.pain_points, 3);
        assert_eq!(set.level(PassiveSkill::PainTolerance), 1);

        // Resting more without taking new damage does not double-count beyond total_damage_taken (500)
        assert_eq!(set.record_rest_healed(1000.0), None);
        assert_eq!(set.pain_points, 3);
    }
}
