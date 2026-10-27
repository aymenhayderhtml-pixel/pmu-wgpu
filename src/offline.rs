use crate::ally::Ally;
use crate::camp::{choose_camp_activity, ActivityContext, CampActivity};
use crate::training::{
    apply_non_lethal_spar_hit, resolve_training_session, SkillGainEvent, TrainRollKind,
    SPAR_XP_GAIN,
};
use crate::traits::{AllyClass, PassiveSkill};
use std::fmt::Write;

pub const MAX_OFFLINE_SECONDS: u64 = 8 * 3600;
pub const OFFLINE_TICK_SECONDS: u64 = 10 * 60;
pub const MIN_OFFLINE_REPORT_SECONDS: u64 = 6 * 60;
pub const OFFLINE_HUNGER_PER_TICK: f32 = 20.0;

#[derive(Clone, Debug)]
pub struct AllyOfflineRow {
    #[cfg(test)]
    pub ally_id: u32,
    pub name: &'static str,
    pub class: AllyClass,
    pub stars: u8,
    pub start_level: u8,
    pub end_level: u8,
    pub xp_gained: u32,
    pub skill_notes: Vec<String>,
    pub sessions_trained: u32,
    pub sessions_nothing: u32,
    pub pain_points_gained: u32,
    pub end_hunger: f32,
    pub died: bool,
}

impl AllyOfflineRow {
    pub fn did_nothing(&self) -> bool {
        !self.died
            && self.end_level == self.start_level
            && self.xp_gained == 0
            && self.skill_notes.is_empty()
            && self.sessions_trained == 0
            && self.pain_points_gained == 0
    }
}

#[derive(Clone, Debug)]
pub struct OfflineReport {
    pub away_seconds: u64,
    pub simulated_seconds: u64,
    pub ticks: u32,
    pub food_consumed: u32,
    pub food_remaining: u32,
    pub rows: Vec<AllyOfflineRow>,
}

/// Computes offline camp progress between `saved_unix` and `now_unix` (capped at 8 hours, in 10m ticks).
/// Returns `None` if `allies` is empty, clock went backward, or elapsed time is `< 6` minutes.
pub fn simulate_offline_progress(
    allies: &mut Vec<Ally>,
    food_stock: &mut u32,
    rng_state: &mut u64,
    saved_unix: u64,
    now_unix: u64,
    death_chance: f32,
) -> Option<OfflineReport> {
    if allies.is_empty() || now_unix <= saved_unix {
        return None;
    }
    let away_seconds = now_unix - saved_unix;
    if away_seconds < MIN_OFFLINE_REPORT_SECONDS {
        return None;
    }

    let simulated_seconds = away_seconds.min(MAX_OFFLINE_SECONDS);
    let ticks = (simulated_seconds / OFFLINE_TICK_SECONDS).max(1) as u32;
    let mut food_consumed = 0u32;

    let mut rows: Vec<AllyOfflineRow> = allies
        .iter()
        .map(|a| AllyOfflineRow {
            #[cfg(test)]
            ally_id: a.id,
            name: a.name,
            class: a.profile.class,
            stars: a.profile.stars,
            start_level: a.profile.level,
            end_level: a.profile.level,
            xp_gained: 0,
            skill_notes: Vec::new(),
            sessions_trained: 0,
            sessions_nothing: 0,
            pain_points_gained: 0,
            end_hunger: a.needs.hunger,
            died: false,
        })
        .collect();

    // Revive any Down ally to 50% health at the start of the first tick
    for ally in allies.iter_mut() {
        if ally.is_down() {
            ally.revive_half_health();
        }
    }

    let mut dead_flags = vec![false; allies.len()];

    for _ in 0..ticks {
        let healthy_count = allies
            .iter()
            .enumerate()
            .filter(|&(idx, a)| {
                !dead_flags[idx]
                    && a.is_living()
                    && (a.health / a.max_health.max(1.0)) >= 0.60
                    && a.needs.hunger <= 80.0
            })
            .count();
        let spar_available = healthy_count >= 2;

        for idx in 0..allies.len() {
            if dead_flags[idx] {
                continue;
            }
            let ally = &mut allies[idx];
            let row = &mut rows[idx];

            // 1. Eat if hunger > 60 and food_stock > 0
            if ally.needs.hunger > 60.0 && *food_stock > 0 {
                *food_stock -= 1;
                food_consumed += 1;
                ally.needs.eat_meal();
            } else if (ally.health / ally.max_health.max(1.0)) < 0.60 {
                // 2. Rest if health < 60%
                let missing = (ally.max_health - ally.health).max(0.0);
                ally.health = ally.max_health;
                ally.needs.fatigue = (ally.needs.fatigue - 50.0).max(0.0);
                let prev_pts = ally.profile.passives.pain_points;
                if let Some(new_lvl) = ally.profile.passives.record_rest_healed(missing) {
                    let mut note = String::with_capacity(32);
                    let _ = write!(
                        &mut note,
                        "{} Lv{}",
                        PassiveSkill::PainTolerance.short_name(),
                        new_lvl
                    );
                    row.skill_notes.push(note);
                }
                row.pain_points_gained += ally.profile.passives.pain_points.saturating_sub(prev_pts);
                ally.refresh_stats_from_profile();
            } else if *food_stock == 0 && ally.needs.hunger >= 99.9 {
                // 3. Starving with no food left: does nothing and loses 5% max HP down to 1
                let drain = ally.max_health * 0.05;
                apply_non_lethal_spar_hit(&mut ally.health, drain);
                ally.profile.passives.record_damage_taken(drain);
            } else {
                // 4. Decide Train, Spar, or Idle with camp scoring
                let ctx = ActivityContext {
                    hp_fraction: ally.health / ally.max_health.max(1.0),
                    food_stock: *food_stock,
                    pot_free: *food_stock > 0,
                    rest_free: true,
                    train_free: true,
                    spar_partner_available: spar_available,
                };
                let act = choose_camp_activity(&ally.profile, &ally.needs, &ctx, rng_state);
                match act {
                    CampActivity::Train => {
                        let hp_frac = ally.health / ally.max_health.max(1.0);
                        let out = resolve_training_session(
                            &mut ally.profile,
                            &mut ally.needs,
                            hp_frac,
                            death_chance,
                            rng_state,
                        );
                        if out.died {
                            dead_flags[idx] = true;
                            row.died = true;
                            continue;
                        }
                        row.sessions_trained += 1;
                        if out.roll == TrainRollKind::NoGain {
                            row.sessions_nothing += 1;
                        }
                        row.xp_gained += out.xp_gained;
                        match out.skill_event {
                            SkillGainEvent::None => {}
                            SkillGainEvent::Learned(sk, lv) | SkillGainEvent::Improved(sk, lv) => {
                                let mut note = String::with_capacity(24);
                                let _ = write!(&mut note, "{} Lv{}", sk.short_name(), lv);
                                row.skill_notes.push(note);
                            }
                            SkillGainEvent::CombinedSwordShield(lv) => {
                                let mut note = String::with_capacity(24);
                                let _ = write!(
                                    &mut note,
                                    "{} Lv{}",
                                    PassiveSkill::BasicSwordShield.short_name(),
                                    lv
                                );
                                row.skill_notes.push(note);
                            }
                        }
                        ally.refresh_stats_from_profile();
                    }
                    CampActivity::Spar => {
                        let _ = ally.profile.add_xp(SPAR_XP_GAIN);
                        row.xp_gained += SPAR_XP_GAIN;
                        ally.needs.fatigue = (ally.needs.fatigue + 25.0).min(100.0);
                        let spar_dmg = ally.max_health * 0.08;
                        apply_non_lethal_spar_hit(&mut ally.health, spar_dmg);
                        ally.profile.passives.record_damage_taken(spar_dmg);
                        ally.refresh_stats_from_profile();
                    }
                    CampActivity::Rest => {
                        ally.needs.fatigue = (ally.needs.fatigue - 25.0).max(0.0);
                    }
                    _ => {}
                }
            }

            // Hunger rises by 20 per tick * trait multiplier (max 100)
            let h_rise = OFFLINE_HUNGER_PER_TICK * ally.profile.hunger_rate_multiplier();
            ally.needs.hunger = (ally.needs.hunger + h_rise).clamp(0.0, 100.0);
            row.end_level = ally.profile.level;
            row.end_hunger = ally.needs.hunger;
        }
    }

    // Remove allies that died during offline training
    let mut keep_idx = 0usize;
    allies.retain(|_| {
        let keep = !dead_flags[keep_idx];
        keep_idx += 1;
        keep
    });

    Some(OfflineReport {
        away_seconds,
        simulated_seconds,
        ticks,
        food_consumed,
        food_remaining: *food_stock,
        rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ally::Rarity;
    use crate::traits::PersonalityTrait;
    use glam::Vec3;

    #[test]
    fn test_offline_report_5min_3h_20h_hunger_death_and_did_nothing() {
        let mut rng = 0x1234_5678_9ABC_DEF0;
        let mut allies = vec![
            Ally::new_with_seed(1, "Rowan", Rarity::Common, Vec3::ZERO, 0.0, 1, 0, &mut rng),
            Ally::new_with_seed(2, "Lyra", Rarity::Rare, Vec3::ZERO, 0.0, 1, 0, &mut rng),
        ];
        allies[0].profile.traits = [PersonalityTrait::Diligent, PersonalityTrait::Calm];
        allies[1].profile.traits = [PersonalityTrait::Lazy, PersonalityTrait::Calm];
        let mut food = 10u32;

        // 1. 5 minutes (300s < 360s) -> no progress and no report
        assert!(simulate_offline_progress(&mut allies, &mut food, &mut rng, 1000, 1300, 0.01).is_none());

        // 2. 3 hours (10,800s = 18 ticks) -> report generated, training & food consumption occur
        let rep_3h = simulate_offline_progress(
            &mut allies,
            &mut food,
            &mut rng,
            1000,
            1000 + 3 * 3600,
            0.0,
        )
        .expect("3h should produce report");
        assert_eq!(rep_3h.rows[0].ally_id, 1);
        assert_eq!(rep_3h.ticks, 18);
        assert!(rep_3h.food_consumed > 0);
        assert!(rep_3h.rows[0].sessions_trained > 0);

        // 3. 20 hours with 0 food -> capped at 8 hours (48 ticks), hunger reaches 100, HP drains to >= 1 (never dies from hunger)
        let mut starving_allies = vec![Ally::new_with_seed(
            3,
            "Bram",
            Rarity::Common,
            Vec3::ZERO,
            0.0,
            1,
            0,
            &mut rng,
        )];
        starving_allies[0].needs.hunger = 100.0;
        let mut zero_food = 0u32;
        let rep_20h = simulate_offline_progress(
            &mut starving_allies,
            &mut zero_food,
            &mut rng,
            1000,
            1000 + 20 * 3600,
            0.0,
        )
        .expect("20h should produce capped report");
        assert_eq!(rep_20h.simulated_seconds, MAX_OFFLINE_SECONDS);
        assert_eq!(rep_20h.ticks, 48);
        assert!(!rep_20h.rows[0].died);
        assert!(rep_20h.rows[0].did_nothing());
        assert!(starving_allies[0].health >= 1.0);

        // 4. Death case when death_chance is forced to 1.0 and fatigue > 70
        let mut doomed = vec![Ally::new_with_seed(
            4,
            "Kael",
            Rarity::Common,
            Vec3::ZERO,
            0.0,
            1,
            0,
            &mut rng,
        )];
        doomed[0].profile.traits = [PersonalityTrait::Diligent, PersonalityTrait::LightEater];
        doomed[0].needs.fatigue = 75.0;
        let mut plenty_food = 20u32;
        let rep_death = simulate_offline_progress(
            &mut doomed,
            &mut plenty_food,
            &mut rng,
            1000,
            1000 + 8 * 3600,
            1.0,
        )
        .expect("death simulation report");
        assert!(rep_death.rows[0].died);
        assert!(doomed.is_empty());
    }
}

