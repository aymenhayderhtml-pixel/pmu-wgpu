use glam::{Vec2, Vec3};

pub const PLAYER_MAX_HEALTH: f32 = 100.0;
pub const SWING_WINDUP: f32 = 0.12;
pub const SWING_ACTIVE: f32 = 0.12;
pub const SWING_COOLDOWN: f32 = 0.45;
pub const SWING_TOTAL_TIME: f32 = SWING_WINDUP + SWING_ACTIVE + SWING_COOLDOWN;
pub const SWING_RANGE: f32 = 2.4;
pub const SWING_ARC_RAD: f32 = 120.0_f32.to_radians();
pub const SWING_DAMAGE: f32 = 25.0;
pub const SWING_SPEED_MULT: f32 = 0.60;

pub const DODGE_DURATION: f32 = 0.35;
pub const DODGE_DISTANCE: f32 = 4.0;
pub const DODGE_SPEED: f32 = DODGE_DISTANCE / DODGE_DURATION;
pub const DODGE_INVULN_TIME: f32 = 0.25;
pub const DODGE_COOLDOWN: f32 = 1.0;

pub const REGEN_DELAY: f32 = 5.0;
pub const REGEN_RATE: f32 = 2.0;

pub const HIT_FLASH_DURATION: f32 = 0.12;
pub const FLOATING_NUMBER_DURATION: f32 = 0.80;
pub const FLOATING_NUMBER_RISE_SPEED: f32 = 1.6;
pub const MAX_FLOATING_NUMBERS: usize = 32;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Faction {
    PlayerSide,
    EnemySide,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ActorId {
    Player,
    Ally(u32),
    Enemy(u32),
}

/// Shared combatant snapshot used by both Enemy AI and Ally Squad AI for perception,
/// target selection, threat tracking, and separation steering without borrow conflicts.
#[derive(Copy, Clone, Debug)]
pub struct CombatantInfo {
    pub id: ActorId,
    pub faction: Faction,
    pub pos: Vec3,
    pub active: bool,
    pub sprinting: bool,
    pub attacking_player_side: bool,
}

/// Shared separation steering force away from all active actors within `sep_radius`.
pub fn compute_separation_force(
    self_id: ActorId,
    self_pos_xz: Vec2,
    actors: &[CombatantInfo],
    sep_radius: f32,
) -> Vec2 {
    let mut sep = Vec2::ZERO;
    let r_sq = sep_radius * sep_radius;
    for actor in actors {
        if !actor.active || actor.id == self_id {
            continue;
        }
        let diff = self_pos_xz - Vec2::new(actor.pos.x, actor.pos.z);
        let d_sq = diff.length_squared();
        if d_sq > 1e-6 && d_sq < r_sq {
            let d = d_sq.sqrt();
            sep += (diff / d) * ((sep_radius - d) / sep_radius);
        }
    }
    sep
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FloatingKind {
    EnemyDamage,
    PlayerSideDamage,
    Heal,
}

#[derive(Copy, Clone, Debug)]
pub struct FloatingNumber {
    pub world_pos: Vec3,
    pub amount: u32,
    pub timer: f32,
    pub kind: FloatingKind,
    pub active: bool,
}

impl Default for FloatingNumber {
    fn default() -> Self {
        Self {
            world_pos: Vec3::ZERO,
            amount: 0,
            timer: 0.0,
            kind: FloatingKind::EnemyDamage,
            active: false,
        }
    }
}

pub struct FloatingNumberPool {
    entries: [FloatingNumber; MAX_FLOATING_NUMBERS],
    next_slot: usize,
}

impl FloatingNumberPool {
    pub fn new() -> Self {
        Self {
            entries: [FloatingNumber::default(); MAX_FLOATING_NUMBERS],
            next_slot: 0,
        }
    }

    pub fn clear(&mut self) {
        self.entries = [FloatingNumber::default(); MAX_FLOATING_NUMBERS];
        self.next_slot = 0;
    }

    pub fn spawn(&mut self, world_pos: Vec3, amount: u32, is_player_damage: bool) {
        let kind = if is_player_damage {
            FloatingKind::PlayerSideDamage
        } else {
            FloatingKind::EnemyDamage
        };
        self.spawn_kind(world_pos, amount, kind);
    }

    pub fn spawn_kind(&mut self, world_pos: Vec3, amount: u32, kind: FloatingKind) {
        let slot = self
            .entries
            .iter()
            .position(|e| !e.active)
            .unwrap_or_else(|| {
                let s = self.next_slot;
                self.next_slot = (self.next_slot + 1) % MAX_FLOATING_NUMBERS;
                s
            });

        self.entries[slot] = FloatingNumber {
            world_pos,
            amount,
            timer: FLOATING_NUMBER_DURATION,
            kind,
            active: true,
        };
    }

    pub fn update(&mut self, dt: f32) {
        for entry in &mut self.entries {
            if entry.active {
                entry.timer -= dt;
                entry.world_pos.y += FLOATING_NUMBER_RISE_SPEED * dt;
                if entry.timer <= 0.0 {
                    entry.active = false;
                }
            }
        }
    }

    pub fn entries(&self) -> &[FloatingNumber; MAX_FLOATING_NUMBERS] {
        &self.entries
    }
}

pub struct PlayerCombat {
    pub health: f32,
    pub time_since_damaged: f32,
    pub hit_flash_timer: f32,
    swing_elapsed: Option<f32>,
    hit_enemy_ids: [u32; 16],
    hit_enemy_count: usize,
    dodge_elapsed: Option<f32>,
    dodge_dir: Vec2,
    pub dodge_cooldown_timer: f32,
    pub skill_invuln_timer: f32,
}

impl PlayerCombat {
    pub fn new() -> Self {
        Self {
            health: PLAYER_MAX_HEALTH,
            time_since_damaged: REGEN_DELAY,
            hit_flash_timer: 0.0,
            swing_elapsed: None,
            hit_enemy_ids: [0; 16],
            hit_enemy_count: 0,
            dodge_elapsed: None,
            dodge_dir: Vec2::new(0.0, -1.0),
            dodge_cooldown_timer: 0.0,
            skill_invuln_timer: 0.0,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn is_dead(&self) -> bool {
        self.health <= 0.0
    }

    pub fn is_swinging(&self) -> bool {
        self.swing_elapsed.is_some()
    }

    pub fn is_swing_active_hitbox(&self) -> bool {
        if let Some(t) = self.swing_elapsed {
            (SWING_WINDUP..(SWING_WINDUP + SWING_ACTIVE)).contains(&t)
        } else {
            false
        }
    }

    pub fn swing_lunge_factor(&self) -> f32 {
        if let Some(t) = self.swing_elapsed {
            let window = SWING_WINDUP + SWING_ACTIVE;
            if t < window {
                (t / window * std::f32::consts::PI).sin().max(0.0)
            } else {
                0.0
            }
        } else {
            0.0
        }
    }

    pub fn is_dodging(&self) -> bool {
        self.dodge_elapsed.is_some()
    }

    pub fn dodge_velocity_xz(&self) -> Option<Vec2> {
        self.dodge_elapsed.map(|_| self.dodge_dir * DODGE_SPEED)
    }

    pub fn is_invulnerable(&self) -> bool {
        if self.skill_invuln_timer > 0.0 {
            return true;
        }
        if let Some(t) = self.dodge_elapsed {
            t < DODGE_INVULN_TIME
        } else {
            false
        }
    }

    pub fn dodge_ready_ratio(&self) -> f32 {
        if DODGE_COOLDOWN <= 1e-5 {
            1.0
        } else {
            (1.0 - self.dodge_cooldown_timer / DODGE_COOLDOWN).clamp(0.0, 1.0)
        }
    }

    pub fn hit_flash_factor(&self) -> f32 {
        (self.hit_flash_timer / HIT_FLASH_DURATION).clamp(0.0, 1.0)
    }

    pub fn try_start_swing(&mut self) -> bool {
        if self.is_dead() || self.swing_elapsed.is_some() || self.dodge_elapsed.is_some() {
            return false;
        }
        self.swing_elapsed = Some(0.0);
        self.hit_enemy_count = 0;
        true
    }

    pub fn try_start_dodge(&mut self, grounded: bool, move_dir_xz: Vec2, yaw: f32) -> bool {
        if self.is_dead()
            || !grounded
            || self.dodge_elapsed.is_some()
            || self.dodge_cooldown_timer > 0.0
        {
            return false;
        }

        let dir = if move_dir_xz.length_squared() > 1e-4 {
            move_dir_xz.normalize()
        } else {
            Vec2::new(-yaw.sin(), -yaw.cos()).normalize_or_zero()
        };

        self.dodge_dir = if dir.length_squared() > 0.0 {
            dir
        } else {
            Vec2::new(0.0, -1.0)
        };
        self.dodge_elapsed = Some(0.0);
        self.dodge_cooldown_timer = DODGE_COOLDOWN;
        self.swing_elapsed = None;
        true
    }

    pub fn try_register_swing_hit(&mut self, enemy_id: u32) -> bool {
        for i in 0..self.hit_enemy_count {
            if self.hit_enemy_ids[i] == enemy_id {
                return false;
            }
        }
        if self.hit_enemy_count < self.hit_enemy_ids.len() {
            self.hit_enemy_ids[self.hit_enemy_count] = enemy_id;
            self.hit_enemy_count += 1;
        }
        true
    }

    pub fn apply_damage(&mut self, amount: f32) -> bool {
        if self.is_dead() || self.is_invulnerable() || amount <= 0.0 {
            return false;
        }
        self.health = (self.health - amount).max(0.0);
        self.time_since_damaged = 0.0;
        self.hit_flash_timer = HIT_FLASH_DURATION;
        true
    }

    pub fn update(&mut self, dt: f32) {
        self.hit_flash_timer = (self.hit_flash_timer - dt).max(0.0);
        self.dodge_cooldown_timer = (self.dodge_cooldown_timer - dt).max(0.0);
        self.skill_invuln_timer = (self.skill_invuln_timer - dt).max(0.0);

        if let Some(ref mut t) = self.swing_elapsed {
            *t += dt;
            if *t >= SWING_TOTAL_TIME {
                self.swing_elapsed = None;
                self.hit_enemy_count = 0;
            }
        }

        if let Some(ref mut t) = self.dodge_elapsed {
            *t += dt;
            if *t >= DODGE_DURATION {
                self.dodge_elapsed = None;
            }
        }

        if !self.is_dead() {
            self.time_since_damaged += dt;
            if self.time_since_damaged > REGEN_DELAY && self.health < PLAYER_MAX_HEALTH {
                let regen_dt = (self.time_since_damaged - REGEN_DELAY).min(dt);
                self.health = (self.health + REGEN_RATE * regen_dt).min(PLAYER_MAX_HEALTH);
            }
        }
    }
}

pub fn is_in_attack_arc(
    attacker_pos: Vec3,
    attacker_yaw: f32,
    target_pos: Vec3,
    range: f32,
    arc_rad: f32,
) -> bool {
    let to_target = Vec2::new(target_pos.x - attacker_pos.x, target_pos.z - attacker_pos.z);
    let dist_sq = to_target.length_squared();
    if dist_sq > range * range {
        return false;
    }
    if dist_sq < 1e-6 {
        return true;
    }

    let forward = Vec2::new(-attacker_yaw.sin(), -attacker_yaw.cos());
    let dir = to_target / dist_sq.sqrt();
    let cos_half = (arc_rad * 0.5).cos();
    forward.dot(dir) >= cos_half
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attack_arc_hit_detection() {
        let origin = Vec3::ZERO;
        let yaw = 0.0;

        assert!(is_in_attack_arc(
            origin,
            yaw,
            Vec3::new(0.0, 0.0, -1.8),
            SWING_RANGE,
            SWING_ARC_RAD
        ));
        assert!(is_in_attack_arc(
            origin,
            yaw,
            Vec3::new(1.2, 0.0, -1.2),
            SWING_RANGE,
            SWING_ARC_RAD
        ));
        assert!(!is_in_attack_arc(
            origin,
            yaw,
            Vec3::new(0.0, 0.0, -3.0),
            SWING_RANGE,
            SWING_ARC_RAD
        ));
        assert!(!is_in_attack_arc(
            origin,
            yaw,
            Vec3::new(1.5, 0.0, 0.0),
            SWING_RANGE,
            SWING_ARC_RAD
        ));
        assert!(!is_in_attack_arc(
            origin,
            yaw,
            Vec3::new(0.0, 0.0, 1.5),
            SWING_RANGE,
            SWING_ARC_RAD
        ));
    }

    #[test]
    fn test_one_hit_per_swing() {
        let mut combat = PlayerCombat::new();
        assert!(combat.try_start_swing());
        assert!(combat.try_register_swing_hit(42));
        assert!(!combat.try_register_swing_hit(42));
        assert!(combat.try_register_swing_hit(43));
    }

    #[test]
    fn test_dodge_invulnerability_and_air_block() {
        let mut combat = PlayerCombat::new();
        assert!(!combat.try_start_dodge(false, Vec2::new(0.0, -1.0), 0.0));

        assert!(combat.try_start_dodge(true, Vec2::new(0.0, -1.0), 0.0));
        assert!(combat.is_invulnerable());
        assert!(!combat.apply_damage(20.0));
        assert_eq!(combat.health, PLAYER_MAX_HEALTH);

        combat.update(0.28);
        assert!(combat.is_dodging());
        assert!(!combat.is_invulnerable());
        assert!(combat.apply_damage(20.0));
        assert_eq!(combat.health, 80.0);
    }

    #[test]
    fn test_player_damage_death_and_health_regen() {
        let mut combat = PlayerCombat::new();
        assert!(combat.apply_damage(30.0));
        assert_eq!(combat.health, 70.0);

        combat.update(4.0);
        assert_eq!(combat.health, 70.0);

        combat.update(2.0);
        assert!((combat.health - 72.0).abs() < 1e-4);

        assert!(combat.apply_damage(100.0));
        assert!(combat.is_dead());
        combat.update(10.0);
        assert_eq!(combat.health, 0.0);
    }
}
