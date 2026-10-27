use crate::camera::Camera;
use crate::combat::PLAYER_MAX_HEALTH;
use crate::input::Input;
use crate::model::{load_scene_models, CpuModel};
use crate::renderer::{RenderOutcome, Renderer};
use crate::skill_menu::EscapeOutcome;
use crate::squad::SquadMode;
use crate::stats::StatsTracker;
use crate::world::World;
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

pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    pending_models: Vec<CpuModel>,
    world: World,
    camera: Camera,
    input: Input,
    stats: StatsTracker,
    mouse_captured: bool,
    last_frame_instant: Option<Instant>,
    accumulator: f32,
}

impl App {
    pub fn new() -> Self {
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
        let camera = Camera::new(world.player.pos, aspect);

        Self {
            window: None,
            renderer: None,
            pending_models: scene_models.models,
            world,
            camera,
            input: Input::new(),
            stats: StatsTracker::new(false),
            mouse_captured: false,
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

    fn step_frame(&mut self, event_loop: &ActiveEventLoop) {
        let (Some(window), Some(renderer)) = (self.window.as_ref(), self.renderer.as_mut()) else {
            return;
        };

        if self.input.take_just_pressed(KeyCode::F1) { renderer.toggle_present_mode(); }
        if self.input.take_just_pressed(KeyCode::F2) { renderer.toggle_render_scale(); }
        if self.input.take_just_pressed(KeyCode::F3) {
            let enabled = self.stats.toggle_hud_panel();
            println!("HUD debug panel toggled (F3): {}", if enabled { "ON" } else { "OFF" });
        }
        if self.input.take_just_pressed(KeyCode::F4) { renderer.cycle_shadow_quality(); }
        if self.input.take_just_pressed(KeyCode::F5) { self.world.cycle_model_yaw_offset(); }
        if self.input.take_just_pressed(KeyCode::F6) { self.world.animator.cycle_manual_clip(); }
        if self.input.take_just_pressed(KeyCode::F7) { self.world.toggle_debug_draw(); }
        if self.input.take_just_pressed(KeyCode::F8) { self.world.toggle_ai_debug(); }
        if self.input.take_just_pressed(KeyCode::KeyR) { self.world.reset(); }
        if self.input.take_just_pressed(KeyCode::KeyK) {
            self.world.menu.toggle(&mut self.world.game_state);
        }
        if self.input.take_just_pressed(KeyCode::KeyT) && !self.world.combat.is_dead() {
            self.world.try_fuse_skills();
        }

        if self.world.menu.open {
            if self.input.take_just_pressed(KeyCode::ArrowUp) { self.world.menu.move_up(); }
            if self.input.take_just_pressed(KeyCode::ArrowDown) { self.world.menu.move_down(); }
            if self.input.take_just_pressed(KeyCode::ArrowRight) || self.input.take_just_pressed(KeyCode::Tab) {
                self.world.menu.next_section();
            }
            if self.input.take_just_pressed(KeyCode::ArrowLeft) { self.world.menu.prev_section(); }
            if self.input.take_just_pressed(KeyCode::Digit1) {
                self.world.menu.assign_selected_to_slot(&mut self.world.skill_inv, 0);
            }
            if self.input.take_just_pressed(KeyCode::Digit2) {
                self.world.menu.assign_selected_to_slot(&mut self.world.skill_inv, 1);
            }
            if self.input.take_just_pressed(KeyCode::Digit3) {
                self.world.menu.assign_selected_to_slot(&mut self.world.skill_inv, 2);
            }
            if self.input.take_just_pressed(KeyCode::Digit4) {
                self.world.menu.assign_selected_to_slot(&mut self.world.skill_inv, 3);
            }
            if self.input.take_just_pressed(KeyCode::Backspace) {
                self.world.menu.clear_slot(&mut self.world.skill_inv);
            }
            if self.input.take_just_pressed(KeyCode::Enter) {
                self.world.try_fuse_selected_menu_recipe();
            }
            let _ = self.input.take_mouse_delta();
            let _ = self.input.take_attack_requested();
        } else {
            if self.input.take_just_pressed(KeyCode::KeyG) && !self.world.combat.is_dead() {
                self.world.summon_ally();
            }
            if self.input.take_just_pressed(KeyCode::Digit1) { self.world.set_squad_mode(SquadMode::Follow); }
            if self.input.take_just_pressed(KeyCode::Digit2) { self.world.set_squad_mode(SquadMode::Hold); }
            if self.input.take_just_pressed(KeyCode::Digit3) { self.world.set_squad_mode(SquadMode::Aggressive); }
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

        let now = Instant::now();
        let raw_dt = match self.last_frame_instant {
            Some(prev) => now.duration_since(prev).as_secs_f32(),
            None => FIXED_DT,
        };
        self.last_frame_instant = Some(now);

        let cpu_work_start = Instant::now();

        if !self.world.game_state.is_paused() {
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

                if let Some(ai_stats) = self.world.step_if_unpaused(&mut self.input, &self.camera, FIXED_DT) {
                    self.stats.record_astar(ai_stats.astar_calls, ai_stats.astar_micros);
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
        }

        let alpha = (self.accumulator / FIXED_DT).clamp(0.0, 1.0);
        let outcome = {
            let hud_telemetry = self.stats.hud_telemetry(
                self.world.player.interpolated_pos(alpha),
                self.world.player.horizontal_speed(),
                self.world.player.grounded,
                self.world.combat.health,
                PLAYER_MAX_HEALTH,
                self.world.combat.is_dead(),
                self.world.combat.dodge_ready_ratio(),
                self.world.waves.current_wave,
                self.world.alive_enemy_count(),
                self.world.waves.score,
                self.world.enemy_state_counts(),
                self.world.squad.state_counts(),
                self.world.squad.mode,
                self.world.game_state,
                self.world.squad.total_ally_casts,
                &self.world.squad.status_message,
                self.world.squad.status_color,
                self.world.squad.status_timer,
                self.mouse_captured,
                self.world.animator.mode(),
                self.world.animator.is_manual_override(),
                self.world.animator.current_clip_name(),
                self.world.animator.current_clip_time(),
                self.world.animator.joint_count(),
                self.camera.yaw,
                self.world.skill_inv.hud_snapshot(
                    self.world.projectiles.active_projectile_count(),
                    self.world.projectiles.active_effect_count() + self.world.effects.active_count(),
                    self.world.pickups.active_orb_count(),
                ),
                &self.world.skill_inv,
                &self.world.menu,
                &self.world.pickups.orbs,
                &self.world.squad.allies,
                &self.world.enemies,
                self.world.floating_numbers.entries(),
            );
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

        if let Some(title) = self.stats.update_and_format_title(raw_dt, WINDOW_TITLE) {
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

            let window = Arc::new(event_loop.create_window(attrs).expect("Failed to create application window"));
            let size = window.inner_size();
            self.camera.set_aspect(size.width, size.height);
            let renderer = Renderer::new(Arc::clone(&window), &self.pending_models);
            self.pending_models.clear();
            self.stats.set_gpu_supported(renderer.supports_timestamps());

            self.window = Some(window);
            self.renderer = Some(renderer);
            self.last_frame_instant = Some(Instant::now());
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Focused(false) => {
                self.input.clear();
                if self.mouse_captured {
                    self.set_mouse_capture(false);
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                if !self.mouse_captured {
                    self.set_mouse_capture(true);
                } else if !self.world.game_state.is_paused() {
                    self.input.request_attack();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed
                    && !event.repeat
                    && event.physical_key == PhysicalKey::Code(KeyCode::Escape)
                {
                    match self.world.menu.handle_escape(&mut self.world.game_state, self.mouse_captured) {
                        EscapeOutcome::ClosedMenu => {}
                        EscapeOutcome::ReleaseMouse => self.set_mouse_capture(false),
                        EscapeOutcome::ExitApp => event_loop.exit(),
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

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device_id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
            if self.mouse_captured && !self.world.game_state.is_paused() {
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
