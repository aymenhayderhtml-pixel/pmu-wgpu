use crate::ally::next_unit_f32;
use crate::frustum::Frustum;
use crate::life::AllyNeeds;
use crate::traits::{AllyClass, AllyProfile, PersonalityTrait};
use crate::world::InstanceData;
use glam::{Mat4, Quat, Vec2, Vec3};

pub const CAMP_CENTER: Vec3 = Vec3::new(-20.0, 0.0, -20.0);
pub const CAMP_HALF_SIZE: f32 = 6.0;
pub const CAMP_PHASE_DURATION: f32 = 45.0;
pub const ACTIVITY_DECIDE_INTERVAL: f32 = 2.0;

pub const DURATION_EAT: f32 = 4.0;
pub const DURATION_REST: f32 = 12.0;
pub const DURATION_TRAIN: f32 = 10.0;
pub const DURATION_SPAR: f32 = 8.0;
pub const DURATION_IDLE: f32 = 3.0;

pub const REST_HEAL_FRACTION_PER_SEC: f32 = 0.06;

#[repr(usize)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CampObjectKind {
    TrainingDummy = 0,
    ArcheryTarget = 1,
    FoodPot = 2,
    RestMat = 3,
    FlagPost = 4,
}

pub const CAMP_OBJECT_COUNT: usize = 5;

#[derive(Copy, Clone, Debug)]
pub struct CampObject {
    pub kind: CampObjectKind,
    pub pos: Vec3,
    pub interaction_pos: Vec3,
    pub capacity: usize,
    pub users: usize,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CampActivity {
    Idle,
    Eat,
    Rest,
    Train,
    Spar,
    Quarrel,
}

impl CampActivity {
    pub fn duration(self) -> f32 {
        match self {
            Self::Eat => DURATION_EAT,
            Self::Rest => DURATION_REST,
            Self::Train => DURATION_TRAIN,
            Self::Spar => DURATION_SPAR,
            Self::Idle => DURATION_IDLE,
            Self::Quarrel => 6.0,
        }
    }

    pub fn icon_letter(self) -> &'static str {
        let _ = self.label();
        match self {
            Self::Eat => "E",
            Self::Rest => "R",
            Self::Train => "T",
            Self::Spar => "S",
            Self::Idle => "Z",
            Self::Quarrel => "Q",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Eat => "EAT",
            Self::Rest => "REST",
            Self::Train => "TRAIN",
            Self::Spar => "SPAR",
            Self::Idle => "IDLE",
            Self::Quarrel => "QUARREL",
        }
    }

    pub fn object_kind(self, class: AllyClass) -> CampObjectKind {
        match self {
            Self::Eat => CampObjectKind::FoodPot,
            Self::Rest => CampObjectKind::RestMat,
            Self::Train => match class {
                AllyClass::Fighter => CampObjectKind::TrainingDummy,
                AllyClass::Archer => CampObjectKind::ArcheryTarget,
            },
            Self::Spar | Self::Idle | Self::Quarrel => CampObjectKind::FlagPost,
        }
    }
}

/// Checks if a point `(x, z)` expanded by `margin` overlaps the reserved cube-free Camp zone.
#[inline]
pub fn overlaps_camp_zone(x: f32, z: f32, margin: f32) -> bool {
    let half = CAMP_HALF_SIZE + margin;
    (x - CAMP_CENTER.x).abs() <= half && (z - CAMP_CENTER.z).abs() <= half
}

pub struct CampZone {
    pub objects: [CampObject; CAMP_OBJECT_COUNT],
}

impl Default for CampZone {
    fn default() -> Self {
        Self::new()
    }
}

impl CampZone {
    pub fn new() -> Self {
        let c = CAMP_CENTER;
        let objects = [
            CampObject {
                kind: CampObjectKind::TrainingDummy,
                pos: c + Vec3::new(-3.8, 0.0, -3.5),
                interaction_pos: c + Vec3::new(-2.6, 0.0, -3.5),
                capacity: 2,
                users: 0,
            },
            CampObject {
                kind: CampObjectKind::ArcheryTarget,
                pos: c + Vec3::new(3.8, 0.0, -3.5),
                interaction_pos: c + Vec3::new(3.8, 0.0, -0.5),
                capacity: 2,
                users: 0,
            },
            CampObject {
                kind: CampObjectKind::FoodPot,
                pos: c + Vec3::new(-3.5, 0.0, 3.2),
                interaction_pos: c + Vec3::new(-2.2, 0.0, 3.2),
                capacity: 3,
                users: 0,
            },
            CampObject {
                kind: CampObjectKind::RestMat,
                pos: c + Vec3::new(3.5, 0.0, 3.2),
                interaction_pos: c + Vec3::new(3.5, 0.0, 3.2),
                capacity: 2,
                users: 0,
            },
            CampObject {
                kind: CampObjectKind::FlagPost,
                pos: c,
                interaction_pos: c + Vec3::new(0.0, 0.0, 1.5),
                capacity: 2,
                users: 0,
            },
        ];
        Self { objects }
    }

    pub fn reset_users(&mut self) {
        for obj in &mut self.objects {
            let _ = obj.kind;
            obj.users = 0;
        }
    }

    pub fn has_free_slot(&self, kind: CampObjectKind) -> bool {
        let obj = &self.objects[kind as usize];
        obj.users < obj.capacity
    }

    pub fn try_reserve(&mut self, kind: CampObjectKind) -> Option<Vec3> {
        let obj = &mut self.objects[kind as usize];
        if obj.users < obj.capacity {
            let slot_idx = obj.users;
            obj.users += 1;
            let offset = Vec3::new((slot_idx as f32 - 0.5) * 0.9, 0.0, 0.0);
            Some(obj.interaction_pos + offset)
        } else {
            None
        }
    }

    pub fn append_visible_cube_instances<F>(&self, frustum: &Frustum, mut push_fn: F)
    where
        F: FnMut(InstanceData),
    {
        let c_min = Vec3::splat(-0.5);
        let c_max = Vec3::splat(0.5);
        let mut emit = |scale: Vec3, pos: Vec3, color: [f32; 4]| {
            let model = Mat4::from_scale_rotation_translation(scale, Quat::IDENTITY, pos);
            if frustum.intersects_local_aabb(model, c_min, c_max) {
                push_fn(InstanceData { model, color });
            }
        };

        // 1. Training Dummy (brown post + head)
        let d_pos = self.objects[CampObjectKind::TrainingDummy as usize].pos;
        emit(
            Vec3::new(0.28, 1.3, 0.28),
            d_pos + Vec3::new(0.0, 0.65, 0.0),
            [0.48, 0.30, 0.16, 1.0],
        );
        emit(
            Vec3::new(0.50, 0.50, 0.50),
            d_pos + Vec3::new(0.0, 1.55, 0.0),
            [0.65, 0.44, 0.26, 1.0],
        );

        // 2. Archery Target (red outer + white inner disc-like flat wide cubes)
        let a_pos = self.objects[CampObjectKind::ArcheryTarget as usize].pos;
        emit(
            Vec3::new(1.2, 1.2, 0.16),
            a_pos + Vec3::new(0.0, 1.1, 0.0),
            [0.90, 0.18, 0.18, 1.0],
        );
        emit(
            Vec3::new(0.6, 0.6, 0.20),
            a_pos + Vec3::new(0.0, 1.1, 0.02),
            [0.96, 0.96, 0.96, 1.0],
        );

        // 3. Food Pot (dark wide base + glowing orange soup top)
        let p_pos = self.objects[CampObjectKind::FoodPot as usize].pos;
        emit(
            Vec3::new(1.1, 0.65, 1.1),
            p_pos + Vec3::new(0.0, 0.325, 0.0),
            [0.16, 0.17, 0.20, 1.0],
        );
        emit(
            Vec3::new(0.95, 0.12, 0.95),
            p_pos + Vec3::new(0.0, 0.68, 0.0),
            [1.0, 0.52, 0.12, 1.4],
        );

        // 4. Rest Mat (flat blue mat)
        let r_pos = self.objects[CampObjectKind::RestMat as usize].pos;
        emit(
            Vec3::new(2.2, 0.08, 1.6),
            r_pos + Vec3::new(0.0, 0.04, 0.0),
            [0.22, 0.48, 0.88, 1.0],
        );

        // 5. Camp Flag Post (pole + golden banner)
        let f_pos = self.objects[CampObjectKind::FlagPost as usize].pos;
        emit(
            Vec3::new(0.14, 2.8, 0.14),
            f_pos + Vec3::new(0.0, 1.4, 0.0),
            [0.75, 0.75, 0.78, 1.0],
        );
        emit(
            Vec3::new(0.75, 0.45, 0.08),
            f_pos + Vec3::new(0.38, 2.45, 0.0),
            [1.0, 0.84, 0.22, 1.3],
        );
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ActivityContext {
    pub hp_fraction: f32,
    pub food_stock: u32,
    pub pot_free: bool,
    pub rest_free: bool,
    pub train_free: bool,
    pub spar_partner_available: bool,
}

/// Pure scoring function for camp activities (`Eat`, `Rest`, `Train`, `Spar`, `Idle`).
pub fn score_camp_activities(
    profile: &AllyProfile,
    needs: &AllyNeeds,
    ctx: &ActivityContext,
) -> [(CampActivity, f32); 5] {
    // 1. Eat: needs food_stock > 0 and free pot slot; score = hunger/100 * 1.5, doubled above 70
    let eat_score = if ctx.food_stock > 0 && ctx.pot_free && needs.hunger > 5.0 {
        let base = (needs.hunger / 100.0) * 1.5;
        if needs.hunger > 70.0 {
            base * 2.0
        } else {
            base
        }
    } else {
        -1.0
    };

    // 2. Rest: score = (1 - hp_fraction) * 1.2 + fear/100 * 0.5 (+0.15 if Cautious)
    let rest_score = if ctx.rest_free && (ctx.hp_fraction < 0.98 || needs.fatigue > 15.0 || needs.fear > 10.0) {
        let mut s = (1.0 - ctx.hp_fraction.clamp(0.0, 1.0)) * 1.2 + (needs.fear / 100.0) * 0.5;
        if profile.has_trait(PersonalityTrait::Cautious) {
            s += 0.15;
        }
        s
    } else {
        0.05
    };

    // 3. Train: not possible when hunger > 80 or health < 50%
    let train_score = if ctx.train_free && needs.hunger <= 80.0 && ctx.hp_fraction >= 0.50 {
        let trait_mod = if profile.has_trait(PersonalityTrait::Diligent) {
            0.30
        } else if profile.has_trait(PersonalityTrait::Lazy) {
            -0.30
        } else {
            0.0
        };
        let level_factor = ((10u8.saturating_sub(profile.level)) as f32 / 9.0) * 0.10;
        let fatigue_pen = (needs.fatigue / 100.0) * 0.5;
        0.50 + trait_mod + level_factor - fatigue_pen
    } else {
        -1.0
    };

    // 4. Spar: needs another willing ally, not when health < 60% or hunger > 80
    let spar_score = if ctx.spar_partner_available && ctx.hp_fraction >= 0.60 && needs.hunger <= 80.0 {
        let trait_mod = if profile.has_trait(PersonalityTrait::Hotheaded) {
            0.25
        } else if profile.has_trait(PersonalityTrait::Calm) {
            -0.15
        } else {
            0.0
        };
        0.40 + trait_mod
    } else {
        -1.0
    };

    // 5. Idle: base 0.20 around the camp flag
    let idle_score = 0.20;

    [
        (CampActivity::Eat, eat_score),
        (CampActivity::Rest, rest_score),
        (CampActivity::Train, train_score),
        (CampActivity::Spar, spar_score),
        (CampActivity::Idle, idle_score),
    ]
}

/// Picks the highest-scoring activity after adding seeded random noise (`0.0..=0.15`) to valid options.
pub fn choose_camp_activity(
    profile: &AllyProfile,
    needs: &AllyNeeds,
    ctx: &ActivityContext,
    rng_state: &mut u64,
) -> CampActivity {
    if needs.scolded_timer > 0.0 {
        return CampActivity::Idle;
    }
    let scores = score_camp_activities(profile, needs, ctx);
    let mut best_act = CampActivity::Idle;
    let mut best_val = f32::NEG_INFINITY;

    for &(act, base_score) in &scores {
        if base_score < 0.0 {
            continue;
        }
        let noise = next_unit_f32(rng_state) * 0.15;
        let total = base_score + noise;
        if total > best_val {
            best_val = total;
            best_act = act;
        }
    }
    best_act
}

#[inline]
pub fn camp_idle_wander_point(rng_state: &mut u64) -> Vec3 {
    let angle = next_unit_f32(rng_state) * std::f32::consts::TAU;
    let r = 1.2 + next_unit_f32(rng_state) * 2.5;
    let offset = Vec2::new(angle.cos() * r, angle.sin() * r);
    CAMP_CENTER + Vec3::new(offset.x, 0.0, offset.y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ally::Rarity;

    #[test]
    fn test_activity_scoring_and_decisions_for_six_representative_allies() {
        let mut seed = 12345u64;
        let base_ctx = ActivityContext {
            hp_fraction: 1.0,
            food_stock: 5,
            pot_free: true,
            rest_free: true,
            train_free: true,
            spar_partner_available: true,
        };

        // 1. Starving ally (hunger = 85) -> always chooses Eat, and Train is strictly blocked (< 0)
        let mut p1 = AllyProfile::roll(&mut seed, Rarity::Common);
        p1.traits = [PersonalityTrait::Diligent, PersonalityTrait::Brave];
        let mut n1 = AllyNeeds::default();
        n1.hunger = 85.0;
        let s1 = score_camp_activities(&p1, &n1, &base_ctx);
        assert!(s1[2].1 < 0.0, "Train must be disallowed when hunger > 80");
        assert_eq!(choose_camp_activity(&p1, &n1, &base_ctx, &mut seed), CampActivity::Eat);

        // 2. Heavily wounded ally (hp = 35%) -> Train is blocked (< 50% HP) and Rest is chosen
        let mut n2 = AllyNeeds::default();
        n2.fear = 40.0;
        let mut low_hp_ctx = base_ctx;
        low_hp_hp(&mut low_hp_ctx, 0.35);
        let s2 = score_camp_activities(&p1, &n2, &low_hp_ctx);
        assert!(s2[2].1 < 0.0, "Train must be disallowed when health < 50%");
        assert_eq!(choose_camp_activity(&p1, &n2, &low_hp_ctx, &mut seed), CampActivity::Rest);

        // 3. Healthy Diligent ally with 0 fatigue -> chooses Train
        let n3 = AllyNeeds::default();
        let mut no_spar_ctx = base_ctx;
        no_spar_ctx.spar_partner_available = false;
        assert_eq!(choose_camp_activity(&p1, &n3, &no_spar_ctx, &mut seed), CampActivity::Train);

        // 4. Exhausted Lazy ally (fatigue = 90) -> avoids Train, chooses Idle
        let mut p4 = AllyProfile::roll(&mut seed, Rarity::Common);
        p4.traits = [PersonalityTrait::Lazy, PersonalityTrait::Calm];
        let mut n4 = AllyNeeds::default();
        n4.fatigue = 90.0;
        assert_eq!(choose_camp_activity(&p4, &n4, &no_spar_ctx, &mut seed), CampActivity::Idle);

        // 5. Healthy Hotheaded ally with spar partner available -> chooses Spar
        let mut p5 = AllyProfile::roll(&mut seed, Rarity::Rare);
        p5.traits = [PersonalityTrait::Hotheaded, PersonalityTrait::Lazy];
        let n5 = AllyNeeds::default();
        assert_eq!(choose_camp_activity(&p5, &n5, &base_ctx, &mut seed), CampActivity::Spar);

        // 6. Scolded ally -> always forced to Idle
        let mut n6 = AllyNeeds::default();
        n6.scolded_timer = 5.0;
        assert_eq!(choose_camp_activity(&p1, &n6, &base_ctx, &mut seed), CampActivity::Idle);
    }

    fn low_hp_hp(ctx: &mut ActivityContext, hp: f32) {
        ctx.hp_fraction = hp;
    }

    #[test]
    fn test_camp_object_capacity_limits() {
        let mut camp = CampZone::new();
        assert_eq!(camp.objects[CampObjectKind::TrainingDummy as usize].kind, CampObjectKind::TrainingDummy);
        assert_eq!(CampActivity::Train.label(), "TRAIN");
        // TrainingDummy has capacity 2
        assert!(camp.try_reserve(CampObjectKind::TrainingDummy).is_some());
        assert!(camp.try_reserve(CampObjectKind::TrainingDummy).is_some());
        assert!(camp.try_reserve(CampObjectKind::TrainingDummy).is_none());

        // FoodPot has capacity 3
        assert!(camp.try_reserve(CampObjectKind::FoodPot).is_some());
        assert!(camp.try_reserve(CampObjectKind::FoodPot).is_some());
        assert!(camp.try_reserve(CampObjectKind::FoodPot).is_some());
        assert!(camp.try_reserve(CampObjectKind::FoodPot).is_none());
    }
}
