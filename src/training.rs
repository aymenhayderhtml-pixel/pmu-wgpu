use crate::ally::next_unit_f32;
use crate::life::{AllyNeeds, FATIGUE_FROM_TRAIN_OR_SPAR};
use crate::traits::{
    AllyClass, AllyProfile, PassiveSkill, PersonalityTrait, MAX_PASSIVE_LEVEL,
    SKILL_PROGRESS_PER_LEVEL,
};

pub const BASE_TRAIN_XP: u32 = 30;
pub const DILIGENT_BONUS_XP: u32 = 10;
pub const SPAR_XP_GAIN: u32 = 20;

pub const ALLY_TRAINING_DEATH_CHANCE: f32 = 0.01;
pub const QUARREL_ESCALATION_CHANCE: f32 = 0.25;
pub const QUARREL_MAX_DURATION: f32 = 6.0;
pub const QUARREL_DAMAGE_MULT: f32 = 0.30;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TrainRollKind {
    NoGain,
    Normal,
    GreatSession,
}

impl TrainRollKind {
    /// Rolls 15% NoGain, 70% Normal, 15% GreatSession.
    pub fn roll(rng_state: &mut u64) -> Self {
        let r = next_unit_f32(rng_state);
        if r < 0.15 {
            Self::NoGain
        } else if r < 0.85 {
            Self::Normal
        } else {
            Self::GreatSession
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SkillGainEvent {
    None,
    Learned(PassiveSkill, u8),
    Improved(PassiveSkill, u8),
    CombinedSwordShield(u8),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TrainingOutcome {
    pub roll: TrainRollKind,
    pub xp_gained: u32,
    pub leveled_up_to: Option<u8>,
    pub skill_event: SkillGainEvent,
    pub died: bool,
}

/// Resolves a completed Train session for an ally using `death_chance` (pass `ALLY_TRAINING_DEATH_CHANCE` in gameplay).
pub fn resolve_training_session(
    profile: &mut AllyProfile,
    needs: &mut AllyNeeds,
    hp_fraction: f32,
    death_chance: f32,
    rng_state: &mut u64,
) -> TrainingOutcome {
    let fatigue_before = needs.fatigue;
    needs.fatigue = (needs.fatigue + FATIGUE_FROM_TRAIN_OR_SPAR).min(100.0);

    let roll = TrainRollKind::roll(rng_state);

    // Danger check: GreatSession when health < 70% or fatigue > 70 has `death_chance` (1.0%) to kill the ally
    if roll == TrainRollKind::GreatSession
        && death_chance > 0.0
        && (hp_fraction < 0.70 || fatigue_before > 70.0)
        && next_unit_f32(rng_state) < death_chance
    {
        return TrainingOutcome {
            roll,
            xp_gained: 0,
            leveled_up_to: None,
            skill_event: SkillGainEvent::None,
            died: true,
        };
    }

    let base_xp = BASE_TRAIN_XP
        + if profile.has_trait(PersonalityTrait::Diligent) {
            DILIGENT_BONUS_XP
        } else {
            0
        };

    let xp_gained = match roll {
        TrainRollKind::NoGain => 0,
        TrainRollKind::Normal => base_xp,
        TrainRollKind::GreatSession => base_xp * 2,
    };

    let leveled = profile.add_xp(xp_gained);
    let leveled_up_to = if leveled { Some(profile.level) } else { None };

    // Advance passive skill progress
    let target_skill = match profile.class {
        AllyClass::Fighter => {
            if profile.passives.level(PassiveSkill::BasicSwordShield) > 0 {
                PassiveSkill::BasicSwordShield
            } else {
                let s = if profile.passives.fighter_train_shield_next {
                    PassiveSkill::BasicShield
                } else {
                    PassiveSkill::BasicSwordsmanship
                };
                profile.passives.fighter_train_shield_next =
                    !profile.passives.fighter_train_shield_next;
                s
            }
        }
        AllyClass::Archer => PassiveSkill::BasicArchery,
    };

    let skill_event = advance_passive_skill(&mut profile.passives, target_skill);

    TrainingOutcome {
        roll,
        xp_gained,
        leveled_up_to,
        skill_event,
        died: false,
    }
}

fn advance_passive_skill(
    passives: &mut crate::traits::PassiveSkillSet,
    skill: PassiveSkill,
) -> SkillGainEvent {
    let idx = skill.index();
    if passives.levels[idx] >= MAX_PASSIVE_LEVEL {
        return SkillGainEvent::None;
    }

    passives.progress[idx] += 1;
    if passives.progress[idx] < SKILL_PROGRESS_PER_LEVEL {
        return SkillGainEvent::None;
    }
    passives.progress[idx] = 0;

    // Skill combining rule: if BasicSwordsmanship and BasicShield are both >= Lv 2 and
    // BasicSwordShield is not yet learned, the next level-up creates BasicSwordShield at Lv 3!
    let swd_lvl = passives.level(PassiveSkill::BasicSwordsmanship);
    let shd_lvl = passives.level(PassiveSkill::BasicShield);
    let comb_idx = PassiveSkill::BasicSwordShield.index();

    if matches!(
        skill,
        PassiveSkill::BasicSwordsmanship | PassiveSkill::BasicShield
    ) && swd_lvl >= 2
        && shd_lvl >= 2
        && passives.levels[comb_idx] == 0
    {
        passives.levels[comb_idx] = 3;
        return SkillGainEvent::CombinedSwordShield(3);
    }

    let prev = passives.levels[idx];
    let next = (prev + 1).min(MAX_PASSIVE_LEVEL);
    passives.levels[idx] = next;
    if prev == 0 {
        SkillGainEvent::Learned(skill, next)
    } else {
        SkillGainEvent::Improved(skill, next)
    }
}

/// Checks if a sparring pair escalates into a Quarrel (25% chance if either has hunger > 80,
/// fear > 60, or both are Hotheaded).
pub fn should_escalate_spar_to_quarrel(
    prof_a: &AllyProfile,
    needs_a: &AllyNeeds,
    prof_b: &AllyProfile,
    needs_b: &AllyNeeds,
    rng_state: &mut u64,
) -> bool {
    let volatile = needs_a.hunger > 80.0
        || needs_b.hunger > 80.0
        || needs_a.fear > 60.0
        || needs_b.fear > 60.0
        || (prof_a.has_trait(PersonalityTrait::Hotheaded)
            && prof_b.has_trait(PersonalityTrait::Hotheaded));

    volatile && next_unit_f32(rng_state) < QUARREL_ESCALATION_CHANCE
}

/// Applies non-lethal sparring damage (health never drops below 1.0).
#[inline]
pub fn apply_non_lethal_spar_hit(health: &mut f32, damage: f32) {
    *health = (*health - damage.max(0.0)).max(1.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ally::Rarity;

    #[test]
    fn test_training_outcome_distribution_10000_rolls() {
        let mut seed = 0x504D_555F_2026_1010;
        let mut no_gain = 0u32;
        let mut normal = 0u32;
        let mut great = 0u32;
        for _ in 0..10_000 {
            match TrainRollKind::roll(&mut seed) {
                TrainRollKind::NoGain => no_gain += 1,
                TrainRollKind::Normal => normal += 1,
                TrainRollKind::GreatSession => great += 1,
            }
        }
        assert!((no_gain as f32 / 10_000.0 - 0.15).abs() < 0.03);
        assert!((normal as f32 / 10_000.0 - 0.70).abs() < 0.03);
        assert!((great as f32 / 10_000.0 - 0.15).abs() < 0.03);
    }

    #[test]
    fn test_skill_learning_improving_combining_and_cap_at_5() {
        let mut seed = 77u64;
        let mut prof = AllyProfile::roll(&mut seed, Rarity::Common);
        prof.class = AllyClass::Fighter;
        let mut needs = AllyNeeds::default();

        // Raise BasicSwordsmanship and BasicShield to Lv 2 (each level takes 3 sessions; alternating = 12 sessions)
        for _ in 0..12 {
            needs.fatigue = 0.0;
            let _ = resolve_training_session(&mut prof, &mut needs, 1.0, 0.0, &mut seed);
        }
        assert_eq!(prof.passives.level(PassiveSkill::BasicSwordsmanship), 2);
        assert_eq!(prof.passives.level(PassiveSkill::BasicShield), 2);
        assert_eq!(prof.passives.level(PassiveSkill::BasicSwordShield), 0);

        // Next 5 alternating sessions build progress until one reaches 3 progress -> combines into BasicSwordShield Lv 3!
        let mut combined_event = None;
        for _ in 0..6 {
            needs.fatigue = 0.0;
            let out = resolve_training_session(&mut prof, &mut needs, 1.0, 0.0, &mut seed);
            if matches!(out.skill_event, SkillGainEvent::CombinedSwordShield(3)) {
                combined_event = Some(out.skill_event);
                break;
            }
        }
        assert_eq!(combined_event, Some(SkillGainEvent::CombinedSwordShield(3)));
        // Sources stay at level 2 while combined skill starts at level 3!
        assert_eq!(prof.passives.level(PassiveSkill::BasicSwordsmanship), 2);
        assert_eq!(prof.passives.level(PassiveSkill::BasicShield), 2);
        assert_eq!(prof.passives.level(PassiveSkill::BasicSwordShield), 3);

        // Further training raises BasicSwordShield up to cap 5 and never exceeds 5
        for _ in 0..15 {
            needs.fatigue = 0.0;
            let _ = resolve_training_session(&mut prof, &mut needs, 1.0, 0.0, &mut seed);
        }
        assert_eq!(
            prof.passives.level(PassiveSkill::BasicSwordShield),
            MAX_PASSIVE_LEVEL
        );
    }

    #[test]
    fn test_training_death_chance_gating_and_sparring_rules() {
        let mut seed = 1u64;
        let mut prof = AllyProfile::roll(&mut seed, Rarity::Common);
        let mut needs = AllyNeeds::default();

        // When healthy (hp = 1.0, fatigue = 0.0), even 100% death_chance never kills the ally
        for _ in 0..50 {
            needs.fatigue = 0.0;
            let out = resolve_training_session(&mut prof, &mut needs, 1.0, 1.0, &mut seed);
            assert!(!out.died);
        }

        // When death_chance == 0.0, even at low HP (0.3) and high fatigue (90), never dies
        for _ in 0..50 {
            needs.fatigue = 90.0;
            let out = resolve_training_session(&mut prof, &mut needs, 0.30, 0.0, &mut seed);
            assert!(!out.died);
        }

        // When low HP (0.30) and death_chance == 1.0, dies on the first GreatSession roll
        let mut died = false;
        for _ in 0..50 {
            let out = resolve_training_session(&mut prof, &mut needs, 0.30, 1.0, &mut seed);
            if out.died {
                died = true;
                break;
            }
        }
        assert!(died);

        // Non-lethal sparring hit never drops health below 1.0
        let mut hp = 5.0;
        apply_non_lethal_spar_hit(&mut hp, 50.0);
        assert_eq!(hp, 1.0);
    }
}
