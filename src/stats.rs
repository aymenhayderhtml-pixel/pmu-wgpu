use crate::ally::Ally;
use crate::combat::FloatingNumber;
use crate::enemy::Enemy;
use crate::player_anim::AnimMode;
use crate::game_flow::GameFlow;
use crate::menu::MenuSystem;
use crate::offline::OfflineReport;
use crate::pickups::SkillOrb;
use crate::shadow::ShadowQuality;
use crate::skill_inv::{SkillHudInfo, SkillInventory};
use crate::skill_menu::MenuState;
use crate::squad::SquadMode;
use glam::{Mat4, Vec3};
use std::fmt::Write;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

const SLOT_IDLE: u8 = 0;
const SLOT_PENDING: u8 = 1;
const SLOT_READY_OK: u8 = 2;
const SLOT_READY_ERR: u8 = 3;

#[derive(Copy, Clone, Debug)]
pub struct GpuPassTimings {
    pub shadow_ms: f64,
    pub main_ms: f64,
}

#[derive(Copy, Clone, Debug)]
pub struct FrameMetrics {
    pub draw_calls: u32,
    pub triangles: u32,
    pub skinned_triangles: u32,
    pub visible_instances: u32,
    pub total_instances: u32,
    pub shadow_instances: u32,
    pub present_mode: wgpu::PresentMode,
    pub render_scale: f32,
    pub shadow_quality: ShadowQuality,
    pub model_yaw_offset_rad: f32,
    pub gpu_timings: Option<GpuPassTimings>,
}

impl Default for FrameMetrics {
    fn default() -> Self {
        Self {
            draw_calls: 0,
            triangles: 0,
            skinned_triangles: 0,
            visible_instances: 0,
            total_instances: 0,
            shadow_instances: 0,
            present_mode: wgpu::PresentMode::Fifo,
            render_scale: 1.0,
            shadow_quality: ShadowQuality::Res2048,
            model_yaw_offset_rad: std::f32::consts::PI,
            gpu_timings: None,
        }
    }
}

#[derive(Copy, Clone)]
pub struct HudTelemetry<'a> {
    pub show_panel: bool,
    pub mouse_captured: bool,
    pub fps: f32,
    pub cpu_ms: f64,
    pub gpu_total_and_shadow_ms: Option<(f64, f64)>,
    pub player_pos: Vec3,
    pub player_speed: f32,
    pub player_grounded: bool,
    pub player_health: f32,
    pub player_max_health: f32,
    pub player_dead: bool,
    pub dodge_ready_ratio: f32,
    pub wave_number: u32,
    pub enemies_alive: u32,
    pub score: u32,
    pub enemy_state_counts: [u32; 5],
    pub ally_state_counts: [u32; 6],
    pub squad_mode: SquadMode,
    pub game_flow: GameFlow,
    pub hud_scale: u32,
    pub fade_alpha: f32,
    pub total_kills: u32,
    pub ally_casts: u32,
    pub in_camp_phase: bool,
    pub camp_timer: f32,
    pub food_stock: u32,
    pub camp_activity_counts: [u32; 6],
    pub fairy_state: &'static str,
    pub selected_ally_idx: usize,
    pub login_report: Option<&'a OfflineReport>,
    pub status_message: &'a str,
    pub status_color: [f32; 3],
    pub status_timer: f32,
    pub astar_per_sec: u32,
    pub astar_avg_us: f64,
    pub draw_calls: u32,
    pub triangles: u32,
    pub skinned_triangles: u32,
    pub visible_instances: u32,
    pub total_instances: u32,
    pub shadow_instances: u32,
    pub shadow_quality: ShadowQuality,
    pub present_mode: wgpu::PresentMode,
    pub render_scale: f32,
    pub model_yaw_offset_rad: f32,
    pub anim_mode: AnimMode,
    pub anim_manual_override: bool,
    pub anim_clip_name: &'a str,
    pub anim_clip_time: f32,
    pub joint_count: u32,
    pub view_proj: Mat4,
    pub camera_pos: Vec3,
    pub camera_yaw: f32,
    pub skills: SkillHudInfo,
    pub skill_inv: &'a SkillInventory,
    pub menu: &'a MenuState,
    pub menu_sys: &'a MenuSystem,
    pub orbs: &'a [SkillOrb],
    pub allies: &'a [Ally],
    pub enemies: &'a [Enemy],
    pub floating_numbers: &'a [FloatingNumber],
}

pub struct StatsTracker {
    stats_timer: f32,
    rendered_frames: u32,
    cpu_time_sum_ms: f64,
    gpu_main_sum_ms: f64,
    gpu_shadow_sum_ms: f64,
    gpu_samples: u32,
    astar_calls_accum: u32,
    astar_micros_accum: u64,
    gpu_supported: bool,
    show_hud_panel: bool,
    last_fps: f32,
    last_cpu_ms: f64,
    last_gpu_total_and_shadow: Option<(f64, f64)>,
    last_astar_per_sec: u32,
    last_astar_avg_us: f64,
    last_metrics: FrameMetrics,
    title_buf: String,
}

impl StatsTracker {
    pub fn new(gpu_supported: bool) -> Self {
        Self {
            stats_timer: 0.0,
            rendered_frames: 0,
            cpu_time_sum_ms: 0.0,
            gpu_main_sum_ms: 0.0,
            gpu_shadow_sum_ms: 0.0,
            gpu_samples: 0,
            astar_calls_accum: 0,
            astar_micros_accum: 0,
            gpu_supported,
            show_hud_panel: true,
            last_fps: 60.0,
            last_cpu_ms: 0.0,
            last_gpu_total_and_shadow: None,
            last_astar_per_sec: 0,
            last_astar_avg_us: 0.0,
            last_metrics: FrameMetrics::default(),
            title_buf: String::with_capacity(96),
        }
    }

    pub fn set_gpu_supported(&mut self, supported: bool) {
        self.gpu_supported = supported;
    }

    pub fn toggle_hud_panel(&mut self) -> bool {
        self.show_hud_panel = !self.show_hud_panel;
        self.show_hud_panel
    }

    pub fn record_astar(&mut self, calls: u32, micros: u64) {
        self.astar_calls_accum += calls;
        self.astar_micros_accum += micros;
    }

    pub fn record_frame(&mut self, cpu_frame_ms: f64, metrics: FrameMetrics) {
        self.rendered_frames += 1;
        self.cpu_time_sum_ms += cpu_frame_ms;
        if let Some(timings) = metrics.gpu_timings {
            self.gpu_shadow_sum_ms += timings.shadow_ms;
            self.gpu_main_sum_ms += timings.main_ms;
            self.gpu_samples += 1;
        }
        self.last_metrics = metrics;
    }

    pub fn fill_hud_metrics(&self, hud: &mut HudTelemetry<'_>) {
        hud.show_panel = self.show_hud_panel;
        hud.fps = self.last_fps;
        hud.cpu_ms = self.last_cpu_ms;
        hud.gpu_total_and_shadow_ms = self.last_gpu_total_and_shadow;
        hud.astar_per_sec = self.last_astar_per_sec;
        hud.astar_avg_us = self.last_astar_avg_us;
        hud.draw_calls = self.last_metrics.draw_calls;
        hud.triangles = self.last_metrics.triangles;
        hud.skinned_triangles = self.last_metrics.skinned_triangles;
        hud.visible_instances = self.last_metrics.visible_instances;
        hud.total_instances = self.last_metrics.total_instances;
        hud.shadow_instances = self.last_metrics.shadow_instances;
        hud.shadow_quality = self.last_metrics.shadow_quality;
        hud.present_mode = self.last_metrics.present_mode;
        hud.render_scale = self.last_metrics.render_scale;
        hud.model_yaw_offset_rad = self.last_metrics.model_yaw_offset_rad;
    }

    pub fn update_and_format_title<'a>(
        &'a mut self,
        raw_dt: f32,
        base_title: &str,
        show_title_stats: bool,
    ) -> Option<&'a str> {
        self.stats_timer += raw_dt;
        if self.stats_timer < 1.0 {
            return None;
        }

        let fps = self.rendered_frames as f32 / self.stats_timer;
        let avg_cpu_ms = if self.rendered_frames > 0 {
            self.cpu_time_sum_ms / (self.rendered_frames as f64)
        } else {
            0.0
        };

        self.last_fps = fps;
        self.last_cpu_ms = avg_cpu_ms;
        self.last_gpu_total_and_shadow = if self.gpu_supported && self.gpu_samples > 0 {
            let avg_main = self.gpu_main_sum_ms / (self.gpu_samples as f64);
            let avg_shd = self.gpu_shadow_sum_ms / (self.gpu_samples as f64);
            Some((avg_main + avg_shd, avg_shd))
        } else {
            None
        };

        self.last_astar_per_sec = ((self.astar_calls_accum as f32) / self.stats_timer).round() as u32;
        self.last_astar_avg_us = if self.astar_calls_accum > 0 {
            (self.astar_micros_accum as f64) / (self.astar_calls_accum as f64)
        } else {
            0.0
        };

        self.title_buf.clear();
        if show_title_stats {
            let _ = write!(&mut self.title_buf, "{} | FPS: {:.1} | CPU: {:.2} ms", base_title, fps, avg_cpu_ms);
        } else {
            self.title_buf.push_str(base_title);
        }

        self.stats_timer = 0.0;
        self.rendered_frames = 0;
        self.cpu_time_sum_ms = 0.0;
        self.gpu_main_sum_ms = 0.0;
        self.gpu_shadow_sum_ms = 0.0;
        self.gpu_samples = 0;
        self.astar_calls_accum = 0;
        self.astar_micros_accum = 0;

        Some(&self.title_buf)
    }
}

struct ReadbackSlot {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
    has_shadow_timestamps: bool,
}

pub struct GpuTimer {
    query_set: wgpu::QuerySet,
    resolve_buffer: wgpu::Buffer,
    slots: [ReadbackSlot; 2],
    scheduled_slot: Option<usize>,
    timestamp_period_ns: f32,
}

impl GpuTimer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("Pass Timestamp Query Set (4 Queries)"),
            count: 4,
            ty: wgpu::QueryType::Timestamp,
        });
        let resolve_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Timestamp Resolve Buffer (32B)"),
            size: 32,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let make_slot = |label| ReadbackSlot {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: 32,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
            state: Arc::new(AtomicU8::new(SLOT_IDLE)),
            has_shadow_timestamps: true,
        };
        Some(Self {
            query_set,
            resolve_buffer,
            slots: [make_slot("Timestamp Readback Slot 0"), make_slot("Timestamp Readback Slot 1")],
            scheduled_slot: None,
            timestamp_period_ns: queue.get_timestamp_period(),
        })
    }

    pub fn query_set(&self) -> &wgpu::QuerySet {
        &self.query_set
    }

    pub fn poll_and_read(&mut self, device: &wgpu::Device) -> Option<GpuPassTimings> {
        let _ = device.poll(wgpu::PollType::Poll);
        let mut latest = None;
        let period_ms = (self.timestamp_period_ns as f64) / 1_000_000.0;

        for slot in &mut self.slots {
            let st = slot.state.load(Ordering::Acquire);
            if st == SLOT_READY_OK {
                {
                    let view = slot.buffer.slice(..).get_mapped_range();
                    let ts: &[u64] = bytemuck::cast_slice(&view);
                    if slot.has_shadow_timestamps && ts.len() >= 4 {
                        let shd_ticks = ts[1].saturating_sub(ts[0]);
                        let main_ticks = ts[3].saturating_sub(ts[2]);
                        latest = Some(GpuPassTimings {
                            shadow_ms: (shd_ticks as f64) * period_ms,
                            main_ms: (main_ticks as f64) * period_ms,
                        });
                    } else if !slot.has_shadow_timestamps && ts.len() >= 2 {
                        let main_ticks = ts[1].saturating_sub(ts[0]);
                        latest = Some(GpuPassTimings {
                            shadow_ms: 0.0,
                            main_ms: (main_ticks as f64) * period_ms,
                        });
                    }
                }
                slot.buffer.unmap();
                slot.state.store(SLOT_IDLE, Ordering::Release);
            } else if st == SLOT_READY_ERR {
                slot.buffer.unmap();
                slot.state.store(SLOT_IDLE, Ordering::Release);
            }
        }
        latest
    }

    pub fn resolve_after_passes(&mut self, encoder: &mut wgpu::CommandEncoder, shadow_pass_ran: bool) {
        self.scheduled_slot = None;
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if slot.state.load(Ordering::Acquire) == SLOT_IDLE {
                slot.has_shadow_timestamps = shadow_pass_ran;
                if shadow_pass_ran {
                    encoder.resolve_query_set(&self.query_set, 0..4, &self.resolve_buffer, 0);
                    encoder.copy_buffer_to_buffer(&self.resolve_buffer, 0, &slot.buffer, 0, 32);
                } else {
                    encoder.resolve_query_set(&self.query_set, 2..4, &self.resolve_buffer, 0);
                    encoder.copy_buffer_to_buffer(&self.resolve_buffer, 0, &slot.buffer, 0, 16);
                }
                self.scheduled_slot = Some(i);
                break;
            }
        }
    }

    pub fn initiate_map_after_submit(&mut self) {
        if let Some(i) = self.scheduled_slot.take() {
            let slot = &self.slots[i];
            slot.state.store(SLOT_PENDING, Ordering::Release);
            let state_clone = Arc::clone(&slot.state);
            slot.buffer.slice(..).map_async(wgpu::MapMode::Read, move |res| match res {
                Ok(()) => state_clone.store(SLOT_READY_OK, Ordering::Release),
                Err(_) => state_clone.store(SLOT_READY_ERR, Ordering::Release),
            });
        }
    }
}
