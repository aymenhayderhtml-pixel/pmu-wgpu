use crate::ai::{collect_enemy_astar_requests, update_enemies_with_targets, AiStepStats};
use crate::ally::MAX_ALLIES;
use crate::animation::{AnimationClip, Skeleton};
use crate::camera::{lerp_angle, Camera};
use crate::collision::{Aabb, CollisionGrid};
use crate::ally::ALLY_HEAL_PULSE_RADIUS;
use crate::combat::{
    is_in_attack_arc, ActorId, CombatantInfo, Faction, FloatingKind, FloatingNumberPool,
    PlayerCombat, PLAYER_MAX_HEALTH, SWING_ARC_RAD, SWING_DAMAGE, SWING_RANGE, SWING_SPEED_MULT,
};
use crate::effects::EffectsPool;
use crate::enemy::{Enemy, EnemyState};
use crate::frustum::Frustum;
use crate::input::Input;
use crate::nav::{NavGrid, NavSearchScratch};
use crate::pickups::PickupManager;
use crate::player_anim::{AnimMode, ClipMap, PlayerAnimator};
use crate::projectiles::{apply_skill_hit_to_enemy, ProjectileManager, SkillDamageHit};
use crate::game_flow::GameFlow;
use crate::save::{
    current_unix_seconds, SaveData, SavedAlly, SavedEnemy, WavePhase, SAVE_VERSION,
};
use crate::skill_inv::{PickupOutcome, SkillInventory};
use crate::skill_menu::MenuState;
use crate::skills::{
    compute_aim_direction_xz, level_multiplier, SkillId, SkillKind, CAST_SPEED_MULT,
};
use crate::squad::{AllySkillCast, AstarBudgetScheduler, Squad, SquadMode};
use crate::waves::{WaveManager, MAX_ALIVE_ENEMIES};
use glam::{Mat4, Quat, Vec2, Vec3};
use std::fmt::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use winit::keyboard::KeyCode;

pub const MODEL_YAW_OFFSET: f32 = std::f32::consts::PI;

pub const GROUND_SIZE: f32 = 60.0;
pub const STATIC_CUBE_COUNT: usize = 300;
pub const TOTAL_CUBE_INSTANCES: usize = 1024;

static CUBE_OVERFLOW_WARNED: AtomicBool = AtomicBool::new(false);

#[inline]
pub fn push_cube_instance_guarded(out: &mut Vec<InstanceData>, inst: InstanceData) {
    if out.len() < TOTAL_CUBE_INSTANCES {
        out.push(inst);
    } else if !CUBE_OVERFLOW_WARNED.swap(true, Ordering::Relaxed) {
        eprintln!(
            "Warning: cube instance count reached TOTAL_CUBE_INSTANCES ({}); dropping lowest-priority visual effect instances",
            TOTAL_CUBE_INSTANCES
        );
    }
}
pub const PROP_MAX_INSTANCES: usize = 50;

pub const PLAYER_RADIUS: f32 = 0.4;
pub const PLAYER_HEIGHT: f32 = 2.0;
pub const PLAYER_SCALE: Vec3 = Vec3::new(1.0, PLAYER_HEIGHT, 1.0);
pub const PLAYER_COLOR: [f32; 4] = [0.10, 0.35, 0.90, 1.0];

pub const WALK_SPEED: f32 = 5.0;
pub const SPRINT_SPEED: f32 = 8.5;
pub const GROUND_ACCEL: f32 = 38.0;
pub const GROUND_DECEL: f32 = 32.0;
pub const AIR_CONTROL_FACTOR: f32 = 0.40;
pub const PLAYER_TURN_SPEED: f32 = 12.0;

pub const JUMP_SPEED: f32 = 6.5;
pub const GRAVITY: f32 = 18.0;
pub const COYOTE_TIME: f32 = 0.10;
pub const JUMP_BUFFER_TIME: f32 = 0.10;

pub const WORLD_SEED: u64 = 0x504D_555F_3230_3236;

#[derive(Copy, Clone, Debug)]
pub struct InstanceData {
    pub model: Mat4,
    pub color: [f32; 4],
}

#[derive(Copy, Clone, Debug)]
pub struct ModelInstance {
    pub model_id: usize,
    pub model: Mat4,
    pub tint: [f32; 4],
}

#[derive(Copy, Clone, Debug)]
pub enum RenderActorTag {
    Player,
    Ally(usize),
    Enemy(usize),
    StaticProp,
}

pub struct Player {
    pub prev_pos: Vec3,
    pub pos: Vec3,
    pub vel: Vec3,
    pub prev_yaw: f32,
    pub yaw: f32,
    pub grounded: bool,
    pub coyote_timer: f32,
    pub jump_buffer_timer: f32,
    was_space_pressed: bool,
    was_dodge_pressed: bool,
}

impl Player {
    pub fn new(spawn_pos: Vec3) -> Self {
        Self {
            prev_pos: spawn_pos,
            pos: spawn_pos,
            vel: Vec3::ZERO,
            prev_yaw: 0.0,
            yaw: 0.0,
            grounded: true,
            coyote_timer: COYOTE_TIME,
            jump_buffer_timer: 0.0,
            was_space_pressed: false,
            was_dodge_pressed: false,
        }
    }

    pub fn horizontal_speed(&self) -> f32 {
        Vec2::new(self.vel.x, self.vel.z).length()
    }

    pub fn interpolated_pos(&self, alpha: f32) -> Vec3 {
        self.prev_pos.lerp(self.pos, alpha.clamp(0.0, 1.0))
    }

    pub fn interpolated_yaw(&self, alpha: f32) -> f32 {
        lerp_angle(self.prev_yaw, self.yaw, alpha.clamp(0.0, 1.0))
    }

    pub fn interpolated_cube_model(&self, alpha: f32, proc_local: Mat4) -> Mat4 {
        let pos = self.interpolated_pos(alpha);
        let yaw = self.interpolated_yaw(alpha);
        let base = Mat4::from_scale_rotation_translation(Vec3::ONE, Quat::from_rotation_y(yaw), pos) * proc_local;
        base * Mat4::from_scale_rotation_translation(
            PLAYER_SCALE,
            Quat::IDENTITY,
            Vec3::new(0.0, PLAYER_SCALE.y * 0.5, 0.0),
        )
    }

    pub fn interpolated_gltf_model(&self, alpha: f32, yaw_offset: f32, proc_local: Mat4) -> Mat4 {
        let pos = self.interpolated_pos(alpha);
        let yaw = self.interpolated_yaw(alpha) + yaw_offset;
        Mat4::from_scale_rotation_translation(Vec3::ONE, Quat::from_rotation_y(yaw), pos) * proc_local
    }
}

pub struct World {
    pub player: Player,
    pub combat: PlayerCombat,
    pub animator: PlayerAnimator,
    pub collision: CollisionGrid,
    pub nav: NavGrid,
    pub nav_scratch: NavSearchScratch,
    pub astar_scheduler: AstarBudgetScheduler,
    pub squad: Squad,
    pub enemies: Vec<Enemy>,
    pub waves: WaveManager,
    pub skill_inv: SkillInventory,
    pub menu: MenuState,
    pub game_flow: GameFlow,
    pub playtime_seconds: f64,
    pub total_kills: u32,
    pub autosave_requested: bool,
    pub projectiles: ProjectileManager,
    pub effects: EffectsPool,
    pub pickups: PickupManager,
    pub floating_numbers: FloatingNumberPool,
    pub debug_draw_enabled: bool,
    pub ai_debug_enabled: bool,
    spawn_pos: Vec3,
    player_model_id: Option<usize>,
    ally_model_id: Option<usize>,
    enemy_model_id: Option<usize>,
    ally_skeleton: Option<Skeleton>,
    ally_clips: Vec<AnimationClip>,
    ally_clip_map: ClipMap,
    ally_anim_mode: AnimMode,
    enemy_skeleton: Option<Skeleton>,
    enemy_clips: Vec<AnimationClip>,
    enemy_clip_map: ClipMap,
    enemy_anim_mode: AnimMode,
    combatant_scratch: Vec<CombatantInfo>,
    yaw_offset_quarters: u8,
    model_yaw_offset: f32,
    static_cubes: Vec<InstanceData>,
    prop_instances: Vec<ModelInstance>,
}

impl World {
    pub fn new(
        player_model_id: Option<usize>,
        ally_model_id: Option<usize>,
        enemy_model_id: Option<usize>,
        prop_model_id: Option<usize>,
        player_skeleton: Option<Skeleton>,
        player_clips: Vec<AnimationClip>,
        ally_skeleton: Option<Skeleton>,
        ally_clips: Vec<AnimationClip>,
        enemy_skeleton: Option<Skeleton>,
        enemy_clips: Vec<AnimationClip>,
    ) -> Self {
        let mut rng = DeterministicRng::new(WORLD_SEED);
        let mut static_cubes = Vec::with_capacity(STATIC_CUBE_COUNT);
        let mut aabbs = Vec::with_capacity(STATIC_CUBE_COUNT);
        let half_bound = GROUND_SIZE * 0.5 - 1.5;

        for _ in 0..STATIC_CUBE_COUNT {
            let (x, z) = loop {
                let x = rng.range_f32(-half_bound, half_bound);
                let z = rng.range_f32(-half_bound, half_bound);
                if x * x + z * z > 6.25 {
                    break (x, z);
                }
            };
            let scale_xz = rng.range_f32(0.6, 1.6);
            let scale_y = rng.range_f32(0.6, 2.4);
            let scale = Vec3::new(scale_xz, scale_y, scale_xz);
            let _unused_yaw = rng.range_f32(0.0, std::f32::consts::TAU);
            let pos = Vec3::new(x, scale_y * 0.5, z);

            let model = Mat4::from_scale_rotation_translation(scale, Quat::IDENTITY, pos);
            aabbs.push(Aabb::from_unit_cube_matrix(model));

            let r = rng.range_f32(0.08, 0.92);
            let g = rng.range_f32(0.08, 0.92);
            let b = rng.range_f32(0.08, 0.92);
            static_cubes.push(InstanceData { model, color: [r, g, b, 1.0] });
        }

        let nav = NavGrid::new(&aabbs);
        let collision = CollisionGrid::new(aabbs);
        let spawn_pos = find_free_spawn_point(&collision, PLAYER_RADIUS, PLAYER_HEIGHT);

        let mut prop_instances = Vec::new();
        if let Some(prop_id) = prop_model_id {
            prop_instances.reserve(PROP_MAX_INSTANCES);
            for _ in 0..PROP_MAX_INSTANCES {
                let (x, z) = loop {
                    let x = rng.range_f32(-half_bound, half_bound);
                    let z = rng.range_f32(-half_bound, half_bound);
                    if x * x + z * z > 9.0 {
                        break (x, z);
                    }
                };
                let uniform_scale = rng.range_f32(0.75, 1.35);
                let yaw = rng.range_f32(0.0, std::f32::consts::TAU);
                let model = Mat4::from_scale_rotation_translation(
                    Vec3::splat(uniform_scale),
                    Quat::from_rotation_y(yaw),
                    Vec3::new(x, 0.0, z),
                );
                let tint = [rng.range_f32(0.85, 1.0), rng.range_f32(0.85, 1.0), rng.range_f32(0.85, 1.0), 1.0];
                prop_instances.push(ModelInstance { model_id: prop_id, model, tint });
            }
        }

        let ally_clip_map = ClipMap::from_clips(&ally_clips);
        let ally_anim_mode = if ally_skeleton.is_some() && !ally_clips.is_empty() {
            AnimMode::Skeletal
        } else {
            AnimMode::Procedural
        };
        let enemy_clip_map = ClipMap::from_clips(&enemy_clips);
        let enemy_anim_mode = if enemy_skeleton.is_some() && !enemy_clips.is_empty() {
            AnimMode::Skeletal
        } else {
            AnimMode::Procedural
        };

        let mut world = Self {
            player: Player::new(spawn_pos),
            combat: PlayerCombat::new(),
            animator: PlayerAnimator::new(player_skeleton, player_clips),
            collision,
            nav,
            nav_scratch: NavSearchScratch::new(),
            astar_scheduler: AstarBudgetScheduler::new(),
            squad: Squad::new(WORLD_SEED),
            enemies: Vec::with_capacity(MAX_ALIVE_ENEMIES),
            waves: WaveManager::new(WORLD_SEED),
            skill_inv: SkillInventory::new(),
            menu: MenuState::new(),
            game_flow: GameFlow::MainMenu,
            playtime_seconds: 0.0,
            total_kills: 0,
            autosave_requested: false,
            projectiles: ProjectileManager::new(),
            effects: EffectsPool::new(),
            pickups: PickupManager::new(WORLD_SEED),
            floating_numbers: FloatingNumberPool::new(),
            debug_draw_enabled: false,
            ai_debug_enabled: false,
            spawn_pos,
            player_model_id,
            ally_model_id,
            enemy_model_id,
            ally_skeleton,
            ally_clips,
            ally_clip_map,
            ally_anim_mode,
            enemy_skeleton,
            enemy_clips,
            enemy_clip_map,
            enemy_anim_mode,
            combatant_scratch: Vec::with_capacity(1 + MAX_ALLIES + MAX_ALIVE_ENEMIES),
            yaw_offset_quarters: 0,
            model_yaw_offset: MODEL_YAW_OFFSET,
            static_cubes,
            prop_instances,
        };

        let node_count = world.enemy_skeleton.as_ref().map_or(0, |s| s.nodes.len());
        let init_clip = world.enemy_clip_map.walk.unwrap_or(0);
        world.waves.update(
            0.0,
            &mut world.enemies,
            world.player.pos,
            0,
            &world.collision,
            &world.nav,
            node_count,
            init_clip,
        );

        world
    }

    pub fn new_game(&mut self) {
        self.player = Player::new(self.spawn_pos);
        self.combat.reset();
        self.squad.reset(WORLD_SEED);
        self.enemies.clear();
        self.waves.reset(WORLD_SEED);
        self.skill_inv.reset();
        self.menu.close(&mut self.game_flow);
        self.game_flow = GameFlow::Playing;
        self.playtime_seconds = 0.0;
        self.total_kills = 0;
        self.autosave_requested = false;
        self.projectiles.clear();
        self.effects.clear();
        self.pickups.reset(WORLD_SEED);
        self.floating_numbers.clear();

        let node_count = self.enemy_skeleton.as_ref().map_or(0, |s| s.nodes.len());
        let init_clip = self.enemy_clip_map.walk.unwrap_or(0);
        self.waves.update(
            0.0,
            &mut self.enemies,
            self.player.pos,
            self.squad.living_count(),
            &self.collision,
            &self.nav,
            node_count,
            init_clip,
        );
    }

    pub fn reset(&mut self) {
        self.new_game();
        println!("Game reset (R): Squad cleared, Wave 1 started.");
    }

    pub fn to_save(&self) -> SaveData {
        let wave_phase = if self.waves.pending_spawns == 0 && self.alive_enemy_count() == 0 {
            WavePhase::Intermission
        } else {
            WavePhase::Active
        };

        let mut allies = Vec::with_capacity(self.squad.allies.len());
        for a in &self.squad.allies {
            let pool = a.rarity.name_pool();
            let name_index = pool.iter().position(|&n| n == a.name).unwrap_or(0);
            allies.push(SavedAlly {
                rarity: a.rarity,
                name_index,
                health: a.health,
                down: a.is_down(),
                pos: a.pos,
                hold_pos: a.hold_pos,
            });
        }

        let mut enemies = Vec::with_capacity(self.enemies.len());
        for e in &self.enemies {
            if e.is_alive() {
                enemies.push(SavedEnemy {
                    pos: e.pos,
                    health: e.health,
                });
            }
        }

        SaveData {
            version: SAVE_VERSION,
            created_unix: current_unix_seconds(),
            playtime_seconds: self.playtime_seconds,
            player_pos: self.player.pos,
            player_yaw: self.player.yaw,
            player_health: self.combat.health,
            player_mana: self.skill_inv.mana,
            total_kills: self.total_kills,
            wave_number: self.waves.current_wave,
            wave_phase,
            intermission_timer: self.waves.intermission_timer,
            pending_spawns: self.waves.pending_spawns,
            score: self.waves.score,
            skill_counts: self.skill_inv.counts,
            skill_slots: self.skill_inv.slots,
            squad_mode: self.squad.mode,
            allies,
            enemies,
            wave_rng_state: self.waves.rng_state(),
            squad_rng_state: self.squad.rng_state,
            pickup_rng_state: self.pickups.rng_state,
            first_wave_drop_pending: self.pickups.first_wave_drop_pending,
        }
    }

    pub fn apply_save(&mut self, raw_save: &SaveData) {
        let mut save = raw_save.clone();
        save.validate_and_sanitize(&self.collision);

        self.projectiles.clear();
        self.effects.clear();
        self.pickups.reset(WORLD_SEED);
        self.pickups.rng_state = save.pickup_rng_state;
        self.pickups.first_wave_drop_pending = save.first_wave_drop_pending;
        self.floating_numbers.clear();
        self.menu.close(&mut self.game_flow);
        self.game_flow = GameFlow::Playing;
        self.playtime_seconds = save.playtime_seconds;
        self.total_kills = save.total_kills;
        self.autosave_requested = false;

        self.player = Player::new(save.player_pos);
        self.player.yaw = save.player_yaw;
        self.player.prev_yaw = save.player_yaw;

        self.combat.reset();
        self.combat.health = save.player_health;

        self.skill_inv.reset();
        self.skill_inv.counts = save.skill_counts;
        self.skill_inv.slots = save.skill_slots;
        self.skill_inv.mana = save.player_mana;

        self.squad.reset(WORLD_SEED);
        self.squad.mode = save.squad_mode;
        self.squad.rng_state = save.squad_rng_state;
        let ally_nodes = self.ally_skeleton.as_ref().map_or(0, |s| s.nodes.len());
        let ally_clip = self.ally_clip_map.idle.unwrap_or(0);
        for (idx, sa) in save.allies.iter().enumerate() {
            let pool = sa.rarity.name_pool();
            let name = pool[sa.name_index % pool.len()];
            let mut ally = crate::ally::Ally::new(
                (idx + 1) as u32,
                name,
                sa.rarity,
                sa.pos,
                save.player_yaw,
                ally_nodes,
                ally_clip,
            );
            ally.spawn_fx_timer = 0.0;
            ally.hold_pos = sa.hold_pos;
            if sa.down {
                ally.health = 0.0;
                ally.state = crate::ally::AllyState::Down;
                ally.down_timer = 0.5;
            } else {
                ally.health = sa.health;
            }
            self.squad.allies.push(ally);
        }

        self.enemies.clear();
        let enemy_nodes = self.enemy_skeleton.as_ref().map_or(0, |s| s.nodes.len());
        let enemy_clip = self.enemy_clip_map.walk.unwrap_or(0);
        for (idx, se) in save.enemies.iter().enumerate() {
            let mut e = Enemy::new(
                (idx + 1) as u32,
                se.pos,
                save.wave_rng_state ^ ((idx as u64 + 1) * 0x9E37_79B9_7F4A_7C15),
                enemy_nodes,
                enemy_clip,
            );
            e.health = se.health;
            e.state = EnemyState::Wander;
            self.enemies.push(e);
        }

        self.waves.restore_state(
            save.wave_number,
            match save.wave_phase {
                WavePhase::Active => save.pending_spawns,
                WavePhase::Intermission => 0,
            },
            save.intermission_timer,
            save.score,
            save.wave_rng_state,
            (save.enemies.len() + 1) as u32,
        );
    }

    pub fn summon_ally(&mut self) -> bool {
        let node_count = self.ally_skeleton.as_ref().map_or(0, |s| s.nodes.len());
        let init_clip = self.ally_clip_map.idle.unwrap_or(0);
        self.squad.try_summon(
            &mut self.waves.score,
            self.player.pos,
            self.player.yaw,
            &self.collision,
            node_count,
            init_clip,
        )
    }

    pub fn set_squad_mode(&mut self, mode: SquadMode) {
        self.squad.set_mode(mode);
    }

    pub fn try_fuse_skills(&mut self) {
        match self.skill_inv.try_fuse_first_available() {
            Ok((a, b, res)) => {
                let mut msg = String::with_capacity(64);
                let _ = write!(&mut msg, "FUSED: {} + {} = {}", a.def().name, b.def().name, res.def().name);
                self.squad.set_status_message_colored(&msg, res.def().tint);
            }
            Err(err_msg) => {
                self.squad.set_status_message(err_msg);
            }
        }
    }

    pub fn try_fuse_selected_menu_recipe(&mut self) {
        match self.menu.try_fuse_selected_recipe(&mut self.skill_inv) {
            Ok((a, b, res)) => {
                let mut msg = String::with_capacity(64);
                let _ = write!(&mut msg, "FUSED: {} + {} = {}", a.def().name, b.def().name, res.def().name);
                self.squad.set_status_message_colored(&msg, res.def().tint);
            }
            Err(err_msg) => {
                self.squad.set_status_message(err_msg);
            }
        }
    }

    pub fn cast_skill_slot(&mut self, slot_idx: usize, cam_forward_xz: Vec3, mouse_captured: bool) -> bool {
        let (skill, level) = match self.skill_inv.try_cast_slot(
            slot_idx,
            self.combat.is_dead(),
            self.combat.is_dodging(),
            mouse_captured,
        ) {
            Ok(pair) => pair,
            Err(err) => {
                self.squad.set_status_message(err.message());
                return false;
            }
        };

        let def = skill.def();
        let mult = level_multiplier(level);
        let scaled_power = def.power * mult;
        let aim_dir = compute_aim_direction_xz(self.player.pos, cam_forward_xz, &self.enemies);
        self.player.yaw = f32::atan2(-aim_dir.x, -aim_dir.y);

        match def.kind {
            SkillKind::Projectile { pierce, .. } => {
                let origin_xz = Vec2::new(self.player.pos.x, self.player.pos.z) + aim_dir * 0.65;
                self.projectiles.spawn_projectile(origin_xz, aim_dir, skill, level, pierce);
            }
            SkillKind::HealPulse => {
                self.apply_squad_heal_pulse(self.player.pos, def.radius, scaled_power);
                self.effects.spawn_ring(self.player.pos, def.radius, 0.45, def.tint);
            }
            SkillKind::Shockwave { knockback } => {
                self.apply_aoe_around_player(scaled_power, def.radius, knockback);
                self.effects.spawn_ring(self.player.pos, def.radius, 0.40, def.tint);
            }
            SkillKind::Dash { leaves_slow_zone } => {
                self.skill_inv.dash_timer = def.duration;
                self.skill_inv.dash_vel_xz = aim_dir * def.speed;
                self.skill_inv.dash_leaves_slow = leaves_slow_zone;
                self.skill_inv.slow_drop_accum = 0.0;
                self.combat.skill_invuln_timer = def.duration;
                if leaves_slow_zone {
                    self.projectiles.spawn_slow_zone(self.player.pos);
                    self.effects.spawn_ring(self.player.pos, 2.5, 2.0, def.tint);
                }
            }
            SkillKind::Meteor { mark_dist, delay, knockback } => {
                self.projectiles.spawn_meteor(
                    self.player.pos,
                    aim_dir,
                    mark_dist,
                    delay,
                    def.radius,
                    scaled_power,
                    knockback,
                    &self.collision,
                );
                if let Some(m) = self.projectiles.meteors.iter().find(|m| m.active && (m.timer - delay).abs() < 1e-4) {
                    self.effects.spawn_meteor_telegraph(m.pos, def.radius, delay, def.tint);
                }
            }
            SkillKind::HolyNova { heal_radius, enemy_damage, enemy_radius, knockback } => {
                self.apply_squad_heal_pulse(self.player.pos, heal_radius, scaled_power);
                self.apply_aoe_around_player(enemy_damage * mult, enemy_radius, knockback);
                self.effects.spawn_ring(self.player.pos, enemy_radius, 0.50, def.tint);
            }
        }
        true
    }

    fn apply_squad_heal_pulse(&mut self, center_pos: Vec3, radius: f32, heal_amt: f32) {
        let c_xz = Vec2::new(center_pos.x, center_pos.z);
        if !self.combat.is_dead() && Vec2::new(self.player.pos.x, self.player.pos.z).distance(c_xz) <= radius {
            self.combat.health = (self.combat.health + heal_amt).min(PLAYER_MAX_HEALTH);
            self.floating_numbers.spawn_kind(self.player.pos + Vec3::new(0.0, 2.2, 0.0), heal_amt.round() as u32, FloatingKind::Heal);
            self.effects.spawn_heal_particles(self.player.pos);
        }
        for ally in &mut self.squad.allies {
            if ally.is_living() && Vec2::new(ally.pos.x, ally.pos.z).distance(c_xz) <= radius {
                ally.health = (ally.health + heal_amt).min(ally.max_health);
                self.floating_numbers.spawn_kind(ally.pos + Vec3::new(0.0, 2.2, 0.0), heal_amt.round() as u32, FloatingKind::Heal);
                self.effects.spawn_heal_particles(ally.pos);
            }
        }
    }

    fn apply_aoe_around_player(&mut self, damage: f32, radius: f32, knockback: f32) {
        let p_pos = self.player.pos;
        let p_xz = Vec2::new(p_pos.x, p_pos.z);
        for idx in 0..self.enemies.len() {
            if !self.enemies[idx].is_alive() {
                continue;
            }
            let e_pos = self.enemies[idx].pos;
            let dist = Vec2::new(e_pos.x, e_pos.z).distance(p_xz);
            if dist <= radius {
                let hit = SkillDamageHit {
                    enemy_id: self.enemies[idx].id,
                    damage,
                    source_pos: p_pos,
                    knockback,
                    apply_burn: false,
                    apply_slow: false,
                };
                let killed = apply_skill_hit_to_enemy(&mut self.enemies[idx], &hit);
                self.floating_numbers.spawn(e_pos + Vec3::new(0.0, 2.1, 0.0), damage.round() as u32, false);
                if killed {
                    self.total_kills += 1;
                    self.waves.add_kill_score();
                    let _ = self.pickups.on_enemy_killed(e_pos, self.waves.current_wave);
                }
            }
        }
    }

    pub fn model_yaw_offset(&self) -> f32 {
        self.model_yaw_offset
    }

    pub fn alive_enemy_count(&self) -> u32 {
        self.enemies.iter().filter(|e| e.is_alive()).count() as u32
    }

    pub fn enemy_state_counts(&self) -> [u32; 5] {
        let mut counts = [0u32; 5];
        for e in &self.enemies {
            match e.state {
                EnemyState::Wander => counts[0] += 1,
                EnemyState::Chase => counts[1] += 1,
                EnemyState::Attack => counts[2] += 1,
                EnemyState::Recover => counts[3] += 1,
                EnemyState::Dead => counts[4] += 1,
            }
        }
        counts
    }

    pub fn cycle_model_yaw_offset(&mut self) -> f32 {
        self.yaw_offset_quarters = (self.yaw_offset_quarters + 1) % 4;
        self.model_yaw_offset = (MODEL_YAW_OFFSET
            + (self.yaw_offset_quarters as f32) * std::f32::consts::FRAC_PI_2)
            .rem_euclid(std::f32::consts::TAU);
        println!(
            "MODEL_YAW_OFFSET cycled (F5): {:.0} deg ({:.4} rad)",
            self.model_yaw_offset.to_degrees(),
            self.model_yaw_offset
        );
        self.model_yaw_offset
    }

    pub fn toggle_debug_draw(&mut self) -> bool {
        self.debug_draw_enabled = !self.debug_draw_enabled;
        println!("Debug wireframe AABB + skeleton overlay toggled (F7): {}", if self.debug_draw_enabled { "ON" } else { "OFF" });
        self.debug_draw_enabled
    }

    pub fn toggle_ai_debug(&mut self) -> bool {
        self.ai_debug_enabled = !self.ai_debug_enabled;
        println!("AI debug overlay (cones, paths, slots, nav cells) toggled (F8): {}", if self.ai_debug_enabled { "ON" } else { "OFF" });
        self.ai_debug_enabled
    }

    pub fn update_animation(&mut self, real_dt: f32) {
        let speed = self.player.horizontal_speed();
        let grounded = self.player.grounded;
        let vel_y = self.player.vel.y;
        self.animator.update(real_dt, speed, grounded, vel_y);

        let ally_skel = self.ally_skeleton.as_ref();
        for ally in &mut self.squad.allies {
            ally.update_animation(real_dt, ally_skel, &self.ally_clips, &self.ally_clip_map);
        }

        let enemy_skel = self.enemy_skeleton.as_ref();
        for enemy in &mut self.enemies {
            enemy.update_animation(real_dt, enemy_skel, &self.enemy_clips, &self.enemy_clip_map);
        }
    }

    pub fn player_world_matrix(&self, alpha: f32) -> Mat4 {
        let proc_local = self.animator.procedural_local_matrix(
            self.player.horizontal_speed(),
            self.player.grounded,
            self.player.vel.y,
            self.combat.swing_lunge_factor(),
        );
        self.player.interpolated_gltf_model(alpha, self.model_yaw_offset, proc_local)
    }

    pub fn step_if_unpaused(&mut self, input: &mut Input, camera: &Camera, dt: f32) -> Option<AiStepStats> {
        if self.game_flow.blocks_world_update() || self.game_flow.is_paused() {
            return None;
        }
        Some(self.update(input, camera, dt))
    }

    pub fn update(&mut self, input: &mut Input, camera: &Camera, dt: f32) -> AiStepStats {
        self.playtime_seconds += f64::from(dt);
        self.player.prev_pos = self.player.pos;
        self.player.prev_yaw = self.player.yaw;

        if input.take_attack_requested() {
            self.combat.try_start_swing();
        }

        let sprinting = !self.combat.is_dead()
            && !self.combat.is_swinging()
            && !self.combat.is_dodging()
            && (input.is_pressed(KeyCode::ShiftLeft) || input.is_pressed(KeyCode::ShiftRight));

        if !self.combat.is_dead() {
            let space_now = input.is_pressed(KeyCode::Space);
            if space_now && !self.player.was_space_pressed {
                self.player.jump_buffer_timer = JUMP_BUFFER_TIME;
            } else {
                self.player.jump_buffer_timer = (self.player.jump_buffer_timer - dt).max(0.0);
            }
            self.player.was_space_pressed = space_now;

            let forward = camera.forward_xz();
            let right = camera.right_xz();

            let mut move_dir = Vec3::ZERO;
            if input.is_pressed(KeyCode::KeyW) { move_dir += forward; }
            if input.is_pressed(KeyCode::KeyS) { move_dir -= forward; }
            if input.is_pressed(KeyCode::KeyD) { move_dir += right; }
            if input.is_pressed(KeyCode::KeyA) { move_dir -= right; }

            let dodge_now = input.is_pressed(KeyCode::KeyC);
            if dodge_now && !self.player.was_dodge_pressed {
                let wish_xz = Vec2::new(move_dir.x, move_dir.z);
                self.combat.try_start_dodge(self.player.grounded, wish_xz, self.player.yaw);
            }
            self.player.was_dodge_pressed = dodge_now;

            if self.skill_inv.is_skill_dashing() {
                let dv = self.skill_inv.dash_vel_xz;
                self.player.vel.x = dv.x;
                self.player.vel.z = dv.y;
                if self.skill_inv.dash_leaves_slow {
                    self.skill_inv.slow_drop_accum += dt;
                    if self.skill_inv.slow_drop_accum >= 0.06 {
                        self.skill_inv.slow_drop_accum = 0.0;
                        self.projectiles.spawn_slow_zone(self.player.pos);
                        self.effects.spawn_ring(self.player.pos, 2.5, 0.35, SkillId::FrostDash.def().tint);
                    }
                }
            } else if let Some(dodge_vel) = self.combat.dodge_velocity_xz() {
                self.player.vel.x = dodge_vel.x;
                self.player.vel.z = dodge_vel.y;
                if dodge_vel.length_squared() > 1e-4 {
                    self.player.yaw = f32::atan2(-dodge_vel.x, -dodge_vel.y);
                }
            } else {
                let mut max_speed = if sprinting { SPRINT_SPEED } else { WALK_SPEED };
                if self.combat.is_swinging() {
                    max_speed *= SWING_SPEED_MULT;
                }
                if self.skill_inv.is_casting() {
                    max_speed *= CAST_SPEED_MULT;
                }
                let (wish_vel_xz, has_wish) = if move_dir.length_squared() > 1e-5 {
                    let dir = move_dir.normalize();
                    let target_yaw = f32::atan2(-dir.x, -dir.z);
                    let turn_alpha = 1.0 - (-PLAYER_TURN_SPEED * dt).exp();
                    self.player.yaw = lerp_angle(self.player.yaw, target_yaw, turn_alpha);
                    (Vec2::new(dir.x, dir.z) * max_speed, true)
                } else {
                    (Vec2::ZERO, false)
                };

                let base_accel = if has_wish { GROUND_ACCEL } else { GROUND_DECEL };
                let accel = if self.player.grounded { base_accel } else { base_accel * AIR_CONTROL_FACTOR };
                let cur_vel_xz = Vec2::new(self.player.vel.x, self.player.vel.z);
                let new_vel_xz = move_towards_vec2(cur_vel_xz, wish_vel_xz, accel * dt);
                self.player.vel.x = new_vel_xz.x;
                self.player.vel.z = new_vel_xz.y;
            }

            self.player.pos.x += self.player.vel.x * dt;
            self.player.pos.z += self.player.vel.z * dt;
            self.collision.resolve_player_xz(
                &mut self.player.pos,
                &mut self.player.vel,
                PLAYER_RADIUS,
                PLAYER_HEIGHT,
                GROUND_SIZE * 0.5,
            );

            if !self.combat.is_dodging()
                && self.player.jump_buffer_timer > 0.0
                && (self.player.grounded || self.player.coyote_timer > 0.0)
            {
                self.player.vel.y = JUMP_SPEED;
                self.player.grounded = false;
                self.player.coyote_timer = 0.0;
                self.player.jump_buffer_timer = 0.0;
            }

            let prev_y = self.player.pos.y;
            self.player.vel.y -= GRAVITY * dt;
            let next_y = prev_y + self.player.vel.y * dt;
            let support_y = self.collision.support_height_at(
                self.player.pos.x,
                self.player.pos.z,
                PLAYER_RADIUS,
                prev_y,
            );

            if next_y <= support_y && self.player.vel.y <= 0.0 {
                self.player.pos.y = support_y;
                self.player.vel.y = 0.0;
                self.player.grounded = true;
                self.player.coyote_timer = COYOTE_TIME;

                if self.player.jump_buffer_timer > 0.0 && !self.combat.is_dodging() {
                    self.player.vel.y = JUMP_SPEED;
                    self.player.grounded = false;
                    self.player.coyote_timer = 0.0;
                    self.player.jump_buffer_timer = 0.0;
                }
            } else {
                self.player.pos.y = next_y;
                self.player.grounded = false;
                self.player.coyote_timer = (self.player.coyote_timer - dt).max(0.0);
            }
        } else {
            self.player.vel = Vec3::ZERO;
        }

        self.combat.update(dt);
        self.skill_inv.update(dt);

        if self.combat.is_swing_active_hitbox() {
            let (p_pos, p_yaw) = (self.player.pos, self.player.yaw);
            for enemy in &mut self.enemies {
                if enemy.is_alive()
                    && is_in_attack_arc(p_pos, p_yaw, enemy.pos, SWING_RANGE, SWING_ARC_RAD)
                    && self.combat.try_register_swing_hit(enemy.id)
                {
                    let e_pos = enemy.pos;
                    let killed = enemy.apply_hit(SWING_DAMAGE, p_pos);
                    self.floating_numbers.spawn(e_pos + Vec3::new(0.0, 2.1, 0.0), SWING_DAMAGE as u32, false);
                    if killed {
                        self.total_kills += 1;
                        self.waves.add_kill_score();
                        let _ = self.pickups.on_enemy_killed(e_pos, self.waves.current_wave);
                    }
                }
            }
        }

        // Update projectiles, meteor blasts, and slow zones
        let proj_events = self.projectiles.update(dt, &mut self.enemies, &self.collision);
        for i in 0..proj_events.hit_count {
            let hit = proj_events.hits[i];
            if let Some(enemy) = self.enemies.iter_mut().find(|e| e.id == hit.enemy_id && e.is_alive()) {
                let e_pos = enemy.pos;
                let killed = apply_skill_hit_to_enemy(enemy, &hit);
                self.floating_numbers.spawn(e_pos + Vec3::new(0.0, 2.1, 0.0), hit.damage.round() as u32, false);
                if killed {
                    self.total_kills += 1;
                    self.waves.add_kill_score();
                    let _ = self.pickups.on_enemy_killed(e_pos, self.waves.current_wave);
                }
            }
        }

        // Tick Burn and Slow status effects on enemies
        for enemy in &mut self.enemies {
            if !enemy.is_alive() {
                continue;
            }
            let (burn_dmg, burn_popup) = enemy.status.update(dt);
            if let Some(pop_amt) = burn_popup {
                self.floating_numbers.spawn(enemy.pos + Vec3::new(0.0, 2.25, 0.0), pop_amt, false);
            }
            if burn_dmg > 0.0 {
                enemy.health = (enemy.health - burn_dmg).max(0.0);
                if enemy.health <= 0.0 {
                    enemy.state = EnemyState::Dead;
                    enemy.state_timer = 0.0;
                    enemy.death_timer = 0.0;
                    enemy.vel = Vec3::ZERO;
                    self.total_kills += 1;
                    self.waves.add_kill_score();
                    let _ = self.pickups.on_enemy_killed(enemy.pos, self.waves.current_wave);
                }
            }
        }

        // Build shared CombatantInfo snapshot
        self.combatant_scratch.clear();
        self.combatant_scratch.push(CombatantInfo {
            id: ActorId::Player,
            faction: Faction::PlayerSide,
            pos: self.player.pos,
            active: !self.combat.is_dead(),
            sprinting,
        });
        for ally in &self.squad.allies {
            self.combatant_scratch.push(CombatantInfo {
                id: ActorId::Ally(ally.id),
                faction: Faction::PlayerSide,
                pos: ally.pos,
                active: ally.is_living(),
                sprinting: false,
            });
        }
        for enemy in &self.enemies {
            self.combatant_scratch.push(CombatantInfo {
                id: ActorId::Enemy(enemy.id),
                faction: Faction::EnemySide,
                pos: enemy.pos,
                active: enemy.is_alive(),
                sprinting: false,
            });
        }

        // Shared round-robin A* budget (max 3 per step across allies and enemies)
        let mut ally_reqs = [false; MAX_ALLIES];
        let mut enemy_reqs = [false; MAX_ALIVE_ENEMIES];
        self.squad.collect_astar_requests(self.player.pos, &self.enemies, &self.collision, &self.nav, &mut ally_reqs);
        collect_enemy_astar_requests(&self.enemies, &self.combatant_scratch, &self.collision, &self.nav, &mut enemy_reqs);

        let mut combined_reqs = [false; MAX_ALLIES + MAX_ALIVE_ENEMIES];
        let mut combined_granted = [false; MAX_ALLIES + MAX_ALIVE_ENEMIES];
        combined_reqs[..MAX_ALLIES].copy_from_slice(&ally_reqs);
        combined_reqs[MAX_ALLIES..].copy_from_slice(&enemy_reqs);

        self.astar_scheduler.begin_step();
        self.astar_scheduler.grant_round_robin(&combined_reqs, &mut combined_granted);

        let mut ally_granted = [false; MAX_ALLIES];
        let mut enemy_granted = [false; MAX_ALIVE_ENEMIES];
        ally_granted.copy_from_slice(&combined_granted[..MAX_ALLIES]);
        enemy_granted.copy_from_slice(&combined_granted[MAX_ALLIES..]);

        let holding_revive = !self.combat.is_dead() && input.is_pressed(KeyCode::KeyE);
        let player_hp_ratio = self.combat.health / PLAYER_MAX_HEALTH.max(1.0);
        let squad_out = self.squad.update(
            dt,
            self.player.pos,
            self.player.yaw,
            player_hp_ratio,
            holding_revive,
            &self.enemies,
            &self.combatant_scratch,
            &ally_granted,
            &self.collision,
            &self.nav,
            &mut self.nav_scratch,
        );

        if let Some((rev_pos, heal_amt)) = squad_out.revived_ally_pos_and_heal {
            self.floating_numbers.spawn_kind(rev_pos + Vec3::new(0.0, 2.2, 0.0), heal_amt, FloatingKind::Heal);
            self.effects.spawn_heal_particles(rev_pos);
        }

        // Execute Rare/Epic ally skill casts (Fireball or Heal Pulse)
        for i in 0..squad_out.skill_cast_count {
            if let Some(cast) = squad_out.skill_casts[i] {
                match cast {
                    AllySkillCast::Fireball { origin_xz, dir_xz } => {
                        self.projectiles.spawn_projectile(origin_xz, dir_xz, SkillId::Fireball, 1, 0);
                    }
                    AllySkillCast::HealPulse { caster_pos, amount } => {
                        self.apply_squad_heal_pulse(caster_pos, ALLY_HEAL_PULSE_RADIUS, amount);
                        self.effects.spawn_ring(caster_pos, ALLY_HEAL_PULSE_RADIUS, 0.45, SkillId::HealPulse.def().tint);
                    }
                }
            }
        }

        for i in 0..squad_out.hit_count {
            let hit = squad_out.hits[i];
            if let Some(enemy) = self.enemies.iter_mut().find(|e| e.id == hit.enemy_id && e.is_alive()) {
                let e_pos = enemy.pos;
                let killed = enemy.apply_hit_from(hit.damage, hit.attacker_pos, ActorId::Ally(hit.ally_id));
                self.floating_numbers.spawn(e_pos + Vec3::new(0.0, 2.1, 0.0), hit.damage.round() as u32, false);
                if killed {
                    self.total_kills += 1;
                    self.waves.add_kill_score();
                    let _ = self.pickups.on_enemy_killed(e_pos, self.waves.current_wave);
                }
            }
        }

        let mut ai_stats = update_enemies_with_targets(
            &mut self.enemies,
            &self.combatant_scratch,
            &enemy_granted,
            &self.collision,
            &self.nav,
            &mut self.nav_scratch,
            dt,
        );
        ai_stats.astar_calls += squad_out.astar_calls;
        ai_stats.astar_micros += squad_out.astar_micros;

        for i in 0..ai_stats.attack_count {
            let ev = ai_stats.attacks[i];
            match ev.target {
                ActorId::Player => {
                    if self.combat.apply_damage(ev.damage) {
                        self.floating_numbers.spawn(self.player.pos + Vec3::new(0.0, 2.2, 0.0), ev.damage.round() as u32, true);
                    }
                }
                ActorId::Ally(ally_id) => {
                    if let Some(ally) = self.squad.allies.iter_mut().find(|a| a.id == ally_id && a.is_living()) {
                        if ally.apply_damage(ev.damage) {
                            self.floating_numbers.spawn(ally.pos + Vec3::new(0.0, 2.1, 0.0), ev.damage.round() as u32, true);
                        }
                    }
                }
                ActorId::Enemy(_) => {}
            }
        }

        if let Some(outcome) = self.pickups.update(
            dt,
            self.player.pos,
            !self.combat.is_dead(),
            &mut self.skill_inv,
        ) {
            let mut msg = String::with_capacity(64);
            let sk = match outcome {
                PickupOutcome::Unlocked { skill, .. } => {
                    let _ = write!(&mut msg, "Picked up: {}", skill.def().name);
                    skill
                }
                PickupOutcome::LeveledUp { skill, new_level } => {
                    let _ = write!(&mut msg, "{} level {}", skill.def().name, new_level);
                    skill
                }
                PickupOutcome::ExtraCopy { skill, count } => {
                    let _ = write!(&mut msg, "Picked up: {} (x{})", skill.def().name, count);
                    skill
                }
            };
            let tc = sk.def().tint;
            self.squad.set_status_message_colored(&msg, tc);
            self.effects.spawn_pickup_sparkle(self.player.pos, tc);
        }

        self.effects.update(dt);
        self.floating_numbers.update(dt);

        let total_allies_once_revived = self.squad.allies.len() as u32;
        let node_count = self.enemy_skeleton.as_ref().map_or(0, |s| s.nodes.len());
        let init_clip = self.enemy_clip_map.walk.unwrap_or(0);
        let new_wave_started = self.waves.update(
            dt,
            &mut self.enemies,
            self.player.pos,
            total_allies_once_revived,
            &self.collision,
            &self.nav,
            node_count,
            init_clip,
        );

        if new_wave_started {
            self.autosave_requested = true;
            for (r_pos, heal_amt) in self.squad.revive_all_down_for_new_wave() {
                self.floating_numbers.spawn_kind(r_pos + Vec3::new(0.0, 2.2, 0.0), heal_amt, FloatingKind::Heal);
                self.effects.spawn_heal_particles(r_pos);
            }
        }

        if self.combat.is_dead() {
            self.game_flow = self.game_flow.on_player_death();
        }

        ai_stats
    }

    pub fn total_instance_count(&self) -> u32 {
        (1 + self.static_cubes.len() + 1 + self.squad.allies.len() + self.enemies.len() + self.prop_instances.len()) as u32
    }

    pub fn fill_visible_cubes(&self, alpha: f32, frustum: &Frustum, out: &mut Vec<InstanceData>) {
        out.clear();
        let (cube_min, cube_max) = (Vec3::splat(-0.5), Vec3::splat(0.5));

        for cube in &self.static_cubes {
            if frustum.intersects_local_aabb(cube.model, cube_min, cube_max) {
                out.push(*cube);
            }
        }

        let flash = self.combat.hit_flash_factor();
        if self.player_model_id.is_none() {
            let proc_local = self.animator.procedural_local_matrix(
                self.player.horizontal_speed(),
                self.player.grounded,
                self.player.vel.y,
                self.combat.swing_lunge_factor(),
            );
            let player_model = self.player.interpolated_cube_model(alpha, proc_local);
            if frustum.intersects_local_aabb(player_model, cube_min, cube_max) {
                out.push(InstanceData {
                    model: player_model,
                    color: [PLAYER_COLOR[0], PLAYER_COLOR[1], PLAYER_COLOR[2], 1.0 + flash],
                });
            }
        }

        if self.ally_model_id.is_none() {
            for ally in &self.squad.allies {
                let am = ally.world_matrix(alpha, 0.0, AnimMode::Procedural)
                    * Mat4::from_scale_rotation_translation(PLAYER_SCALE, Quat::IDENTITY, Vec3::new(0.0, PLAYER_SCALE.y * 0.5, 0.0));
                if frustum.intersects_local_aabb(am, cube_min, cube_max) {
                    out.push(InstanceData { model: am, color: ally.tint_with_flash() });
                }
            }
        }

        if self.enemy_model_id.is_none() {
            for enemy in &self.enemies {
                let em = enemy.world_matrix(alpha, 0.0, AnimMode::Procedural)
                    * Mat4::from_scale_rotation_translation(PLAYER_SCALE, Quat::IDENTITY, Vec3::new(0.0, PLAYER_SCALE.y * 0.5, 0.0));
                if frustum.intersects_local_aabb(em, cube_min, cube_max) {
                    out.push(InstanceData { model: em, color: enemy.tint_with_flash() });
                }
            }
        }

        self.projectiles.append_visible_cube_instances(frustum, out);
        self.pickups.append_visible_cube_instances(frustum, out);
        if out.len() > TOTAL_CUBE_INSTANCES {
            out.truncate(TOTAL_CUBE_INSTANCES);
        }
        self.effects.append_visible_cube_instances(frustum, |inst| {
            push_cube_instance_guarded(out, inst);
        });
    }

    pub fn fill_visible_models(
        &self,
        model_id: usize,
        local_min: Vec3,
        local_max: Vec3,
        alpha: f32,
        frustum: &Frustum,
        out: &mut Vec<InstanceData>,
        out_actor_tag: &mut Vec<RenderActorTag>,
    ) {
        out.clear();
        out_actor_tag.clear();

        let pad = Vec3::splat(0.5);
        let (c_min, c_max) = (local_min - pad, local_max + pad);

        if self.player_model_id == Some(model_id) {
            let player_model = self.player_world_matrix(alpha);
            if frustum.intersects_local_aabb(player_model, c_min, c_max) {
                let flash = self.combat.hit_flash_factor();
                out.push(InstanceData { model: player_model, color: [1.0, 1.0, 1.0, 1.0 + flash] });
                out_actor_tag.push(RenderActorTag::Player);
            }
        }

        if self.ally_model_id == Some(model_id) {
            for (idx, ally) in self.squad.allies.iter().enumerate() {
                let ally_mat = ally.world_matrix(alpha, self.model_yaw_offset, self.ally_anim_mode);
                if frustum.intersects_local_aabb(ally_mat, c_min, c_max) {
                    out.push(InstanceData { model: ally_mat, color: ally.tint_with_flash() });
                    out_actor_tag.push(RenderActorTag::Ally(idx));
                }
            }
        }

        if self.enemy_model_id == Some(model_id) {
            for (idx, enemy) in self.enemies.iter().enumerate() {
                let enemy_mat = enemy.world_matrix(alpha, self.model_yaw_offset, self.enemy_anim_mode);
                if frustum.intersects_local_aabb(enemy_mat, c_min, c_max) {
                    out.push(InstanceData { model: enemy_mat, color: enemy.tint_with_flash() });
                    out_actor_tag.push(RenderActorTag::Enemy(idx));
                }
            }
        }

        for prop in &self.prop_instances {
            if prop.model_id == model_id && frustum.intersects_local_aabb(prop.model, local_min, local_max) {
                out.push(InstanceData { model: prop.model, color: prop.tint });
                out_actor_tag.push(RenderActorTag::StaticProp);
            }
        }
    }
}

fn move_towards_vec2(current: Vec2, target: Vec2, max_delta: f32) -> Vec2 {
    let diff = target - current;
    let dist = diff.length();
    if dist <= max_delta || dist < 1e-6 {
        target
    } else {
        current + (diff / dist) * max_delta
    }
}

fn find_free_spawn_point(grid: &CollisionGrid, radius: f32, height: f32) -> Vec3 {
    let margin = radius + 0.15;
    if grid.is_circle_free(0.0, 0.0, margin, height) {
        return Vec3::ZERO;
    }
    for step in 1..200 {
        let r = (step as f32) * 0.5;
        let angle = (step as f32) * 0.85;
        let (x, z) = (r * angle.cos(), r * angle.sin());
        if grid.is_circle_free(x, z, margin, height) {
            return Vec3::new(x, 0.0, z);
        }
    }
    Vec3::ZERO
}

struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self { state: seed | 1 }
    }

    fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 32) as u32
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / ((1u32 << 24) as f32)
    }

    fn range_f32(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.next_f32()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ally::ALLY_RADIUS;
    use crate::collision::circle_overlaps_aabb_xz;
    use crate::combat::PLAYER_MAX_HEALTH;
    use crate::enemy::{ENEMY_MAX_HEALTH, ENEMY_RADIUS};
    use crate::squad::MAX_SHARED_ASTAR_BUDGET;

    fn test_world_with_single_cube(aabb: Aabb) -> World {
        let nav = NavGrid::new(&[aabb]);
        let collision = CollisionGrid::new(vec![aabb]);
        World {
            player: Player::new(Vec3::ZERO),
            combat: PlayerCombat::new(),
            animator: PlayerAnimator::new(None, Vec::new()),
            collision,
            nav,
            nav_scratch: NavSearchScratch::new(),
            astar_scheduler: AstarBudgetScheduler::new(),
            squad: Squad::new(WORLD_SEED),
            enemies: Vec::new(),
            waves: WaveManager::new(WORLD_SEED),
            skill_inv: SkillInventory::new(),
            menu: MenuState::new(),
            game_flow: GameFlow::Playing,
            playtime_seconds: 0.0,
            total_kills: 0,
            autosave_requested: false,
            projectiles: ProjectileManager::new(),
            effects: EffectsPool::new(),
            pickups: PickupManager::new(WORLD_SEED),
            floating_numbers: FloatingNumberPool::new(),
            debug_draw_enabled: false,
            ai_debug_enabled: false,
            spawn_pos: Vec3::ZERO,
            player_model_id: None,
            ally_model_id: None,
            enemy_model_id: None,
            ally_skeleton: None,
            ally_clips: Vec::new(),
            ally_clip_map: ClipMap::from_clips(&[]),
            ally_anim_mode: AnimMode::Procedural,
            enemy_skeleton: None,
            enemy_clips: Vec::new(),
            enemy_clip_map: ClipMap::from_clips(&[]),
            enemy_anim_mode: AnimMode::Procedural,
            combatant_scratch: Vec::new(),
            yaw_offset_quarters: 0,
            model_yaw_offset: MODEL_YAW_OFFSET,
            static_cubes: Vec::new(),
            prop_instances: Vec::new(),
        }
    }

    #[test]
    fn test_landing_on_cube_top() {
        let cube = Aabb { min: Vec3::new(-1.0, 0.0, -1.0), max: Vec3::new(1.0, 1.5, 1.0) };
        let mut world = test_world_with_single_cube(cube);
        world.waves.pending_spawns = 0;
        world.player.pos = Vec3::new(0.0, 2.2, 0.0);
        world.player.prev_pos = world.player.pos;
        world.player.grounded = false;
        world.player.coyote_timer = 0.0;

        let mut input = Input::new();
        let camera = Camera::new(Vec3::ZERO, 16.0 / 9.0);
        for _ in 0..45 {
            world.update(&mut input, &camera, 1.0 / 60.0);
        }
        assert!(world.player.grounded);
        assert!((world.player.pos.y - 1.5).abs() < 1e-4);
        assert!(world.player.vel.y.abs() < 1e-4);
    }

    #[test]
    fn test_coyote_time_allows_late_jump() {
        let ledge = Aabb { min: Vec3::new(-1.0, 0.0, -1.0), max: Vec3::new(1.0, 2.0, 1.0) };
        let mut world = test_world_with_single_cube(ledge);
        world.waves.pending_spawns = 0;
        world.player.pos = Vec3::new(2.0, 2.0, 0.0);
        world.player.grounded = true;
        world.player.coyote_timer = COYOTE_TIME;

        let mut input = Input::new();
        let camera = Camera::new(Vec3::ZERO, 16.0 / 9.0);
        let dt = 1.0 / 60.0;

        world.update(&mut input, &camera, dt);
        assert!(!world.player.grounded && world.player.coyote_timer > 0.0);

        input.set_key_state(KeyCode::Space, true, false);
        world.update(&mut input, &camera, dt);
        assert!(world.player.vel.y > JUMP_SPEED - 0.5);
        assert_eq!(world.player.coyote_timer, 0.0);
    }

    #[test]
    fn test_jump_buffer_triggers_on_landing() {
        let mut world = test_world_with_single_cube(Aabb {
            min: Vec3::new(10.0, 0.0, 10.0),
            max: Vec3::new(11.0, 1.0, 11.0),
        });
        world.waves.pending_spawns = 0;
        world.player.pos = Vec3::new(0.0, 0.06, 0.0);
        world.player.vel = Vec3::new(0.0, -3.0, 0.0);
        world.player.grounded = false;
        world.player.coyote_timer = 0.0;

        let mut input = Input::new();
        let camera = Camera::new(Vec3::ZERO, 16.0 / 9.0);
        let dt = 1.0 / 60.0;

        input.set_key_state(KeyCode::Space, true, false);
        world.update(&mut input, &camera, dt);
        assert!(world.player.jump_buffer_timer > 0.0);

        world.update(&mut input, &camera, dt);
        assert!(world.player.vel.y >= JUMP_SPEED - 0.01);
        assert_eq!(world.player.jump_buffer_timer, 0.0);
    }

    #[test]
    fn test_deterministic_600_step_simulation() {
        let mut world = World::new(None, None, None, None, None, Vec::new(), None, Vec::new(), None, Vec::new());
        assert_eq!(world.enemies.len(), 3);
        for enemy in &mut world.enemies {
            enemy.state = EnemyState::Chase;
            enemy.target_pos = world.player.pos;
        }

        let mut input = Input::new();
        let camera = Camera::new(world.player.pos, 16.0 / 9.0);
        let dt = 1.0 / 60.0;
        for _ in 0..630 {
            world.update(&mut input, &camera, dt);
            for enemy in &world.enemies {
                for aabb in world.collision.aabbs() {
                    assert!(!circle_overlaps_aabb_xz(enemy.pos.x, enemy.pos.z, ENEMY_RADIUS - 0.02, aabb));
                }
            }
        }
        assert!(world.combat.health < PLAYER_MAX_HEALTH);
    }

    #[test]
    fn test_deterministic_1200_step_squad_simulation() {
        let mut world = World::new(None, None, None, None, None, Vec::new(), None, Vec::new(), None, Vec::new());

        world.waves.score = 100;
        assert!(world.summon_ally());
        assert!(world.summon_ally());
        assert_eq!(world.squad.allies.len(), 2);

        world.waves.pending_spawns = 1;
        let mut input = Input::new();
        let camera = Camera::new(world.player.pos, 16.0 / 9.0);
        let dt = 1.0 / 60.0;
        world.update(&mut input, &camera, dt);
        assert_eq!(world.enemies.len(), 4);

        for enemy in &mut world.enemies {
            enemy.state = EnemyState::Chase;
            enemy.target_pos = world.player.pos;
        }

        let initial_total_enemy_hp: f32 = world.enemies.iter().map(|e| e.health).sum();
        assert!((initial_total_enemy_hp - 4.0 * ENEMY_MAX_HEALTH).abs() < 1e-4);

        let mut enemy_took_damage_from_allies = false;

        for _ in 0..1200 {
            let stats = world.update(&mut input, &camera, dt);
            assert!(
                stats.astar_calls as usize <= MAX_SHARED_ASTAR_BUDGET,
                "shared A* budget exceeded: {} > {}",
                stats.astar_calls,
                MAX_SHARED_ASTAR_BUDGET
            );

            let current_enemy_hp: f32 = world.enemies.iter().filter(|e| e.is_alive()).map(|e| e.health).sum();
            if current_enemy_hp < initial_total_enemy_hp || world.waves.score > 40 {
                enemy_took_damage_from_allies = true;
            }

            for aabb in world.collision.aabbs() {
                assert!(!circle_overlaps_aabb_xz(world.player.pos.x, world.player.pos.z, PLAYER_RADIUS - 0.02, aabb));
                for ally in &world.squad.allies {
                    assert!(!circle_overlaps_aabb_xz(ally.pos.x, ally.pos.z, ALLY_RADIUS - 0.02, aabb));
                }
                for enemy in &world.enemies {
                    assert!(!circle_overlaps_aabb_xz(enemy.pos.x, enemy.pos.z, ENEMY_RADIUS - 0.02, aabb));
                }
            }
        }

        assert!(
            enemy_took_damage_from_allies,
            "enemies must take damage from the 2 allies within 1200 steps"
        );
    }

    #[test]
    fn test_holy_nova_heals_allies_and_damages_enemies_and_wind_dash_stops_at_cube() {
        let wall = Aabb {
            min: Vec3::new(-2.0, 0.0, -3.5),
            max: Vec3::new(2.0, 2.5, -2.5),
        };
        let mut world = test_world_with_single_cube(wall);
        world.waves.pending_spawns = 0;

        // Wind Dash toward wall at z = -2.5 stops cleanly at cube face + radius
        let _ = world.skill_inv.add_skill_copy(crate::skills::SkillId::WindDash);
        world.skill_inv.slots[2] = Some(crate::skills::SkillId::WindDash);
        assert!(world.cast_skill_slot(2, Vec3::NEG_Z, true));
        let mut input = Input::new();
        let camera = Camera::new(Vec3::ZERO, 16.0 / 9.0);
        for _ in 0..15 {
            world.update(&mut input, &camera, 1.0 / 60.0);
        }
        assert!(world.player.pos.z >= -2.5 + PLAYER_RADIUS - 1e-3);

        // Holy Nova heals player + living ally and damages nearby enemy
        world.player.pos = Vec3::ZERO;
        world.combat.health = 50.0;
        world.waves.score = 60;
        assert!(world.summon_ally());
        world.squad.allies[0].health = 20.0;
        let ally_before = world.squad.allies[0].health;
        world.enemies.push(Enemy::new(99, Vec3::new(0.0, 0.0, 4.0), 1, 1, 0));

        let _ = world.skill_inv.add_skill_copy(crate::skills::SkillId::HolyNova);
        world.skill_inv.slots[3] = Some(crate::skills::SkillId::HolyNova);
        world.skill_inv.mana = 100.0;
        assert!(world.cast_skill_slot(3, Vec3::Z, true));
        assert!((world.combat.health - 90.0).abs() < 1e-3);
        assert!(world.squad.allies[0].health > ally_before);
        assert!((world.enemies[0].health - 20.0).abs() < 1e-3);
    }

    #[test]
    fn test_deterministic_900_step_fireball_simulation() {
        let mut world = World::new(None, None, None, None, None, Vec::new(), None, Vec::new(), None, Vec::new());
        assert_eq!(world.enemies.len(), 3);
        // Place the 3 enemies within 16m so Fireball casts reach them
        world.enemies[0].pos = Vec3::new(0.0, 0.0, -10.0);
        world.enemies[1].pos = Vec3::new(2.0, 0.0, -12.0);
        world.enemies[2].pos = Vec3::new(-2.0, 0.0, -14.0);
        for enemy in &mut world.enemies {
            enemy.state = EnemyState::Chase;
        }

        let mut input = Input::new();
        let camera = Camera::new(world.player.pos, 16.0 / 9.0);
        let dt = 1.0 / 60.0;
        let mut cast_accum = 1.2_f32;

        for _ in 0..900 {
            cast_accum += dt;
            if cast_accum >= 1.2 {
                cast_accum -= 1.2;
                let dir = if let Some(e) = world.enemies.iter().find(|e| e.is_alive()) {
                    (e.pos - world.player.pos).normalize_or_zero()
                } else {
                    Vec3::NEG_Z
                };
                let _ = world.cast_skill_slot(0, dir, true);
            }

            world.update(&mut input, &camera, dt);
            assert!(world.skill_inv.mana >= 0.0 && world.skill_inv.mana <= crate::skills::PLAYER_MAX_MANA);
            assert!(world.projectiles.active_projectile_count() as usize <= crate::projectiles::MAX_PROJECTILES);
            assert!(world.pickups.active_orb_count() as usize <= crate::pickups::MAX_SKILL_ORBS);
        }

        assert!(
            world.waves.score >= 10 || world.alive_enemy_count() < 3,
            "at least one enemy must die from Fireball casts within 900 steps"
        );
    }

    #[test]
    fn test_pause_check_blocks_fixed_update_and_instance_writer_guard() {
        let mut world = test_world_with_single_cube(Aabb {
            min: Vec3::new(10.0, 0.0, 10.0),
            max: Vec3::new(11.0, 1.0, 11.0),
        });
        world.waves.pending_spawns = 0;
        world.player.vel = Vec3::new(5.0, 0.0, 0.0);

        let mut input = Input::new();
        let camera = Camera::new(Vec3::ZERO, 16.0 / 9.0);

        world.menu.toggle(&mut world.game_flow);
        assert!(world.game_flow.is_paused());
        assert!(world.step_if_unpaused(&mut input, &camera, 1.0 / 60.0).is_none());
        assert_eq!(world.player.pos.x, 0.0);

        world.menu.toggle(&mut world.game_flow);
        assert!(!world.game_flow.is_paused());
        assert!(world.step_if_unpaused(&mut input, &camera, 1.0 / 60.0).is_some());
        assert!(world.player.pos.x > 0.0);

        // Test instance writer guard never exceeds TOTAL_CUBE_INSTANCES
        let mut buf = Vec::with_capacity(TOTAL_CUBE_INSTANCES);
        for _ in 0..(TOTAL_CUBE_INSTANCES + 50) {
            push_cube_instance_guarded(
                &mut buf,
                InstanceData {
                    model: Mat4::IDENTITY,
                    color: [1.0, 1.0, 1.0, 1.0],
                },
            );
        }
        assert_eq!(buf.len(), TOTAL_CUBE_INSTANCES);
    }

    #[test]
    fn test_deterministic_900_step_epic_ally_skill_simulation() {
        let mut world = World::new(None, None, None, None, None, Vec::new(), None, Vec::new(), None, Vec::new());
        assert_eq!(world.enemies.len(), 3);

        // Spawn 1 Epic ally near the stationary player and place 3 enemies within 10m
        let mut epic_ally = crate::ally::Ally::new(
            1,
            "Aurelia",
            crate::ally::Rarity::Epic,
            Vec3::new(1.5, 0.0, 0.0),
            0.0,
            1,
            0,
        );
        epic_ally.state = crate::ally::AllyState::Engage;
        world.squad.allies.push(epic_ally);

        world.enemies[0].pos = Vec3::new(0.0, 0.0, -9.0);
        world.enemies[1].pos = Vec3::new(2.0, 0.0, -10.0);
        world.enemies[2].pos = Vec3::new(-2.0, 0.0, -11.0);
        for e in &mut world.enemies {
            e.state = EnemyState::Chase;
        }
        let initial_enemy_hp: f32 = world.enemies.iter().map(|e| e.health).sum();

        let mut input = Input::new();
        let camera = Camera::new(world.player.pos, 16.0 / 9.0);
        let dt = 1.0 / 60.0;

        for _ in 0..900 {
            world.update(&mut input, &camera, dt);
            assert!(world.projectiles.active_projectile_count() as usize <= crate::projectiles::MAX_PROJECTILES);
            assert!(world.effects.active_count() as usize <= crate::effects::MAX_EFFECT_ENTRIES);
            assert!(world.pickups.active_orb_count() as usize <= crate::pickups::MAX_SKILL_ORBS);
        }

        let final_enemy_hp: f32 = world.enemies.iter().filter(|e| e.is_alive()).map(|e| e.health).sum();
        assert!(world.squad.total_ally_casts >= 1, "Epic ally must cast at least once");
        assert!(final_enemy_hp < initial_enemy_hp || world.waves.score > 0, "enemies must take damage");
    }

    #[test]
    fn test_world_save_apply_and_300_step_loaded_simulation_and_new_game() {
        let mut world = World::new(None, None, None, None, None, Vec::new(), None, Vec::new(), None, Vec::new());
        world.new_game();

        world.waves.score = 90;
        assert!(world.summon_ally());
        assert!(world.summon_ally());
        world.squad.allies[1].apply_damage(999.0);
        assert!(world.squad.allies[1].is_down());
        world.combat.health = 77.0;
        world.skill_inv.mana = 55.0;
        let _ = world.skill_inv.add_skill_copy(SkillId::IceShard);
        world.waves.current_wave = 4;
        world.enemies[0].health = 22.0;

        let saved = world.to_save();

        // Mutate world and then restore from `saved`
        world.new_game();
        assert_eq!(world.waves.current_wave, 1);
        assert_eq!(world.squad.allies.len(), 0);

        world.apply_save(&saved);
        assert_eq!(world.waves.current_wave, 4);
        assert_eq!(world.waves.score, 30);
        assert!((world.combat.health - 77.0).abs() < 1e-4);
        assert!((world.skill_inv.mana - 55.0).abs() < 1e-4);
        assert_eq!(world.skill_inv.count(SkillId::IceShard), 1);
        assert_eq!(world.squad.allies.len(), 2);
        assert!(world.squad.allies[1].is_down());
        assert!((world.enemies[0].health - 22.0).abs() < 1e-4);

        // Run 300 fixed updates on the restored world without panic and with no actor inside a cube
        let mut input = Input::new();
        let camera = Camera::new(world.player.pos, 16.0 / 9.0);
        for _ in 0..300 {
            world.update(&mut input, &camera, 1.0 / 60.0);
            for aabb in world.collision.aabbs() {
                assert!(!circle_overlaps_aabb_xz(world.player.pos.x, world.player.pos.z, PLAYER_RADIUS - 0.02, aabb));
                for ally in &world.squad.allies {
                    assert!(!circle_overlaps_aabb_xz(ally.pos.x, ally.pos.z, ALLY_RADIUS - 0.02, aabb));
                }
                for enemy in &world.enemies {
                    assert!(!circle_overlaps_aabb_xz(enemy.pos.x, enemy.pos.z, ENEMY_RADIUS - 0.02, aabb));
                }
            }
        }
    }
}
