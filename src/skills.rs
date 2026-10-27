use crate::enemy::Enemy;
use glam::{Vec2, Vec3};

pub const PLAYER_MAX_MANA: f32 = 100.0;
pub const MANA_REGEN_RATE: f32 = 6.0;
pub const MANA_REGEN_PAUSE: f32 = 1.0;

pub const CAST_DURATION: f32 = 0.10;
pub const CAST_SPEED_MULT: f32 = 0.70;
pub const DASH_SKILL_DURATION: f32 = 0.20;

pub const AIM_ASSIST_ANGLE_RAD: f32 = 8.0_f32.to_radians();
pub const AIM_ASSIST_RANGE: f32 = 25.0;

pub const SKILL_COUNT: usize = 9;

#[repr(usize)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SkillId {
    Fireball = 0,
    IceShard = 1,
    HealPulse = 2,
    Shockwave = 3,
    WindDash = 4,
    SteamBurst = 5,
    MeteorSlam = 6,
    FrostDash = 7,
    HolyNova = 8,
}

pub const TIER1_SKILLS: [SkillId; 5] = [
    SkillId::Fireball,
    SkillId::IceShard,
    SkillId::HealPulse,
    SkillId::Shockwave,
    SkillId::WindDash,
];

impl SkillId {
    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }

    #[inline]
    pub fn def(self) -> &'static SkillDef {
        &SKILL_TABLE[self as usize]
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum SkillKind {
    Projectile {
        pierce: u32,
        explosion_radius: f32,
        applies_burn: bool,
        applies_slow: bool,
    },
    HealPulse,
    Shockwave {
        knockback: f32,
    },
    Dash {
        leaves_slow_zone: bool,
    },
    Meteor {
        mark_dist: f32,
        delay: f32,
        knockback: f32,
    },
    HolyNova {
        heal_radius: f32,
        enemy_damage: f32,
        enemy_radius: f32,
        knockback: f32,
    },
}

#[derive(Copy, Clone, Debug)]
pub struct SkillDef {
    pub id: SkillId,
    pub name: &'static str,
    pub short_name: &'static str,
    pub tier: u8,
    pub mana_cost: f32,
    pub cooldown: f32,
    pub kind: SkillKind,
    pub power: f32,
    pub radius: f32,
    pub speed: f32,
    pub duration: f32,
    pub tint: [f32; 3],
}

pub const SKILL_TABLE: [SkillDef; SKILL_COUNT] = [
    SkillDef {
        id: SkillId::Fireball,
        name: "Fireball",
        short_name: "FIRE",
        tier: 1,
        mana_cost: 20.0,
        cooldown: 1.0,
        kind: SkillKind::Projectile {
            pierce: 0,
            explosion_radius: 2.0,
            applies_burn: true,
            applies_slow: false,
        },
        power: 28.0,
        radius: 2.0,
        speed: 20.0,
        duration: 2.0,
        tint: [1.0, 0.42, 0.12],
    },
    SkillDef {
        id: SkillId::IceShard,
        name: "Ice Shard",
        short_name: "ICE",
        tier: 1,
        mana_cost: 15.0,
        cooldown: 0.8,
        kind: SkillKind::Projectile {
            pierce: 1,
            explosion_radius: 0.0,
            applies_burn: false,
            applies_slow: true,
        },
        power: 18.0,
        radius: 0.25,
        speed: 26.0,
        duration: 1.6,
        tint: [0.35, 0.86, 1.0],
    },
    SkillDef {
        id: SkillId::HealPulse,
        name: "Heal Pulse",
        short_name: "HEAL",
        tier: 1,
        mana_cost: 30.0,
        cooldown: 6.0,
        kind: SkillKind::HealPulse,
        power: 30.0,
        radius: 8.0,
        speed: 0.0,
        duration: 0.3,
        tint: [0.28, 0.96, 0.45],
    },
    SkillDef {
        id: SkillId::Shockwave,
        name: "Shockwave",
        short_name: "SHCK",
        tier: 1,
        mana_cost: 25.0,
        cooldown: 4.0,
        kind: SkillKind::Shockwave { knockback: 8.0 },
        power: 20.0,
        radius: 5.0,
        speed: 0.0,
        duration: 0.3,
        tint: [0.98, 0.82, 0.22],
    },
    SkillDef {
        id: SkillId::WindDash,
        name: "Wind Dash",
        short_name: "DASH",
        tier: 1,
        mana_cost: 10.0,
        cooldown: 3.0,
        kind: SkillKind::Dash {
            leaves_slow_zone: false,
        },
        power: 0.0,
        radius: 6.0,
        speed: 6.0 / DASH_SKILL_DURATION,
        duration: DASH_SKILL_DURATION,
        tint: [0.55, 1.0, 0.80],
    },
    SkillDef {
        id: SkillId::SteamBurst,
        name: "Steam Burst",
        short_name: "STMB",
        tier: 2,
        mana_cost: 40.0,
        cooldown: 3.0,
        kind: SkillKind::Projectile {
            pierce: 0,
            explosion_radius: 4.0,
            applies_burn: false,
            applies_slow: true,
        },
        power: 55.0,
        radius: 4.0,
        speed: 16.0,
        duration: 2.0,
        tint: [0.75, 0.90, 1.0],
    },
    SkillDef {
        id: SkillId::MeteorSlam,
        name: "Meteor Slam",
        short_name: "METR",
        tier: 2,
        mana_cost: 45.0,
        cooldown: 6.0,
        kind: SkillKind::Meteor {
            mark_dist: 10.0,
            delay: 0.6,
            knockback: 10.0,
        },
        power: 70.0,
        radius: 6.0,
        speed: 0.0,
        duration: 0.6,
        tint: [1.0, 0.26, 0.10],
    },
    SkillDef {
        id: SkillId::FrostDash,
        name: "Frost Dash",
        short_name: "FRST",
        tier: 2,
        mana_cost: 25.0,
        cooldown: 4.0,
        kind: SkillKind::Dash {
            leaves_slow_zone: true,
        },
        power: 0.0,
        radius: 8.0,
        speed: 8.0 / DASH_SKILL_DURATION,
        duration: DASH_SKILL_DURATION,
        tint: [0.40, 0.94, 1.0],
    },
    SkillDef {
        id: SkillId::HolyNova,
        name: "Holy Nova",
        short_name: "NOVA",
        tier: 2,
        mana_cost: 50.0,
        cooldown: 8.0,
        kind: SkillKind::HolyNova {
            heal_radius: 8.0,
            enemy_damage: 30.0,
            enemy_radius: 7.0,
            knockback: 6.0,
        },
        power: 40.0,
        radius: 8.0,
        speed: 0.0,
        duration: 0.3,
        tint: [1.0, 0.95, 0.48],
    },
];

/// Damage and heal multiplier for skill level `1..=5`: `1 + 0.15 * (level - 1)`.
#[inline]
pub fn level_multiplier(level: u8) -> f32 {
    let clamped = level.clamp(1, 5);
    1.0 + 0.15 * f32::from(clamped - 1)
}

/// Computes horizontal aim direction along `cam_forward_xz` with 8-degree aim assist within 25 units.
pub fn compute_aim_direction_xz(
    player_pos: Vec3,
    cam_forward_xz: Vec3,
    enemies: &[Enemy],
) -> Vec2 {
    let base = Vec2::new(cam_forward_xz.x, cam_forward_xz.z).normalize_or_zero();
    let aim_dir = if base.length_squared() > 1e-5 {
        base
    } else {
        Vec2::new(0.0, -1.0)
    };

    let cos_limit = AIM_ASSIST_ANGLE_RAD.cos();
    let p_xz = Vec2::new(player_pos.x, player_pos.z);
    let mut best: Option<(Vec2, f32)> = None;

    for e in enemies {
        if !e.is_alive() {
            continue;
        }
        let to_e = Vec2::new(e.pos.x, e.pos.z) - p_xz;
        let dist = to_e.length();
        if dist <= 1e-4 || dist > AIM_ASSIST_RANGE {
            continue;
        }
        let dir = to_e / dist;
        if aim_dir.dot(dir) >= cos_limit && best.map_or(true, |(_, bd)| dist < bd) {
            best = Some((dir, dist));
        }
    }

    best.map(|(d, _)| d).unwrap_or(aim_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level_multiplier_formula() {
        assert!((level_multiplier(1) - 1.00).abs() < 1e-6);
        assert!((level_multiplier(2) - 1.15).abs() < 1e-6);
        assert!((level_multiplier(3) - 1.30).abs() < 1e-6);
        assert!((level_multiplier(5) - 1.60).abs() < 1e-6);
        assert!((level_multiplier(9) - 1.60).abs() < 1e-6);
    }

    #[test]
    fn test_aim_assist_angle() {
        let e_in_cone = Enemy::new(1, Vec3::new(1.0, 0.0, -10.0), 1, 1, 0); // ~5.7 deg off -Z
        let e_outside = Enemy::new(2, Vec3::new(5.0, 0.0, -10.0), 2, 1, 0); // ~26.5 deg off -Z

        let dir1 = compute_aim_direction_xz(Vec3::ZERO, Vec3::NEG_Z, &[e_in_cone]);
        let expected = Vec2::new(1.0, -10.0).normalize();
        assert!(dir1.distance(expected) < 1e-4);

        let dir2 = compute_aim_direction_xz(Vec3::ZERO, Vec3::NEG_Z, &[e_outside]);
        assert!(dir2.distance(Vec2::new(0.0, -1.0)) < 1e-4);
    }
}
