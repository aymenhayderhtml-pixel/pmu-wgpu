use crate::collision::{circle_overlaps_aabb_xz, ray_intersects_aabb, CollisionGrid};
use crate::combat::{ActorId, Faction};
use crate::enemy::Enemy;
use crate::frustum::Frustum;
use crate::skills::{level_multiplier, SkillId};
use crate::status::{BURN_DURATION, SLOW_DURATION};
use crate::world::{InstanceData, GROUND_SIZE};
use glam::{Mat4, Quat, Vec2, Vec3};

pub const MAX_PROJECTILES: usize = 32;
pub const MAX_EFFECT_FLASHES: usize = 32;
pub const MAX_METEOR_MARKS: usize = 8;
pub const MAX_SLOW_ZONES: usize = 16;

pub const PROJECTILE_RADIUS: f32 = 0.25;
pub const ENEMY_HIT_RADIUS: f32 = 0.60;
pub const PROJECTILE_HEIGHT: f32 = 1.0;
pub const MAX_SUBSTEP_DIST: f32 = 0.25;
pub const EFFECT_FLASH_DURATION: f32 = 0.30;
pub const SLOW_ZONE_RADIUS: f32 = 2.5;
pub const SLOW_ZONE_DURATION: f32 = 2.0;

#[derive(Copy, Clone, Debug)]
pub struct Projectile {
    pub pos: Vec3,
    pub vel: Vec3,
    pub remaining_life: f32,
    pub owner_faction: Faction,
    pub skill: SkillId,
    pub pierce_remaining: u32,
    pub level: u8,
    pub hit_enemy_ids: [u32; 4],
    pub hit_count: usize,
    pub active: bool,
}

impl Default for Projectile {
    fn default() -> Self {
        Self {
            pos: Vec3::ZERO,
            vel: Vec3::ZERO,
            remaining_life: 0.0,
            owner_faction: Faction::PlayerSide,
            skill: SkillId::Fireball,
            pierce_remaining: 0,
            level: 1,
            hit_enemy_ids: [0; 4],
            hit_count: 0,
            active: false,
        }
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct EffectFlash {
    pub pos: Vec3,
    pub max_radius: f32,
    pub timer: f32,
    pub color: [f32; 3],
    pub active: bool,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct MeteorMark {
    pub pos: Vec3,
    pub timer: f32,
    pub radius: f32,
    pub damage: f32,
    pub knockback: f32,
    pub active: bool,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct SlowZone {
    pub pos: Vec3,
    pub radius: f32,
    pub timer: f32,
    pub active: bool,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct SkillDamageHit {
    pub enemy_id: u32,
    pub damage: f32,
    pub source_pos: Vec3,
    pub knockback: f32,
    pub apply_burn: bool,
    pub apply_slow: bool,
}

pub struct ProjectileStepEvents {
    pub hits: [SkillDamageHit; 64],
    pub hit_count: usize,
}

impl ProjectileStepEvents {
    pub fn new() -> Self {
        Self {
            hits: [SkillDamageHit::default(); 64],
            hit_count: 0,
        }
    }

    pub fn push_hit(&mut self, hit: SkillDamageHit) {
        if self.hit_count < self.hits.len() {
            self.hits[self.hit_count] = hit;
            self.hit_count += 1;
        }
    }
}

pub struct ProjectileManager {
    pub projectiles: [Projectile; MAX_PROJECTILES],
    pub flashes: [EffectFlash; MAX_EFFECT_FLASHES],
    pub meteors: [MeteorMark; MAX_METEOR_MARKS],
    pub slow_zones: [SlowZone; MAX_SLOW_ZONES],
    next_proj: usize,
    next_flash: usize,
    next_meteor: usize,
    next_zone: usize,
}

impl ProjectileManager {
    pub fn new() -> Self {
        Self {
            projectiles: [Projectile::default(); MAX_PROJECTILES],
            flashes: [EffectFlash::default(); MAX_EFFECT_FLASHES],
            meteors: [MeteorMark::default(); MAX_METEOR_MARKS],
            slow_zones: [SlowZone::default(); MAX_SLOW_ZONES],
            next_proj: 0,
            next_flash: 0,
            next_meteor: 0,
            next_zone: 0,
        }
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }

    pub fn active_projectile_count(&self) -> u32 {
        self.projectiles.iter().filter(|p| p.active).count() as u32
    }

    pub fn active_effect_count(&self) -> u32 {
        let f = self.flashes.iter().filter(|e| e.active).count();
        let m = self.meteors.iter().filter(|e| e.active).count();
        let z = self.slow_zones.iter().filter(|e| e.active).count();
        (f + m + z) as u32
    }

    pub fn spawn_projectile(
        &mut self,
        origin_xz: Vec2,
        dir_xz: Vec2,
        skill: SkillId,
        level: u8,
        pierce: u32,
    ) {
        let def = skill.def();
        let norm_dir = dir_xz.normalize_or_zero();
        let slot = self
            .projectiles
            .iter()
            .position(|p| !p.active)
            .unwrap_or_else(|| {
                let s = self.next_proj;
                self.next_proj = (self.next_proj + 1) % MAX_PROJECTILES;
                s
            });

        self.projectiles[slot] = Projectile {
            pos: Vec3::new(origin_xz.x, PROJECTILE_HEIGHT, origin_xz.y),
            vel: Vec3::new(norm_dir.x * def.speed, 0.0, norm_dir.y * def.speed),
            remaining_life: def.duration,
            owner_faction: Faction::PlayerSide,
            skill,
            pierce_remaining: pierce,
            level,
            hit_enemy_ids: [0; 4],
            hit_count: 0,
            active: true,
        };
    }

    pub fn spawn_flash(&mut self, pos: Vec3, max_radius: f32, color: [f32; 3]) {
        let slot = self
            .flashes
            .iter()
            .position(|f| !f.active)
            .unwrap_or_else(|| {
                let s = self.next_flash;
                self.next_flash = (self.next_flash + 1) % MAX_EFFECT_FLASHES;
                s
            });

        self.flashes[slot] = EffectFlash {
            pos,
            max_radius: max_radius.max(0.8),
            timer: EFFECT_FLASH_DURATION,
            color,
            active: true,
        };
    }

    pub fn spawn_meteor(
        &mut self,
        player_pos: Vec3,
        aim_dir_xz: Vec2,
        mark_dist: f32,
        delay: f32,
        radius: f32,
        damage: f32,
        knockback: f32,
        collision: &CollisionGrid,
    ) {
        let ray_origin = Vec3::new(player_pos.x, 1.0, player_pos.z);
        let ray_dir = Vec3::new(aim_dir_xz.x, 0.0, aim_dir_xz.y).normalize_or_zero();
        let mut best_t = mark_dist;

        let end = ray_origin + ray_dir * mark_dist;
        collision.query_region(
            ray_origin.x.min(end.x) - 0.5,
            ray_origin.x.max(end.x) + 0.5,
            ray_origin.z.min(end.z) - 0.5,
            ray_origin.z.max(end.z) + 0.5,
            |_, aabb| {
                if aabb.max.y > 0.2 {
                    if let Some(t) = ray_intersects_aabb(ray_origin, ray_dir, best_t, aabb) {
                        if t < best_t {
                            best_t = (t - 0.3).max(0.5);
                        }
                    }
                }
            },
        );

        let bound = GROUND_SIZE * 0.5 - 1.0;
        let impact_pos = Vec3::new(
            (player_pos.x + ray_dir.x * best_t).clamp(-bound, bound),
            0.0,
            (player_pos.z + ray_dir.z * best_t).clamp(-bound, bound),
        );

        let slot = self
            .meteors
            .iter()
            .position(|m| !m.active)
            .unwrap_or_else(|| {
                let s = self.next_meteor;
                self.next_meteor = (self.next_meteor + 1) % MAX_METEOR_MARKS;
                s
            });

        self.meteors[slot] = MeteorMark {
            pos: impact_pos,
            timer: delay,
            radius,
            damage,
            knockback,
            active: true,
        };
    }

    pub fn spawn_slow_zone(&mut self, pos: Vec3) {
        let slot = self
            .slow_zones
            .iter()
            .position(|z| !z.active)
            .unwrap_or_else(|| {
                let s = self.next_zone;
                self.next_zone = (self.next_zone + 1) % MAX_SLOW_ZONES;
                s
            });

        self.slow_zones[slot] = SlowZone {
            pos: Vec3::new(pos.x, 0.0, pos.z),
            radius: SLOW_ZONE_RADIUS,
            timer: SLOW_ZONE_DURATION,
            active: true,
        };
    }

    /// Updates all projectiles (with `<= 0.25m` sub-stepping), meteor markers, slow zones, and flashes.
    pub fn update(
        &mut self,
        dt: f32,
        enemies: &mut [Enemy],
        collision: &CollisionGrid,
    ) -> ProjectileStepEvents {
        let mut events = ProjectileStepEvents::new();
        let half_ground = GROUND_SIZE * 0.5;
        let combined_hit_r = PROJECTILE_RADIUS + ENEMY_HIT_RADIUS;
        let combined_hit_r_sq = combined_hit_r * combined_hit_r;

        // 1. Sub-stepped projectiles
        for p_idx in 0..MAX_PROJECTILES {
            if !self.projectiles[p_idx].active {
                continue;
            }
            let step_time = dt.min(self.projectiles[p_idx].remaining_life);
            self.projectiles[p_idx].remaining_life -= dt;

            let move_vec = self.projectiles[p_idx].vel * step_time;
            let total_dist = move_vec.length();
            let substeps = ((total_dist / MAX_SUBSTEP_DIST).ceil() as usize).max(1);
            let sub_delta = move_vec / (substeps as f32);

            let def = self.projectiles[p_idx].skill.def();
            let (explosion_radius, applies_burn, applies_slow) = match def.kind {
                crate::skills::SkillKind::Projectile {
                    explosion_radius,
                    applies_burn,
                    applies_slow,
                    ..
                } => (explosion_radius, applies_burn, applies_slow),
                _ => (0.0, false, false),
            };
            let scaled_power = def.power * level_multiplier(self.projectiles[p_idx].level);

            let mut exploded_at: Option<(Vec3, Option<u32>)> = None;

            for _ in 0..substeps {
                let next_pos = self.projectiles[p_idx].pos + sub_delta;
                self.projectiles[p_idx].pos = next_pos;

                if next_pos.x.abs() > half_ground
                    || next_pos.z.abs() > half_ground
                    || next_pos.y <= 0.0
                {
                    exploded_at = Some((next_pos, None));
                    break;
                }

                // Check cube AABB collision
                let mut hit_cube = false;
                collision.query_region(
                    next_pos.x - PROJECTILE_RADIUS,
                    next_pos.x + PROJECTILE_RADIUS,
                    next_pos.z - PROJECTILE_RADIUS,
                    next_pos.z + PROJECTILE_RADIUS,
                    |_, aabb| {
                        if next_pos.y + PROJECTILE_RADIUS >= aabb.min.y
                            && next_pos.y - PROJECTILE_RADIUS <= aabb.max.y
                            && circle_overlaps_aabb_xz(
                                next_pos.x,
                                next_pos.z,
                                PROJECTILE_RADIUS,
                                aabb,
                            )
                        {
                            hit_cube = true;
                        }
                    },
                );
                if hit_cube {
                    exploded_at = Some((next_pos, None));
                    break;
                }

                // Check enemy collision (friendly fire is always off for PlayerSide projectiles)
                if self.projectiles[p_idx].owner_faction == Faction::PlayerSide {
                    let mut hit_enemy_id = None;
                    for enemy in enemies.iter() {
                        if !enemy.is_alive() {
                            continue;
                        }
                        let already_hit = self.projectiles[p_idx].hit_enemy_ids
                            [..self.projectiles[p_idx].hit_count]
                            .contains(&enemy.id);
                        if already_hit {
                            continue;
                        }
                        let dx = next_pos.x - enemy.pos.x;
                        let dz = next_pos.z - enemy.pos.z;
                        if dx * dx + dz * dz <= combined_hit_r_sq {
                            hit_enemy_id = Some(enemy.id);
                            break;
                        }
                    }

                    if let Some(eid) = hit_enemy_id {
                        let hc = self.projectiles[p_idx].hit_count;
                        if hc < self.projectiles[p_idx].hit_enemy_ids.len() {
                            self.projectiles[p_idx].hit_enemy_ids[hc] = eid;
                            self.projectiles[p_idx].hit_count += 1;
                        }

                        events.push_hit(SkillDamageHit {
                            enemy_id: eid,
                            damage: scaled_power,
                            source_pos: next_pos - sub_delta,
                            knockback: 4.0,
                            apply_burn: applies_burn,
                            apply_slow: applies_slow,
                        });

                        if self.projectiles[p_idx].pierce_remaining > 0 {
                            self.projectiles[p_idx].pierce_remaining -= 1;
                        } else {
                            exploded_at = Some((next_pos, Some(eid)));
                            break;
                        }
                    }
                }
            }

            if let Some((impact_pos, direct_hit_id)) = exploded_at {
                self.projectiles[p_idx].active = false;
                if explosion_radius > 0.0 {
                    self.spawn_flash(impact_pos, explosion_radius, def.tint);
                    let impact_xz = Vec2::new(impact_pos.x, impact_pos.z);
                    for enemy in enemies.iter() {
                        if !enemy.is_alive() || Some(enemy.id) == direct_hit_id {
                            continue;
                        }
                        let dist = Vec2::new(enemy.pos.x, enemy.pos.z).distance(impact_xz);
                        if dist <= explosion_radius {
                            // Linear falloff from 100% at center to 50% at edge
                            let falloff = 1.0 - 0.5 * (dist / explosion_radius).clamp(0.0, 1.0);
                            events.push_hit(SkillDamageHit {
                                enemy_id: enemy.id,
                                damage: scaled_power * falloff,
                                source_pos: impact_pos,
                                knockback: 5.0 * falloff,
                                apply_burn: applies_burn,
                                apply_slow: applies_slow,
                            });
                        }
                    }
                } else {
                    self.spawn_flash(impact_pos, 0.7, def.tint);
                }
            } else if self.projectiles[p_idx].remaining_life <= 0.0 {
                self.projectiles[p_idx].active = false;
            }
        }

        // 2. Meteor Slam delayed blasts
        for m_idx in 0..MAX_METEOR_MARKS {
            if !self.meteors[m_idx].active {
                continue;
            }
            self.meteors[m_idx].timer -= dt;
            if self.meteors[m_idx].timer <= 0.0 {
                let m = self.meteors[m_idx];
                self.meteors[m_idx].active = false;
                self.spawn_flash(
                    m.pos + Vec3::new(0.0, 0.5, 0.0),
                    m.radius,
                    [1.0, 0.30, 0.10],
                );
                let center_xz = Vec2::new(m.pos.x, m.pos.z);
                for enemy in enemies.iter() {
                    if !enemy.is_alive() {
                        continue;
                    }
                    let dist = Vec2::new(enemy.pos.x, enemy.pos.z).distance(center_xz);
                    if dist <= m.radius {
                        let falloff = 1.0 - 0.5 * (dist / m.radius).clamp(0.0, 1.0);
                        events.push_hit(SkillDamageHit {
                            enemy_id: enemy.id,
                            damage: m.damage * falloff,
                            source_pos: m.pos,
                            knockback: m.knockback * falloff,
                            apply_burn: true,
                            apply_slow: false,
                        });
                    }
                }
            }
        }

        // 3. Frost Dash slow zones
        for z_idx in 0..MAX_SLOW_ZONES {
            if !self.slow_zones[z_idx].active {
                continue;
            }
            self.slow_zones[z_idx].timer -= dt;
            if self.slow_zones[z_idx].timer <= 0.0 {
                self.slow_zones[z_idx].active = false;
                continue;
            }
            let z = self.slow_zones[z_idx];
            let z_xz = Vec2::new(z.pos.x, z.pos.z);
            for enemy in enemies.iter_mut() {
                if enemy.is_alive()
                    && Vec2::new(enemy.pos.x, enemy.pos.z).distance_squared(z_xz)
                        <= z.radius * z.radius
                {
                    enemy.status.apply_slow(SLOW_DURATION);
                }
            }
        }

        // 4. Effect flashes
        for flash in &mut self.flashes {
            if flash.active {
                flash.timer -= dt;
                if flash.timer <= 0.0 {
                    flash.active = false;
                }
            }
        }

        events
    }

    /// Appends cube instances for active projectiles (plus 3 trail cubes each), flashes, meteor marks, and slow zones.
    pub fn append_visible_cube_instances(&self, frustum: &Frustum, out: &mut Vec<InstanceData>) {
        let cube_min = Vec3::splat(-0.5);
        let cube_max = Vec3::splat(0.5);

        for proj in &self.projectiles {
            if !proj.active {
                continue;
            }
            let tint = proj.skill.def().tint;
            let vel_dir = proj.vel.normalize_or_zero();

            // Main projectile cube + 3 trailing cubes
            for trail_idx in 0..4 {
                let t_f = trail_idx as f32;
                let scale_s = (0.36 - t_f * 0.07).max(0.10);
                let trail_pos = proj.pos - vel_dir * (t_f * 0.32);
                let model = Mat4::from_scale_rotation_translation(
                    Vec3::splat(scale_s),
                    Quat::IDENTITY,
                    trail_pos,
                );
                if frustum.intersects_local_aabb(model, cube_min, cube_max) {
                    out.push(InstanceData {
                        model,
                        color: [tint[0], tint[1], tint[2], 1.65 - t_f * 0.15],
                    });
                }
            }
        }

        for flash in &self.flashes {
            if !flash.active {
                continue;
            }
            let progress = 1.0 - (flash.timer / EFFECT_FLASH_DURATION).clamp(0.0, 1.0);
            let cur_r = flash.max_radius * (0.25 + 0.75 * progress);
            let scale = Vec3::new(cur_r * 1.4, (cur_r * 0.6).max(0.25), cur_r * 1.4);
            let model = Mat4::from_scale_rotation_translation(scale, Quat::IDENTITY, flash.pos);
            if frustum.intersects_local_aabb(model, cube_min, cube_max) {
                let intensity = 1.0 + (1.0 - progress) * 0.9;
                out.push(InstanceData {
                    model,
                    color: [flash.color[0], flash.color[1], flash.color[2], intensity],
                });
            }
        }

        for m in &self.meteors {
            if !m.active {
                continue;
            }
            let scale = Vec3::new(m.radius * 1.2, 0.08, m.radius * 1.2);
            let model = Mat4::from_scale_rotation_translation(
                scale,
                Quat::IDENTITY,
                m.pos + Vec3::new(0.0, 0.05, 0.0),
            );
            if frustum.intersects_local_aabb(model, cube_min, cube_max) {
                out.push(InstanceData {
                    model,
                    color: [1.0, 0.25, 0.10, 1.8],
                });
            }
        }

        for z in &self.slow_zones {
            if !z.active {
                continue;
            }
            let scale = Vec3::new(z.radius * 1.3, 0.06, z.radius * 1.3);
            let model = Mat4::from_scale_rotation_translation(
                scale,
                Quat::IDENTITY,
                z.pos + Vec3::new(0.0, 0.03, 0.0),
            );
            if frustum.intersects_local_aabb(model, cube_min, cube_max) {
                out.push(InstanceData {
                    model,
                    color: [0.35, 0.85, 1.0, 1.3],
                });
            }
        }
    }
}

pub fn apply_skill_hit_to_enemy(enemy: &mut Enemy, hit: &SkillDamageHit) -> bool {
    if !enemy.is_alive() {
        return false;
    }
    if hit.apply_burn {
        enemy.status.apply_burn(BURN_DURATION);
    }
    if hit.apply_slow {
        enemy.status.apply_slow(SLOW_DURATION);
    }
    let killed = enemy.apply_hit_from(hit.damage, hit.source_pos, ActorId::Player);
    if !killed && hit.knockback > 0.0 {
        let away = Vec2::new(
            enemy.pos.x - hit.source_pos.x,
            enemy.pos.z - hit.source_pos.z,
        );
        let dir = if away.length_squared() > 1e-5 {
            away.normalize()
        } else {
            Vec2::new(0.0, 1.0)
        };
        enemy.vel.x = dir.x * hit.knockback;
        enemy.vel.z = dir.y * hit.knockback;
    }
    killed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::Aabb;

    #[test]
    fn test_projectile_cube_collision_and_no_tunneling_at_high_speed() {
        let thin_wall = Aabb {
            min: Vec3::new(-2.0, 0.0, -5.1),
            max: Vec3::new(2.0, 2.5, -4.9),
        };
        let grid = CollisionGrid::new(vec![thin_wall]);
        let mut mgr = ProjectileManager::new();
        let mut enemies = Vec::new();

        // Spawn IceShard (speed 26 m/s) firing along -Z toward the 0.2m thin wall at z = -5.0
        mgr.spawn_projectile(
            Vec2::ZERO,
            Vec2::new(0.0, -1.0),
            SkillId::IceShard,
            1,
            1,
        );
        // Step with a large dt (0.5s = 13m of travel in one step); substepping prevents tunneling!
        mgr.update(0.5, &mut enemies, &grid);
        assert!(!mgr.projectiles[0].active);
        assert!(mgr.projectiles[0].pos.z >= -5.2);
    }

    #[test]
    fn test_projectile_pierce_explosion_falloff_and_no_friendly_fire() {
        let grid = CollisionGrid::new(Vec::new());
        let mut mgr = ProjectileManager::new();

        // Two enemies lined up along -Z at 2m and 4m
        let mut enemies = vec![
            Enemy::new(1, Vec3::new(0.0, 0.0, -2.0), 1, 1, 0),
            Enemy::new(2, Vec3::new(0.0, 0.0, -4.0), 2, 1, 0),
        ];

        // IceShard has pierce = 1 -> hits both enemy 1 and enemy 2, hitting each at most once
        mgr.spawn_projectile(
            Vec2::ZERO,
            Vec2::new(0.0, -1.0),
            SkillId::IceShard,
            1,
            1,
        );
        let ev = mgr.update(0.25, &mut enemies, &grid);
        assert_eq!(ev.hit_count, 2);
        assert_eq!(ev.hits[0].enemy_id, 1);
        assert_eq!(ev.hits[1].enemy_id, 2);

        // Fireball explosion falloff: direct hit on enemy 1 at (0,0,-2), splash on enemy 3 at 1.9m away
        let mut enemies2 = vec![
            Enemy::new(10, Vec3::new(0.0, 0.0, -2.0), 1, 1, 0),
            Enemy::new(11, Vec3::new(1.8, 0.0, -1.25), 2, 1, 0),
        ];
        mgr.clear();
        mgr.spawn_projectile(
            Vec2::ZERO,
            Vec2::new(0.0, -1.0),
            SkillId::Fireball,
            1,
            0,
        );
        let ev2 = mgr.update(0.20, &mut enemies2, &grid);
        assert_eq!(ev2.hit_count, 2);
        let direct_dmg = ev2.hits[0].damage;
        let edge_dmg = ev2.hits[1].damage;
        assert!((direct_dmg - 28.0).abs() < 1e-4);
        assert!(edge_dmg < direct_dmg && edge_dmg >= 14.0);
    }

    #[test]
    fn test_meteor_delay_detonation() {
        let grid = CollisionGrid::new(Vec::new());
        let mut mgr = ProjectileManager::new();
        let mut enemies = vec![Enemy::new(1, Vec3::new(0.0, 0.0, -10.0), 1, 1, 0)];

        mgr.spawn_meteor(
            Vec3::ZERO,
            Vec2::new(0.0, -1.0),
            10.0,
            0.60,
            6.0,
            70.0,
            10.0,
            &grid,
        );

        // Before 0.6s delay, no damage
        let ev_early = mgr.update(0.40, &mut enemies, &grid);
        assert_eq!(ev_early.hit_count, 0);

        // After 0.6s total, meteor detonates and hits enemy at -10m
        let ev_boom = mgr.update(0.25, &mut enemies, &grid);
        assert_eq!(ev_boom.hit_count, 1);
        assert!((ev_boom.hits[0].damage - 70.0).abs() < 1e-3);
        assert!(ev_boom.hits[0].apply_burn);
    }
}
