
use crate::camera::Camera;
use crate::combat::PLAYER_MAX_HEALTH;
use crate::game_flow::GameFlow;
use crate::input::Input;
use crate::menu::{apply_setting_adjustment, layout, MenuAction, MenuScreen, MenuSystem};
use crate::model::{load_scene_models, CpuModel};
use crate::renderer::{RenderOutcome, Renderer};
use crate::save::{
    delete_save, list_saves, load_save_with_backup, most_recent_save_slot, write_save_atomic,
    SlotStatus, SLOT_FILES,
};
use crate::settings::Settings;
use crate::squad::SquadMode;
use crate::stats::{HudTelemetry, StatsTracker};
use crate::world::World;
use glam::Mat4;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowAttributes, WindowId};

pub const WINDOW_WIDTH: u32 = 1280;
pub const WINDOW_HEIGHT: u32 = 720;
pub const WINDOW_TITLE: &str = "PMU wgpu";

pub const FIXED_DT: f32 = 1.0 / 60.0;
pub const MAX_FRAME_DELTA: f32 = 0.25;
pub const MAX_CATCHUP_STEPS: u32 = 5;
pub const TRANSITION_FADE_DURATION: f32 = 0.20;

pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    pending_models: Vec<CpuModel>,
    base_dir: PathBuf,
    settings: Settings,
    save_statuses: [SlotStatus; 5],
    menu_sys: MenuSystem,
    world: World,
    camera: Camera,
    input: Input,
    stats: StatsTracker,
    mouse_captured: bool,
    fade_timer: f32,
    last_frame_instant: Option<Instant>,
    accumulator: f32,
}

impl App {
    pub fn new() -> Self {
        let base_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let settings = Settings::load_from_dir(&base_dir);
        let save_statuses = list_saves(&base_dir);

        let scene_models = load_scene_models();
        let (player_skeleton, player_clips) = scene_models
            .player_model_id
            .and_then(|id| scene_models.models.get(id))
            .map(|m| (m.skeleton.clone(), m.animations.clone()))
            .unwrap_or((None, Vec::new()));

        let (ally_skeleton, ally_clips) = scene_models
            .ally_model_id
            .and_then(|id| scene_models.models.get(id))
            .map(|m| (m.skeleton.clone(), m.animations.clone()))
            .unwrap_or((None, Vec::new()));

        let (enemy_skeleton, enemy_clips) = scene_models
            .enemy_model_id
            .and_then(|id| scene_models.models.get(id))
            .map(|m| (m.skeleton.clone(), m.animations.clone()))
            .unwrap_or((None, Vec::new()));

        let world = World::new(
            scene_models.player_model_id,
            scene_models.ally_model_id,
            scene_models.enemy_model_id,
            scene_models.prop_model_id,
            player_skeleton,
            player_clips,
            ally_skeleton,
            ally_clips,
            enemy_skeleton,
            enemy_clips,
        );
        let aspect = WINDOW_WIDTH as f32 / WINDOW_HEIGHT as f32;
        let mut camera = Camera::new(world.player.pos, aspect);
        camera.fov_y_radians = settings.fov_degrees.to_radians();
        camera.mouse_sens_mult = settings.mouse_sensitivity;
        camera.in_main_menu_orbit = true;

        let mut menu_sys = MenuSystem::new();
        menu_sys.open_root(MenuScreen::Main, &settings, &save_statuses);

        Self {
            window: None,
            renderer: None,
            pending_models: scene_models.models,
            base_dir,
            settings,
            save_statuses,
            menu_sys,
            world,
            camera,
            input: Input::new(),
            stats: StatsTracker::new(false),
            mouse_captured: false,
            fade_timer: 0.0,
            last_frame_instant: None,
            accumulator: 0.0,
        }
    }

    fn set_mouse_capture(&mut self, capture: bool) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        if capture {
            let grab_res = window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
            match grab_res {
                Ok(()) => {
                    window.set_cursor_visible(false);
                    self.mouse_captured = true;
                }
                Err(e) => {
                    eprintln!("Warning: could not grab cursor: {e}");
                    self.mouse_captured = false;
                }
            }
        } else {
            let _ = window.set_cursor_grab(CursorGrabMode::None);
            window.set_cursor_visible(true);
            self.mouse_captured = false;
        }
    }

    fn sync_settings_to_systems(&mut self) {
        self.camera.fov_y_radians = self.settings.fov_degrees.to_radians();
        self.camera.mouse_sens_mult = self.settings.mouse_sensitivity;
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.apply_settings(&self.settings);
        }
        let _ = self.settings.save_to_dir(&self.base_dir);
    }

    fn save_to_slot(&mut self, slot_idx: usize, toast_label: &str) {
        let file_name = SLOT_FILES[slot_idx % SLOT_FILES.len()];
        let data = self.world.to_save();
        match write_save_atomic(&self.base_dir, file_name, &data) {
            Ok(()) => {
                self.save_statuses = list_saves(&self.base_dir);
                self.menu_sys
                    .rebuild_items(&self.settings, &self.save_statuses);
                self.world
                    .squad
                    .set_status_message_colored(toast_label, [0.35, 0.98, 0.50]);
            }
            Err(e) => {
                let msg = format!("Save failed: {}", e.message());
                self.world
                    .squad
                    .set_status_message_colored(&msg, [1.0, 0.30, 0.25]);
            }
        }
    }

    fn load_from_slot(&mut self, slot_idx: usize) {
        let file_name = SLOT_FILES[slot_idx % SLOT_FILES.len()];
        match load_save_with_backup(&self.base_dir, file_name) {
            Ok((data, used_bak)) => {
                self.world.apply_save(&data);
                self.camera.focus = self.world.player.pos;
                self.camera.in_main_menu_orbit = false;
                self.fade_timer = TRANSITION_FADE_DURATION;
                self.save_statuses = list_saves(&self.base_dir);
                let msg = if used_bak {
                    "Game loaded (restored from backup)"
                } else {
                    "Game loaded"
                };
                self.world
                    .squad
                    .set_status_message_colored(msg, [0.40, 0.95, 1.0]);
            }
            Err(e) => {
                let msg = format!("Load failed: {}", e.message());
                self.world
                    .squad
                    .set_status_message_colored(&msg, [1.0, 0.30, 0.25]);
            }
        }
    }

    fn execute_menu_action(&mut self, action: MenuAction, event_loop: &ActiveEventLoop) {
        match action {
            MenuAction::None => {}
            MenuAction::Continue | MenuAction::LoadLast => {
                self.save_statuses = list_saves(&self.base_dir);
                if let Some(slot_idx) = most_recent_save_slot(&self.save_statuses) {
                    self.load_from_slot(slot_idx);
                }
            }
            MenuAction::NewGame => {
                self.world.new_game();
                self.camera.focus = self.world.player.pos;
                self.camera.in_main_menu_orbit = false;
                self.fade_timer = TRANSITION_FADE_DURATION;
            }
            MenuAction::Retry => {
                self.world.new_game();
                self.world.game_flow = self.world.game_flow.on_retry();
                self.camera.focus = self.world.player.pos;
                self.camera.in_main_menu_orbit = false;
                self.fade_timer = TRANSITION_FADE_DURATION;
            }
            MenuAction::ResumeGame => {
                self.world.game_flow = self.world.game_flow.on_start_or_resume();
            }
            MenuAction::OpenSave => {
                self.save_statuses = list_saves(&self.base_dir);
                self.menu_sys.push_screen(
                    MenuScreen::SaveSlots,
                    &self.settings,
                    &self.save_statuses,
                );
            }
            MenuAction::OpenLoad => {
                self.save_statuses = list_saves(&self.base_dir);
                self.menu_sys.push_screen(
                    MenuScreen::LoadSlots,
                    &self.settings,
                    &self.save_statuses,
                );
            }
            MenuAction::OpenSettings => {
                self.menu_sys.push_screen(
                    MenuScreen::Settings,
                    &self.settings,
                    &self.save_statuses,
                );
            }
            MenuAction::OpenControls => {
                self.menu_sys.push_screen(
                    MenuScreen::Controls,
                    &self.settings,
                    &self.save_statuses,
                );
            }
            MenuAction::SaveToSlot(slot) => {
                let _ = self.menu_sys.pop_screen(&self.settings, &self.save_statuses);
                self.save_to_slot(slot, "Game saved");
            }
            MenuAction::LoadSlot(slot) => {
                self.load_from_slot(slot);
            }
            MenuAction::DeleteSlot(slot) => {
                let file_name = SLOT_FILES[slot % SLOT_FILES.len()];
                let _ = delete_save(&self.base_dir, file_name);
                self.save_statuses = list_saves(&self.base_dir);
                let _ = self.menu_sys.pop_screen(&self.settings, &self.save_statuses);
                self.world
                    .squad
                    .set_status_message_colored("Save slot deleted", [1.0, 0.80, 0.35]);
            }
            MenuAction::AdjustSetting(id, delta) => {
                apply_setting_adjustment(&mut self.settings, id, delta);
                self.sync_settings_to_systems();
                self.menu_sys
                    .rebuild_items(&self.settings, &self.save_statuses);
            }
            MenuAction::ResetSettings => {
                self.settings = Settings::default();
                self.sync_settings_to_systems();
                self.menu_sys
                    .rebuild_items(&self.settings, &self.save_statuses);
            }
            MenuAction::BackToMainMenu => {
                if self.world.game_flow == GameFlow::PauseMenu && !self.world.combat.is_dead() {
                    self.save_to_slot(3, "Autosaved");
                }
                self.world.new_game();
                self.world.game_flow = self.world.game_flow.on_back_to_main();
                self.camera.in_main_menu_orbit = true;
                self.save_statuses = list_saves(&self.base_dir);
                self.menu_sys
                    .open_root(MenuScreen::Main, &self.settings, &self.save_statuses);
                self.fade_timer = TRANSITION_FADE_DURATION;
            }
            MenuAction::Quit => {
                if matches!(self.world.game_flow, GameFlow::Playing | GameFlow::PauseMenu)
                    && !self.world.combat.is_dead()
                {
                    self.save_to_slot(3, "Autosaved");
                }
                let _ = self.settings.save_to_dir(&self.base_dir);
                event_loop.exit();
            }
            MenuAction::Back => {
                if !self.menu_sys.pop_screen(&self.settings, &self.save_statuses)
                    && self.world.game_flow == GameFlow::PauseMenu
                {
                    self.world.game_flow = GameFlow::Playing;
                }
            }
        }
    }

    fn step_frame(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() || self.renderer.is_none() {
            return;
        }

        // Global benchmark/settings hotkeys F1..F8
        if self.input.take_just_pressed(KeyCode::F1) {
            self.settings.present_mode = self.settings.present_mode.toggle();
            self.sync_settings_to_systems();
            self.menu_sys
                .rebuild_items(&self.settings, &self.save_statuses);
        }
        if self.input.take_just_pressed(KeyCode::F2) {
            self.settings.cycle_render_scale_f2();
            self.sync_settings_to_systems();
            self.menu_sys
                .rebuild_items(&self.settings, &self.save_statuses);
        }
        if self.input.take_just_pressed(KeyCode::F3) {
            let enabled = self.stats.toggle_hud_panel();
            println!(
                "HUD debug panel toggled (F3): {}",
                if enabled { "ON" } else { "OFF" }
            );
        }
        if self.input.take_just_pressed(KeyCode::F4) {
            self.settings.shadow_quality = self.settings.shadow_quality.cycle();
            self.sync_settings_to_systems();
            self.menu_sys
                .rebuild_items(&self.settings, &self.save_statuses);
        }
        if self.input.take_just_pressed(KeyCode::F5) {
            self.world.cycle_model_yaw_offset();
        }
        if self.input.take_just_pressed(KeyCode::F6) {
            self.world.animator.cycle_manual_clip();
        }
        if self.input.take_just_pressed(KeyCode::F7) {
            self.world.toggle_debug_draw();
        }
        if self.input.take_just_pressed(KeyCode::F8) {
            self.world.toggle_ai_debug();
        }

        let prev_flow = self.world.game_flow;

        if matches!(prev_flow, GameFlow::Playing | GameFlow::GameOver)
            && self.input.take_just_pressed(KeyCode::KeyR)
        {
            self.world.reset();
            self.camera.in_main_menu_orbit = false;
            self.fade_timer = TRANSITION_FADE_DURATION;
        }

        if matches!(prev_flow, GameFlow::Playing | GameFlow::SkillMenu)
            && self.input.take_just_pressed(KeyCode::KeyK)
        {
            self.world.menu.toggle(&mut self.world.game_flow);
            if self.world.menu.open && self.mouse_captured {
                self.set_mouse_capture(false);
            }
        }

        if matches!(prev_flow, GameFlow::Playing | GameFlow::SkillMenu)
            && !self.world.combat.is_dead()
            && self.input.take_just_pressed(KeyCode::KeyT)
        {
            self.world.try_fuse_skills();
        }

        match self.world.game_flow {
            GameFlow::MainMenu | GameFlow::PauseMenu | GameFlow::GameOver => {
                if self.mouse_captured {
                    self.set_mouse_capture(false);
                }
                if self.input.take_just_pressed(KeyCode::ArrowUp) {
                    self.menu_sys.move_up();
                }
                if self.input.take_just_pressed(KeyCode::ArrowDown) {
                    self.menu_sys.move_down();
                }
                if self.input.take_just_pressed(KeyCode::ArrowLeft) {
                    let act = self.menu_sys.adjust_selected(-1);
                    self.execute_menu_action(act, event_loop);
                }
                if self.input.take_just_pressed(KeyCode::ArrowRight) {
                    let act = self.menu_sys.adjust_selected(1);
                    self.execute_menu_action(act, event_loop);
                }
                if self.input.take_just_pressed(KeyCode::Delete) {
                    self.menu_sys
                        .request_delete_selected(&self.settings, &self.save_statuses);
                }
                if self.input.take_just_pressed(KeyCode::Enter) {
                    let act = self
                        .menu_sys
                        .activate_selected(&self.settings, &self.save_statuses);
                    self.execute_menu_action(act, event_loop);
                }
                let _ = self.input.take_mouse_delta();
                let _ = self.input.take_attack_requested();
            }
            GameFlow::SkillMenu => {
                if self.input.take_just_pressed(KeyCode::ArrowUp) {
                    self.world.menu.move_up();
                }
                if self.input.take_just_pressed(KeyCode::ArrowDown) {
                    self.world.menu.move_down();
                }
                if self.input.take_just_pressed(KeyCode::ArrowRight)
                    || self.input.take_just_pressed(KeyCode::Tab)
                {
                    self.world.menu.next_section();
                }
                if self.input.take_just_pressed(KeyCode::ArrowLeft) {
                    self.world.menu.prev_section();
                }
                if self.input.take_just_pressed(KeyCode::Digit1) {
                    self.world
                        .menu
                        .assign_selected_to_slot(&mut self.world.skill_inv, 0);
                }
                if self.input.take_just_pressed(KeyCode::Digit2) {
                    self.world
                        .menu
                        .assign_selected_to_slot(&mut self.world.skill_inv, 1);
                }
                if self.input.take_just_pressed(KeyCode::Digit3) {
                    self.world
                        .menu
                        .assign_selected_to_slot(&mut self.world.skill_inv, 2);
                }
                if self.input.take_just_pressed(KeyCode::Digit4) {
                    self.world
                        .menu
                        .assign_selected_to_slot(&mut self.world.skill_inv, 3);
                }
                if self.input.take_just_pressed(KeyCode::Backspace) {
                    self.world.menu.clear_slot(&mut self.world.skill_inv);
                }
                if self.input.take_just_pressed(KeyCode::Enter) {
                    self.world.try_fuse_selected_menu_recipe();
                }
                let _ = self.input.take_mouse_delta();
                let _ = self.input.take_attack_requested();
            }
            GameFlow::Playing => {
                if self.input.take_just_pressed(KeyCode::F9) {
                    self.save_to_slot(4, "Game saved");
                }
                if self.input.take_just_pressed(KeyCode::F10) {
                    self.load_from_slot(4);
                }
                if self.input.take_just_pressed(KeyCode::KeyG) && !self.world.combat.is_dead() {
                    self.world.summon_ally();
                }
                if self.input.take_just_pressed(KeyCode::Digit1) {
                    self.world.set_squad_mode(SquadMode::Follow);
                }
                if self.input.take_just_pressed(KeyCode::Digit2) {
                    self.world.set_squad_mode(SquadMode::Hold);
                }
                if self.input.take_just_pressed(KeyCode::Digit3) {
                    self.world.set_squad_mode(SquadMode::Aggressive);
                }
                let cam_fwd = self.camera.forward_xz();
                if self.input.take_just_pressed(KeyCode::KeyQ) {
                    self.world.cast_skill_slot(0, cam_fwd, self.mouse_captured);
                }
                if self.input.take_just_pressed(KeyCode::KeyF) {
                    self.world.cast_skill_slot(1, cam_fwd, self.mouse_captured);
                }
                if self.input.take_just_pressed(KeyCode::KeyX) {
                    self.world.cast_skill_slot(2, cam_fwd, self.mouse_captured);
                }
                if self.input.take_just_pressed(KeyCode::KeyV) {
                    self.world.cast_skill_slot(3, cam_fwd, self.mouse_captured);
                }
            }
        }

        let now = Instant::now();
        let raw_dt = match self.last_frame_instant {
            Some(prev) => now.duration_since(prev).as_secs_f32(),
            None => FIXED_DT,
        };
        self.last_frame_instant = Some(now);
        self.fade_timer = (self.fade_timer - raw_dt).max(0.0);

        let cpu_work_start = Instant::now();

        if self.world.game_flow == GameFlow::MainMenu {
            self.camera.update_main_menu_orbit(raw_dt.min(0.10));
            if self.world.squad.status_timer > 0.0 {
                self.world.squad.status_timer =
                    (self.world.squad.status_timer - raw_dt.min(0.10)).max(0.0);
            }
        } else if !self.world.game_flow.blocks_world_update() {
            let frame_dt = raw_dt.min(MAX_FRAME_DELTA);
            self.accumulator += frame_dt;

            let mut steps = 0;
            while self.accumulator >= FIXED_DT {
                if steps >= MAX_CATCHUP_STEPS {
                    self.accumulator = 0.0;
                    break;
                }
                let mouse_delta = if self.mouse_captured {
                    self.input.take_mouse_delta()
                } else {
                    let _ = self.input.take_mouse_delta();
                    (0.0, 0.0)
                };

                if let Some(ai_stats) =
                    self.world
                        .step_if_unpaused(&mut self.input, &self.camera, FIXED_DT)
                {
                    self.stats
                        .record_astar(ai_stats.astar_calls, ai_stats.astar_micros);
                }

                self.camera.update(
                    &self.input,
                    mouse_delta,
                    self.world.player.pos,
                    &self.world.collision,
                    FIXED_DT,
                );
                self.accumulator -= FIXED_DT;
                steps += 1;
            }

            self.world.update_animation(raw_dt.min(0.10));

            if self.world.autosave_requested {
                self.world.autosave_requested = false;
                if !self.world.combat.is_dead() {
                    self.save_to_slot(3, "Autosaved");
                }
            }

            if self.world.game_flow == GameFlow::GameOver && prev_flow != GameFlow::GameOver {
                self.set_mouse_capture(false);
                self.save_statuses = list_saves(&self.base_dir);
                self.menu_sys.open_root(
                    MenuScreen::GameOver,
                    &self.settings,
                    &self.save_statuses,
                );
            }
        } else if self.world.squad.status_timer > 0.0 {
            self.world.squad.status_timer =
                (self.world.squad.status_timer - raw_dt.min(0.10)).max(0.0);
        }

        let alpha = (self.accumulator / FIXED_DT).clamp(0.0, 1.0);
        let fade_alpha = (self.fade_timer / TRANSITION_FADE_DURATION).clamp(0.0, 1.0);

        let (Some(window), Some(renderer)) = (self.window.as_ref(), self.renderer.as_mut()) else {
            return;
        };

        let outcome = {
            let skills_snap = self.world.skill_inv.hud_snapshot(
                self.world.projectiles.active_projectile_count(),
                self.world.projectiles.active_effect_count() + self.world.effects.active_count(),
                self.world.pickups.active_orb_count(),
            );
            let mut hud_telemetry = HudTelemetry {
                show_panel: true,
                mouse_captured: self.mouse_captured,
                fps: 60.0,
                cpu_ms: 0.0,
                gpu_total_and_shadow_ms: None,
                player_pos: self.world.player.interpolated_pos(alpha),
                player_speed: self.world.player.horizontal_speed(),
                player_grounded: self.world.player.grounded,
                player_health: self.world.combat.health,
                player_max_health: PLAYER_MAX_HEALTH,
                player_dead: self.world.combat.is_dead(),
                dodge_ready_ratio: self.world.combat.dodge_ready_ratio(),
                wave_number: self.world.waves.current_wave,
                enemies_alive: self.world.alive_enemy_count(),
                score: self.world.waves.score,
                enemy_state_counts: self.world.enemy_state_counts(),
                ally_state_counts: self.world.squad.state_counts(),
                squad_mode: self.world.squad.mode,
                game_flow: self.world.game_flow,
                hud_scale: self.settings.hud_scale,
                fade_alpha,
                total_kills: self.world.total_kills,
                ally_casts: self.world.squad.total_ally_casts,
                status_message: &self.world.squad.status_message,
                status_color: self.world.squad.status_color,
                status_timer: self.world.squad.status_timer,
                astar_per_sec: 0,
                astar_avg_us: 0.0,
                draw_calls: 0,
                triangles: 0,
                skinned_triangles: 0,
                visible_instances: 0,
                total_instances: 0,
                shadow_instances: 0,
                shadow_quality: self.settings.shadow_quality,
                present_mode: wgpu::PresentMode::Fifo,
                render_scale: self.settings.render_scale,
                model_yaw_offset_rad: self.world.model_yaw_offset(),
                anim_mode: self.world.animator.mode(),
                anim_manual_override: self.world.animator.is_manual_override(),
                anim_clip_name: self.world.animator.current_clip_name(),
                anim_clip_time: self.world.animator.current_clip_time(),
                joint_count: self.world.animator.joint_count(),
                view_proj: Mat4::IDENTITY,
                camera_pos: glam::Vec3::ZERO,
                camera_yaw: self.camera.yaw,
                skills: skills_snap,
                skill_inv: &self.world.skill_inv,
                menu: &self.world.menu,
                menu_sys: &self.menu_sys,
                orbs: &self.world.pickups.orbs,
                allies: &self.world.squad.allies,
                enemies: &self.world.enemies,
                floating_numbers: self.world.floating_numbers.entries(),
            };
            self.stats.fill_hud_metrics(&mut hud_telemetry);
            renderer.render(&self.world, &self.camera, alpha, hud_telemetry)
        };
        let cpu_frame_ms = cpu_work_start.elapsed().as_secs_f64() * 1000.0;

        match outcome {
            RenderOutcome::Rendered(metrics) => self.stats.record_frame(cpu_frame_ms, metrics),
            RenderOutcome::Skipped => {}
            RenderOutcome::OutOfMemory => {
                event_loop.exit();
                return;
            }
        }

        if let Some(title) = self.stats.update_and_format_title(
            raw_dt,
            WINDOW_TITLE,
            self.settings.show_title_stats,
        ) {
            window.set_title(title);
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let attrs = WindowAttributes::default()
                .with_title(WINDOW_TITLE)
                .with_inner_size(LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT));

            let window = Arc::new(
                event_loop
                    .create_window(attrs)
                    .expect("Failed to create application window"),
            );
            let size = window.inner_size();
            self.camera.set_aspect(size.width, size.height);
            let mut renderer = Renderer::new(Arc::clone(&window), &self.pending_models);
            renderer.apply_settings(&self.settings);
            self.pending_models.clear();
            self.stats
                .set_gpu_supported(renderer.supports_timestamps());

            self.window = Some(window);
            self.renderer = Some(renderer);
            self.last_frame_instant = Some(Instant::now());
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                let _ = self.settings.save_to_dir(&self.base_dir);
                event_loop.exit();
            }
            WindowEvent::Focused(false) => {
                self.input.clear();
                if self.mouse_captured {
                    self.set_mouse_capture(false);
                }
                let next_flow = self.world.game_flow.on_focus_lost();
                if next_flow == GameFlow::PauseMenu && self.world.game_flow != GameFlow::PauseMenu {
                    self.world.menu.open = false;
                    self.world.game_flow = GameFlow::PauseMenu;
                    self.save_statuses = list_saves(&self.base_dir);
                    self.menu_sys.open_root(
                        MenuScreen::Pause,
                        &self.settings,
                        &self.save_statuses,
                    );
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (mx, my) = (position.x as f32, position.y as f32);
                self.input.set_cursor_pos(mx, my);
                if matches!(
                    self.world.game_flow,
                    GameFlow::MainMenu | GameFlow::PauseMenu | GameFlow::GameOver
                ) {
                    if let Some(win) = self.window.as_ref() {
                        let sz = win.inner_size();
                        let lay = layout(
                            sz.width,
                            sz.height,
                            self.settings.hud_scale,
                            self.menu_sys.item_count,
                        );
                        self.menu_sys.on_mouse_move(mx, my, &lay);
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if matches!(
                    self.world.game_flow,
                    GameFlow::MainMenu | GameFlow::PauseMenu | GameFlow::GameOver
                ) {
                    let (mx, my) = self.input.cursor_pos;
                    if let Some(win) = self.window.as_ref() {
                        let sz = win.inner_size();
                        let lay = layout(
                            sz.width,
                            sz.height,
                            self.settings.hud_scale,
                            self.menu_sys.item_count,
                        );
                        let act = self.menu_sys.on_mouse_click(
                            mx,
                            my,
                            &lay,
                            &self.settings,
                            &self.save_statuses,
                        );
                        self.execute_menu_action(act, event_loop);
                    }
                } else if self.world.game_flow == GameFlow::Playing {
                    if !self.mouse_captured {
                        self.set_mouse_capture(true);
                    } else {
                        self.input.request_attack();
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed
                    && !event.repeat
                    && event.physical_key == PhysicalKey::Code(KeyCode::Escape)
                {
                    match self.world.game_flow {
                        GameFlow::Playing => {
                            if self.mouse_captured {
                                self.set_mouse_capture(false);
                            }
                            self.world.game_flow = self.world.game_flow.on_escape();
                            self.save_statuses = list_saves(&self.base_dir);
                            self.menu_sys.open_root(
                                MenuScreen::Pause,
                                &self.settings,
                                &self.save_statuses,
                            );
                        }
                        GameFlow::SkillMenu => {
                            self.world.menu.close(&mut self.world.game_flow);
                        }
                        GameFlow::PauseMenu => {
                            if !self
                                .menu_sys
                                .pop_screen(&self.settings, &self.save_statuses)
                            {
                                self.world.game_flow = self.world.game_flow.on_escape();
                            }
                        }
                        GameFlow::MainMenu | GameFlow::GameOver => {
                            let _ = self
                                .menu_sys
                                .pop_screen(&self.settings, &self.save_statuses);
                        }
                    }
                    return;
                }
                self.input.handle_key_event(&event);
            }
            WindowEvent::Resized(new_size) => {
                self.camera.set_aspect(new_size.width, new_size.height);
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(new_size);
                }
            }
            WindowEvent::RedrawRequested => self.step_frame(event_loop),
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
            if self.mouse_captured && self.world.game_flow.mouse_should_be_captured() {
                self.input.accumulate_mouse_delta(dx as f32, dy as f32);
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}
