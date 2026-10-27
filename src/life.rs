use crate::traits::{AllyProfile, PersonalityTrait};

pub const MAX_NEED_VALUE: f32 = 100.0;
pub const HUNGER_RISE_PER_SEC: f32 = 0.5;
pub const HUNGER_PENALTY_THRESHOLD: f32 = 70.0;
pub const HUNGER_STARVING_THRESHOLD: f32 = 90.0;
pub const HUNGER_DAMAGE_MULT: f32 = 0.80;
pub const HUNGER_SPEED_MULT: f32 = 0.90;
pub const EAT_HUNGER_REDUCTION: f32 = 60.0;

pub const START_FOOD_STOCK: u32 = 6;
pub const MAX_FOOD_STOCK: u32 = 20;
pub const FOOD_DROP_CHANCE: f32 = 0.15;

pub const FEAR_LOW_HP_RATE: f32 = 8.0;
pub const FEAR_CROWD_ENEMY_RATE: f32 = 4.0;
pub const FEAR_ALLY_DOWN_SPIKE: f32 = 25.0;
pub const FEAR_DECAY_SAFE_RATE: f32 = 3.0;

pub const FEAR_THRESHOLD_BRAVE: f32 = 80.0;
pub const FEAR_THRESHOLD_NORMAL: f32 = 60.0;
pub const FEAR_THRESHOLD_CAUTIOUS: f32 = 45.0;
pub const FEAR_FREEZE_OFFSET: f32 = 20.0;

pub const FLEE_MIN_DURATION: f32 = 2.0;
pub const FREEZE_DURATION: f32 = 1.0;

pub const FATIGUE_FROM_TRAIN_OR_SPAR: f32 = 25.0;
pub const FATIGUE_REST_DECAY_PER_SEC: f32 = 8.0;
pub const SCOLDED_DURATION: f32 = 10.0;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EmotionLabel {
    Calm,
    Hungry,
    Scared,
    Starving,
    Panicked,
}

impl EmotionLabel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Calm => "Calm",
            Self::Hungry => "Hungry",
            Self::Scared => "Scared",
            Self::Starving => "Starving",
            Self::Panicked => "Panicked",
        }
    }

    pub fn color(self) -> [f32; 4] {
        match self {
            Self::Calm => [0.55, 0.95, 0.65, 1.0],
            Self::Hungry => [1.0, 0.82, 0.30, 1.0],
            Self::Scared => [0.75, 0.60, 1.0, 1.0],
            Self::Starving => [1.0, 0.45, 0.20, 1.0],
            Self::Panicked => [1.0, 0.22, 0.28, 1.0],
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FearReaction {
    None,
    Flee,
    FreezeThenFlee,
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct AllyNeeds {
    pub hunger: f32,
    pub fear: f32,
    pub fatigue: f32,
    pub flee_timer: f32,
    pub freeze_timer: f32,
    pub scolded_timer: f32,
    pub starving_at_100_timer: f32,
}

impl AllyNeeds {
    pub fn fear_threshold(&self, profile: &AllyProfile) -> f32 {
        if profile.has_trait(PersonalityTrait::Brave) {
            FEAR_THRESHOLD_BRAVE
        } else if profile.has_trait(PersonalityTrait::Cautious) {
            FEAR_THRESHOLD_CAUTIOUS
        } else {
            FEAR_THRESHOLD_NORMAL
        }
    }

    pub fn eat_meal(&mut self) {
        self.hunger = (self.hunger - EAT_HUNGER_REDUCTION).max(0.0);
        self.starving_at_100_timer = 0.0;
    }

    pub fn add_ally_down_fear_spike(&mut self) {
        self.fear = (self.fear + FEAR_ALLY_DOWN_SPIKE).min(MAX_NEED_VALUE);
    }

    pub fn damage_multiplier(&self) -> f32 {
        if self.hunger > HUNGER_PENALTY_THRESHOLD {
            HUNGER_DAMAGE_MULT
        } else {
            1.0
        }
    }

    pub fn speed_multiplier(&self) -> f32 {
        if self.hunger > HUNGER_PENALTY_THRESHOLD {
            HUNGER_SPEED_MULT
        } else {
            1.0
        }
    }

    pub fn refuses_combat(&self) -> bool {
        self.hunger > HUNGER_STARVING_THRESHOLD
    }

    /// Updates hunger, fear, and reaction timers for `dt`.
    pub fn update(
        &mut self,
        dt: f32,
        is_down: bool,
        hp_ratio: f32,
        enemies_within_6m: usize,
        any_enemy_within_12m: bool,
        profile: &AllyProfile,
    ) -> FearReaction {
        self.scolded_timer = (self.scolded_timer - dt).max(0.0);
        if is_down {
            self.flee_timer = 0.0;
            self.freeze_timer = 0.0;
            return FearReaction::None;
        }

        let h_rate = HUNGER_RISE_PER_SEC * profile.hunger_rate_multiplier();
        self.hunger = (self.hunger + h_rate * dt).clamp(0.0, MAX_NEED_VALUE);
        if self.hunger >= 99.99 {
            self.starving_at_100_timer += dt;
        } else {
            self.starving_at_100_timer = 0.0;
        }

        let mut fear_delta = 0.0;
        if hp_ratio < 0.40 {
            fear_delta += FEAR_LOW_HP_RATE * dt;
        }
        if enemies_within_6m > 2 {
            fear_delta += ((enemies_within_6m - 2) as f32) * FEAR_CROWD_ENEMY_RATE * dt;
        }
        if !any_enemy_within_12m && fear_delta == 0.0 {
            fear_delta -= FEAR_DECAY_SAFE_RATE * dt;
        }
        self.fear = (self.fear + fear_delta).clamp(0.0, MAX_NEED_VALUE);

        if self.freeze_timer > 0.0 {
            self.freeze_timer = (self.freeze_timer - dt).max(0.0);
            if self.freeze_timer <= 0.0 {
                self.flee_timer = FLEE_MIN_DURATION;
                return FearReaction::Flee;
            }
            return FearReaction::FreezeThenFlee;
        }

        if self.flee_timer > 0.0 {
            self.flee_timer = (self.flee_timer - dt).max(0.0);
        }

        let threshold = self.fear_threshold(profile);
        if self.fear > threshold + FEAR_FREEZE_OFFSET && self.flee_timer <= 0.0 {
            self.freeze_timer = FREEZE_DURATION;
            self.flee_timer = FLEE_MIN_DURATION;
            FearReaction::FreezeThenFlee
        } else if self.fear > threshold {
            self.flee_timer = self.flee_timer.max(FLEE_MIN_DURATION);
            FearReaction::Flee
        } else if self.flee_timer > 0.0 {
            FearReaction::Flee
        } else {
            FearReaction::None
        }
    }
}

pub fn emotion_label(needs: &AllyNeeds, profile: &AllyProfile) -> EmotionLabel {
    let thresh = needs.fear_threshold(profile);
    if needs.fear > thresh + FEAR_FREEZE_OFFSET || needs.freeze_timer > 0.0 {
        EmotionLabel::Panicked
    } else if needs.hunger > HUNGER_STARVING_THRESHOLD {
        EmotionLabel::Starving
    } else if needs.fear > thresh || needs.flee_timer > 0.0 {
        EmotionLabel::Scared
    } else if needs.hunger > HUNGER_PENALTY_THRESHOLD {
        EmotionLabel::Hungry
    } else {
        EmotionLabel::Calm
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ally::Rarity;

    #[test]
    fn test_hunger_and_fear_dynamics_thresholds_flee_freeze_and_emotions() {
        let mut seed = 10u64;
        let mut prof = AllyProfile::roll(&mut seed, Rarity::Common);
        prof.traits = [PersonalityTrait::Cautious, PersonalityTrait::HungryType];

        let mut needs = AllyNeeds::default();
        assert_eq!(needs.fear_threshold(&prof), FEAR_THRESHOLD_CAUTIOUS);
        assert_eq!(emotion_label(&needs, &prof), EmotionLabel::Calm);

        // 10s update with HungryType (0.5 * 1.3 = 0.65/s -> 6.5 hunger) and low HP + 4 enemies nearby
        // fear rises by (8.0 + 2 * 4.0) * 10 = 160 -> clamped to 100 (> 45 + 20 = 65 -> FreezeThenFlee!)
        let react = needs.update(10.0, false, 0.25, 4, true, &prof);
        assert!((needs.hunger - 6.5).abs() < 1e-4);
        assert_eq!(react, FearReaction::FreezeThenFlee);
        assert_eq!(emotion_label(&needs, &prof), EmotionLabel::Panicked);

        // After 1.0s freeze finishes, transitions into 2.0s Flee
        needs.fear = 50.0;
        let react2 = needs.update(1.0, false, 0.80, 0, true, &prof);
        assert_eq!(react2, FearReaction::Flee);
        assert_eq!(emotion_label(&needs, &prof), EmotionLabel::Scared);

        // Starving penalty (> 70 and > 90)
        needs.fear = 0.0;
        needs.flee_timer = 0.0;
        needs.hunger = 75.0;
        assert_eq!(emotion_label(&needs, &prof), EmotionLabel::Hungry);
        assert!((needs.damage_multiplier() - 0.80).abs() < 1e-5);
        needs.hunger = 95.0;
        assert_eq!(emotion_label(&needs, &prof), EmotionLabel::Starving);
        assert!(needs.refuses_combat());
    }
}
