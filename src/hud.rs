use crate::combat::{FloatingKind, FLOATING_NUMBER_DURATION};
use crate::enemy::EnemyState;
use crate::font::{
    build_font_atlas_rgba8, glyph_uv, solid_white_uv, ATLAS_HEIGHT, ATLAS_WIDTH, GLYPH_SIZE,
};
use crate::hud_squad::draw_squad_hud_elements;
use crate::stats::HudTelemetry;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2, Vec3, Vec4};
use std::fmt::Write;

pub const HUD_SCALE: f32 = 2.0;
pub const MAX_HUD_QUADS: usize = 4096;
pub const MAX_HUD_VERTICES: usize = MAX_HUD_QUADS * 6;

const CONTROLS_HINT: &str =
    "LClick:Atk Q/F/X/V:Skills T:Fuse C:Dodge G:Summon E:Rev 1/2/3:Squad R:Reset F1..F8:Tools";
const DEATH_MESSAGE: &str = "YOU DIED - press R to restart";

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct HudUniform {
    pub screen_size: [f32; 2],
    pub _pad: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct HudVertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

pub struct HudRenderer {
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    vertex_buffer: wgpu::Buffer,
    vertex_scratch: Vec<HudVertex>,
    text_scratch: String,
    visible_vertex_count: u32,
}

impl HudRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, target_format: wgpu::TextureFormat) -> Self {
        let atlas_rgba = build_font_atlas_rgba8();
        let atlas_size = wgpu::Extent3d {
            width: ATLAS_WIDTH,
            height: ATLAS_HEIGHT,
            depth_or_array_layers: 1,
        };
        let font_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("HUD 8x8 Font Atlas Texture"),
            size: atlas_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &font_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &atlas_rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * ATLAS_WIDTH),
                rows_per_image: Some(ATLAS_HEIGHT),
            },
            atlas_size,
        );

        let font_view = font_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let font_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("HUD Nearest Font Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("HUD Screen Size Uniform Buffer"),
            size: std::mem::size_of::<HudUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("HUD Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("HUD Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&font_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&font_sampler) },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("HUD WGSL Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("hud.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("HUD Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<HudVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
                wgpu::VertexAttribute { offset: 8, shader_location: 1, format: wgpu::VertexFormat::Float32x2 },
                wgpu::VertexAttribute { offset: 16, shader_location: 2, format: wgpu::VertexFormat::Float32x4 },
            ],
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("HUD Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_layout],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("HUD Pre-Sized Vertex Buffer"),
            size: (MAX_HUD_VERTICES * std::mem::size_of::<HudVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            uniform_buffer,
            bind_group,
            vertex_buffer,
            vertex_scratch: Vec::with_capacity(MAX_HUD_VERTICES),
            text_scratch: String::with_capacity(512),
            visible_vertex_count: 0,
        }
    }

    pub fn prepare(
        &mut self,
        queue: &wgpu::Queue,
        screen_width: u32,
        screen_height: u32,
        telemetry: &HudTelemetry<'_>,
    ) {
        let sw = screen_width.max(1) as f32;
        let sh = screen_height.max(1) as f32;

        let uniform = HudUniform { screen_size: [sw, sh], _pad: [0.0, 0.0] };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniform));

        self.vertex_scratch.clear();
        let glyph_px = (GLYPH_SIZE as f32) * HUD_SCALE;
        let line_step = glyph_px + 4.0 * HUD_SCALE;

        // 1. World-space enemy health bars
        for enemy in telemetry.enemies {
            if !enemy.is_alive() || enemy.pos.distance_squared(telemetry.camera_pos) > 30.0 * 30.0 {
                continue;
            }
            let show_bar = enemy.health < enemy.max_health
                || enemy.state == EnemyState::Chase
                || enemy.state == EnemyState::Attack;
            if !show_bar {
                continue;
            }
            let head_world = enemy.pos + Vec3::new(0.0, 2.35, 0.0);
            if let Some(screen_pt) = project_world_to_screen(head_world, telemetry.view_proj, sw, sh) {
                let (bar_w, bar_h) = (54.0, 7.0);
                let (bx, by) = (screen_pt.x - bar_w * 0.5, screen_pt.y - bar_h);
                let ratio = (enemy.health / enemy.max_health.max(1.0)).clamp(0.0, 1.0);
                push_solid_quad(&mut self.vertex_scratch, bx - 1.0, by - 1.0, bar_w + 2.0, bar_h + 2.0, [0.02, 0.02, 0.02, 0.85]);
                push_solid_quad(&mut self.vertex_scratch, bx, by, bar_w * ratio, bar_h, [0.95, 0.22, 0.18, 0.95]);
            }
        }

        // 2. Squad HUD elements (left squad panel, ally world bars/names/revive, radar, status msg)
        draw_squad_hud_elements(&mut self.vertex_scratch, &mut self.text_scratch, sw, sh, telemetry);

        // 3. Floating damage and healing numbers
        for num in telemetry.floating_numbers {
            if !num.active {
                continue;
            }
            if let Some(screen_pt) = project_world_to_screen(num.world_pos, telemetry.view_proj, sw, sh) {
                self.text_scratch.clear();
                let alpha = (num.timer / FLOATING_NUMBER_DURATION).clamp(0.0, 1.0);
                let color = match num.kind {
                    FloatingKind::Heal => {
                        let _ = write!(&mut self.text_scratch, "+{}", num.amount);
                        [0.30, 0.98, 0.45, alpha]
                    }
                    FloatingKind::PlayerSideDamage => {
                        let _ = write!(&mut self.text_scratch, "-{}", num.amount);
                        [1.0, 0.25, 0.22, alpha]
                    }
                    FloatingKind::EnemyDamage => {
                        let _ = write!(&mut self.text_scratch, "-{}", num.amount);
                        [1.0, 0.92, 0.25, alpha]
                    }
                };
                let w = (self.text_scratch.len() as f32) * glyph_px;
                push_text_line(&mut self.vertex_scratch, &self.text_scratch, screen_pt.x - w * 0.5, screen_pt.y, glyph_px, color);
            }
        }

        // 4. Top-Right Wave, Alive Enemies, Score
        {
            let tr_w = 240.0;
            let tr_h = line_step * 3.0 + 14.0;
            let tr_x = (sw - tr_w - 12.0).max(10.0);
            let tr_y = 10.0;
            push_solid_quad(&mut self.vertex_scratch, tr_x, tr_y, tr_w, tr_h, [0.02, 0.03, 0.05, 0.75]);

            let tx = tr_x + 10.0;
            let mut ty = tr_y + 8.0;

            self.text_scratch.clear();
            let _ = write!(&mut self.text_scratch, "WAVE:   {:3}", telemetry.wave_number);
            push_text_line(&mut self.vertex_scratch, &self.text_scratch, tx, ty, glyph_px, [1.0, 0.85, 0.35, 1.0]);
            ty += line_step;

            self.text_scratch.clear();
            let _ = write!(&mut self.text_scratch, "ENEMIES:{:3}", telemetry.enemies_alive);
            push_text_line(&mut self.vertex_scratch, &self.text_scratch, tx, ty, glyph_px, [0.98, 0.55, 0.50, 1.0]);
            ty += line_step;

            self.text_scratch.clear();
            let _ = write!(&mut self.text_scratch, "SCORE: {:4}", telemetry.score);
            push_text_line(&mut self.vertex_scratch, &self.text_scratch, tx, ty, glyph_px, [0.65, 0.98, 0.75, 1.0]);
        }

        // 5. Bottom-Center Mana Bar, Player Health Bar, Dodge Bar, and 4 Skill Slot Boxes
        {
            let (hp_w, hp_h) = (260.0, 16.0);
            let bx = (sw - hp_w) * 0.5 - 110.0;
            let by = (sh - 72.0).max(20.0);

            // Mana bar (blue) above HP bar
            let mp_h = 13.0;
            let mp_y = by - mp_h - 5.0;
            let mp_ratio = (telemetry.skills.mana / telemetry.skills.max_mana.max(1.0)).clamp(0.0, 1.0);
            push_solid_quad(&mut self.vertex_scratch, bx - 2.0, mp_y - 2.0, hp_w + 4.0, mp_h + 4.0, [0.02, 0.03, 0.05, 0.85]);
            push_solid_quad(&mut self.vertex_scratch, bx, mp_y, hp_w * mp_ratio, mp_h, [0.22, 0.56, 0.98, 0.95]);
            self.text_scratch.clear();
            let _ = write!(&mut self.text_scratch, "MP {:3.0}/{:3.0}", telemetry.skills.mana.floor(), telemetry.skills.max_mana);
            let mp_text_w = (self.text_scratch.len() as f32) * 10.0;
            push_text_line(&mut self.vertex_scratch, &self.text_scratch, bx + (hp_w - mp_text_w) * 0.5, mp_y + 2.0, 10.0, [1.0, 1.0, 1.0, 1.0]);

            // 4 Skill Slot boxes to the right of the bars (keys Q, F, X, V)
            let slot_keys = ["Q", "F", "X", "V"];
            let slot_w = 50.0;
            let slot_h = 46.0;
            let slots_start_x = bx + hp_w + 14.0;
            let slots_y = mp_y;
            for (s_idx, key_label) in slot_keys.iter().enumerate() {
                let sx = slots_start_x + (s_idx as f32) * (slot_w + 6.0);
                let slot_info = telemetry.skills.slots[s_idx];
                let border_col = if slot_info.fail_flash > 0.0 {
                    [1.0, 0.18, 0.18, 0.95]
                } else {
                    [0.35, 0.45, 0.58, 0.80]
                };
                push_solid_quad(&mut self.vertex_scratch, sx - 2.0, slots_y - 2.0, slot_w + 4.0, slot_h + 4.0, border_col);
                push_solid_quad(&mut self.vertex_scratch, sx, slots_y, slot_w, slot_h, [0.04, 0.06, 0.10, 0.88]);

                if let Some(sk) = slot_info.skill {
                    let def = sk.def();
                    push_text_line(&mut self.vertex_scratch, def.short_name, sx + 5.0, slots_y + 16.0, 10.0, [def.tint[0], def.tint[1], def.tint[2], 1.0]);
                    self.text_scratch.clear();
                    let _ = write!(&mut self.text_scratch, "L{}", slot_info.level);
                    push_text_line(&mut self.vertex_scratch, &self.text_scratch, sx + slot_w - 20.0, slots_y + 4.0, 9.0, [0.95, 0.90, 0.45, 1.0]);

                    if slot_info.cooldown_ratio > 0.0 {
                        let cd_h = slot_h * slot_info.cooldown_ratio;
                        push_solid_quad(&mut self.vertex_scratch, sx, slots_y, slot_w, cd_h, [0.01, 0.01, 0.02, 0.76]);
                    }
                } else {
                    push_text_line(&mut self.vertex_scratch, "--", sx + 16.0, slots_y + 18.0, 10.0, [0.45, 0.48, 0.52, 0.9]);
                }
                push_text_line(&mut self.vertex_scratch, key_label, sx + 4.0, slots_y + 4.0, 10.0, [0.85, 0.90, 1.0, 1.0]);
            }

            let hp_ratio = (telemetry.player_health / telemetry.player_max_health.max(1.0)).clamp(0.0, 1.0);

            push_solid_quad(&mut self.vertex_scratch, bx - 2.0, by - 2.0, hp_w + 4.0, hp_h + 4.0, [0.02, 0.03, 0.05, 0.85]);
            push_solid_quad(&mut self.vertex_scratch, bx, by, hp_w * hp_ratio, hp_h, [0.20, 0.85, 0.35, 0.95]);

            self.text_scratch.clear();
            let _ = write!(&mut self.text_scratch, "HP {:3.0}/{:3.0}", telemetry.player_health.ceil(), telemetry.player_max_health);
            let hp_text_px = 12.0;
            let hp_text_w = (self.text_scratch.len() as f32) * hp_text_px;
            push_text_line(&mut self.vertex_scratch, &self.text_scratch, bx + (hp_w - hp_text_w) * 0.5, by + 3.0, hp_text_px, [1.0, 1.0, 1.0, 1.0]);

            let (dg_h, dg_y) = (8.0, by + hp_h + 6.0);
            let dg_ratio = telemetry.dodge_ready_ratio.clamp(0.0, 1.0);
            push_solid_quad(&mut self.vertex_scratch, bx - 2.0, dg_y - 1.0, hp_w + 4.0, dg_h + 2.0, [0.02, 0.03, 0.05, 0.85]);
            let dg_color = if dg_ratio >= 0.999 { [0.30, 0.80, 1.0, 0.95] } else { [0.45, 0.55, 0.65, 0.85] };
            push_solid_quad(&mut self.vertex_scratch, bx, dg_y, hp_w * dg_ratio, dg_h, dg_color);
        }

        // 6. F3 Top-left debug panel
        if telemetry.show_panel {
            let (panel_x, panel_y) = (10.0, 10.0);
            let panel_w = 580.0 * (HUD_SCALE * 0.5);
            let panel_h = line_step * 11.0 + 12.0;
            push_solid_quad(&mut self.vertex_scratch, panel_x, panel_y, panel_w, panel_h, [0.02, 0.03, 0.05, 0.72]);

            let text_x = panel_x + 8.0;
            let mut text_y = panel_y + 8.0;
            let text_color = [0.95, 0.98, 1.0, 1.0];

            self.text_scratch.clear();
            let _ = write!(&mut self.text_scratch, "FPS: {:5.1}   CPU: {:5.2} ms", telemetry.fps, telemetry.cpu_ms);
            push_text_line(&mut self.vertex_scratch, &self.text_scratch, text_x, text_y, glyph_px, text_color);
            text_y += line_step;

            self.text_scratch.clear();
            if let Some((gpu_total, gpu_shd)) = telemetry.gpu_total_and_shadow_ms {
                let _ = write!(&mut self.text_scratch, "GPU: {:5.2} ms (Shd: {:4.2} ms)", gpu_total, gpu_shd);
            } else {
                let _ = write!(&mut self.text_scratch, "GPU: n/a");
            }
            push_text_line(&mut self.vertex_scratch, &self.text_scratch, text_x, text_y, glyph_px, text_color);
            text_y += line_step;

            self.text_scratch.clear();
            let _ = write!(&mut self.text_scratch, "Pos: ({:5.1}, {:4.1}, {:5.1})", telemetry.player_pos.x, telemetry.player_pos.y, telemetry.player_pos.z);
            push_text_line(&mut self.vertex_scratch, &self.text_scratch, text_x, text_y, glyph_px, [0.65, 0.95, 0.75, 1.0]);
            text_y += line_step;

            self.text_scratch.clear();
            let _ = write!(&mut self.text_scratch, "Spd: {:4.1} m/s  Grounded: {}", telemetry.player_speed, if telemetry.player_grounded { "YES" } else { "NO" });
            push_text_line(&mut self.vertex_scratch, &self.text_scratch, text_x, text_y, glyph_px, [0.65, 0.95, 0.75, 1.0]);
            text_y += line_step;

            self.text_scratch.clear();
            let _ = write!(
                &mut self.text_scratch,
                "Draws:{} Tris:{}(Skn:{}) Inst:{}/{}(Shd:{})",
                telemetry.draw_calls, telemetry.triangles, telemetry.skinned_triangles,
                telemetry.visible_instances, telemetry.total_instances, telemetry.shadow_instances
            );
            push_text_line(&mut self.vertex_scratch, &self.text_scratch, text_x, text_y, glyph_px, text_color);
            text_y += line_step;

            self.text_scratch.clear();
            let yaw_deg = telemetry.model_yaw_offset_rad.to_degrees().round() as i32;
            let _ = write!(
                &mut self.text_scratch,
                "ShdQ:{} Mode:{:?} Scale:{:.2}x Yaw:{}d",
                telemetry.shadow_quality.label(), telemetry.present_mode, telemetry.render_scale, yaw_deg
            );
            push_text_line(&mut self.vertex_scratch, &self.text_scratch, text_x, text_y, glyph_px, text_color);
            text_y += line_step;

            self.text_scratch.clear();
            let ctrl_tag = if telemetry.anim_manual_override { "man" } else { "auto" };
            let _ = write!(
                &mut self.text_scratch,
                "Anim:{}({}) '{}' t:{:.2}s Jnt:{}",
                telemetry.anim_mode.label(), ctrl_tag, telemetry.anim_clip_name, telemetry.anim_clip_time, telemetry.joint_count
            );
            push_text_line(&mut self.vertex_scratch, &self.text_scratch, text_x, text_y, glyph_px, [0.98, 0.88, 0.55, 1.0]);
            text_y += line_step;

            let sc = telemetry.enemy_state_counts;
            self.text_scratch.clear();
            let _ = write!(&mut self.text_scratch, "Enemy[W:{} C:{} A:{} R:{} D:{}]", sc[0], sc[1], sc[2], sc[3], sc[4]);
            push_text_line(&mut self.vertex_scratch, &self.text_scratch, text_x, text_y, glyph_px, [0.95, 0.75, 0.65, 1.0]);
            text_y += line_step;

            let ac = telemetry.ally_state_counts;
            self.text_scratch.clear();
            let _ = write!(
                &mut self.text_scratch,
                "Squad({})[F:{} E:{} A:{} R:{} G:{} D:{}]",
                telemetry.squad_mode.label(), ac[0], ac[1], ac[2], ac[3], ac[4], ac[5]
            );
            push_text_line(&mut self.vertex_scratch, &self.text_scratch, text_x, text_y, glyph_px, [0.65, 0.98, 0.85, 1.0]);
            text_y += line_step;

            self.text_scratch.clear();
            let _ = write!(&mut self.text_scratch, "A*: {}/s  Avg: {:.1} us", telemetry.astar_per_sec, telemetry.astar_avg_us);
            push_text_line(&mut self.vertex_scratch, &self.text_scratch, text_x, text_y, glyph_px, [0.75, 0.90, 1.0, 1.0]);
            text_y += line_step;

            let c = telemetry.skills.counts;
            self.text_scratch.clear();
            let _ = write!(
                &mut self.text_scratch,
                "MP:{:.0} Prj:{} Fx:{} Orb:{} Inv[{}{}{}{}{}|{}{}{}{}]",
                telemetry.skills.mana,
                telemetry.skills.active_projectiles,
                telemetry.skills.active_effects,
                telemetry.skills.active_orbs,
                c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8]
            );
            push_text_line(&mut self.vertex_scratch, &self.text_scratch, text_x, text_y, glyph_px, [0.65, 0.85, 1.0, 1.0]);
        }

        // 7. Bottom-left controls hint line
        let hint_scale = (HUD_SCALE * 0.65).max(1.0);
        let hint_glyph_px = (GLYPH_SIZE as f32) * hint_scale;
        let hint_w = (CONTROLS_HINT.len() as f32) * hint_glyph_px + 16.0;
        let hint_h = hint_glyph_px + 10.0;
        let (hint_x, hint_y) = (10.0, (sh - hint_h - 8.0).max(10.0));

        push_solid_quad(&mut self.vertex_scratch, hint_x, hint_y, hint_w, hint_h, [0.02, 0.03, 0.05, 0.68]);
        push_text_line(&mut self.vertex_scratch, CONTROLS_HINT, hint_x + 8.0, hint_y + 5.0, hint_glyph_px, [0.88, 0.90, 0.95, 1.0]);

        // 8. Center crosshair dot or YOU DIED banner
        if telemetry.player_dead {
            let banner_px = glyph_px * 1.5;
            let msg_w = (DEATH_MESSAGE.len() as f32) * banner_px;
            let (mx, my) = ((sw - msg_w) * 0.5, (sh - banner_px) * 0.5);
            push_solid_quad(&mut self.vertex_scratch, mx - 16.0, my - 14.0, msg_w + 32.0, banner_px + 28.0, [0.08, 0.01, 0.01, 0.88]);
            push_text_line(&mut self.vertex_scratch, DEATH_MESSAGE, mx, my, banner_px, [1.0, 0.25, 0.22, 1.0]);
        } else if telemetry.mouse_captured {
            let dot_size = 2.0 * HUD_SCALE;
            let (cx, cy) = ((sw - dot_size) * 0.5, (sh - dot_size) * 0.5);
            push_solid_quad(&mut self.vertex_scratch, cx - 1.0, cy - 1.0, dot_size + 2.0, dot_size + 2.0, [0.0, 0.0, 0.0, 0.75]);
            push_solid_quad(&mut self.vertex_scratch, cx, cy, dot_size, dot_size, [1.0, 1.0, 1.0, 0.95]);
        }

        let count = self.vertex_scratch.len().min(MAX_HUD_VERTICES);
        self.visible_vertex_count = count as u32;
        if count > 0 {
            queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&self.vertex_scratch[..count]));
        }
    }

    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, target_view: &wgpu::TextureView) -> (u32, u32) {
        if self.visible_vertex_count == 0 {
            return (0, 0);
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("HUD Overlay Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.draw(0..self.visible_vertex_count, 0..1);
        (1, self.visible_vertex_count / 3)
    }
}

pub fn project_world_to_screen(world_pos: Vec3, view_proj: Mat4, sw: f32, sh: f32) -> Option<Vec2> {
    let clip = view_proj * Vec4::new(world_pos.x, world_pos.y, world_pos.z, 1.0);
    if clip.w <= 0.05 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    if ndc.z < 0.0 || ndc.z > 1.0 || ndc.x.abs() > 1.2 || ndc.y.abs() > 1.2 {
        return None;
    }
    Some(Vec2::new((ndc.x * 0.5 + 0.5) * sw, (0.5 - ndc.y * 0.5) * sh))
}

pub fn push_text_line(out: &mut Vec<HudVertex>, text: &str, start_x: f32, start_y: f32, glyph_px: f32, color: [f32; 4]) {
    let mut cursor_x = start_x;
    for byte in text.bytes() {
        if byte == b' ' {
            cursor_x += glyph_px;
            continue;
        }
        if out.len() + 6 > MAX_HUD_VERTICES {
            break;
        }
        let (u0, v0, u1, v1) = glyph_uv(byte);
        push_quad_uv(out, cursor_x, start_y, glyph_px, glyph_px, u0, v0, u1, v1, color);
        cursor_x += glyph_px;
    }
}

pub fn push_solid_quad(out: &mut Vec<HudVertex>, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
    if out.len() + 6 > MAX_HUD_VERTICES || w <= 0.0 || h <= 0.0 {
        return;
    }
    let (u0, v0, u1, v1) = solid_white_uv();
    push_quad_uv(out, x, y, w, h, u0, v0, u1, v1, color);
}

#[allow(clippy::too_many_arguments)]
pub fn push_quad_uv(out: &mut Vec<HudVertex>, x: f32, y: f32, w: f32, h: f32, u0: f32, v0: f32, u1: f32, v1: f32, color: [f32; 4]) {
    let (x0, y0, x1, y1) = (x, y, x + w, y + h);
    out.extend_from_slice(&[
        HudVertex { pos: [x0, y0], uv: [u0, v0], color },
        HudVertex { pos: [x0, y1], uv: [u0, v1], color },
        HudVertex { pos: [x1, y1], uv: [u1, v1], color },
        HudVertex { pos: [x0, y0], uv: [u0, v0], color },
        HudVertex { pos: [x1, y1], uv: [u1, v1], color },
        HudVertex { pos: [x1, y0], uv: [u1, v0], color },
    ]);
}
