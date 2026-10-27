use crate::ally::{Ally, AllyState, ALLY_AGGRO_RANGE, SLOT_STOP_RADIUS};
use crate::collision::Aabb;
use crate::enemy::{Enemy, EnemyState, ATTACK_RANGE, SIGHT_HALF_ANGLE_RAD, SIGHT_RANGE};
use crate::nav::{NavGrid, NAV_CELL_SIZE, NAV_GRID_DIM};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

pub const MAX_LINE_VERTICES: usize = 32768;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct LineVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

pub struct DebugLinesRenderer {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    scratch: Vec<LineVertex>,
    vertex_count: u32,
}

impl DebugLinesRenderer {
    pub fn new(
        device: &wgpu::Device,
        scene_bind_group_layout: &wgpu::BindGroupLayout,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Debug Lines WGSL Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("lines.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Debug Lines Pipeline Layout"),
            bind_group_layouts: &[scene_bind_group_layout],
            push_constant_ranges: &[],
        });
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 12, shader_location: 1, format: wgpu::VertexFormat::Float32x4 },
            ],
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Debug Lines Pipeline"),
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
                    format: color_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Debug Lines Pre-Sized Vertex Buffer"),
            size: (MAX_LINE_VERTICES * std::mem::size_of::<LineVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            vertex_buffer,
            scratch: Vec::with_capacity(MAX_LINE_VERTICES),
            vertex_count: 0,
        }
    }

    pub fn prepare(
        &mut self,
        queue: &wgpu::Queue,
        f7_enabled: bool,
        f8_ai_enabled: bool,
        aabbs: &[Aabb],
        player_world_mat: Mat4,
        bone_segments: &[(Vec3, Vec3)],
        allies: &[Ally],
        enemies: &[Enemy],
        nav: &NavGrid,
    ) {
        if !f7_enabled && !f8_ai_enabled {
            self.vertex_count = 0;
            return;
        }
        self.scratch.clear();

        if f7_enabled {
            let aabb_color = [0.15, 0.95, 0.40, 1.0];
            for aabb in aabbs {
                if self.scratch.len() + 24 > MAX_LINE_VERTICES {
                    break;
                }
                push_aabb_wireframe(&mut self.scratch, aabb, aabb_color);
            }
            let bone_color = [1.0, 0.85, 0.10, 1.0];
            for &(p0, p1) in bone_segments {
                if self.scratch.len() + 2 > MAX_LINE_VERTICES {
                    break;
                }
                let w0 = player_world_mat.transform_point3(p0).to_array();
                let w1 = player_world_mat.transform_point3(p1).to_array();
                push_line(&mut self.scratch, w0, w1, bone_color);
            }
        }

        if f8_ai_enabled {
            let blocked_color = [0.85, 0.15, 0.15, 1.0];
            let half = NAV_CELL_SIZE * 0.45;
            for cz in 0..NAV_GRID_DIM {
                for cx in 0..NAV_GRID_DIM {
                    if nav.is_blocked(cx, cz) {
                        if self.scratch.len() + 8 > MAX_LINE_VERTICES {
                            break;
                        }
                        let c = NavGrid::cell_to_world(cx, cz);
                        let y = 0.04;
                        let p0 = [c.x - half, y, c.y - half];
                        let p1 = [c.x + half, y, c.y - half];
                        let p2 = [c.x + half, y, c.y + half];
                        let p3 = [c.x - half, y, c.y + half];
                        push_line(&mut self.scratch, p0, p1, blocked_color);
                        push_line(&mut self.scratch, p1, p2, blocked_color);
                        push_line(&mut self.scratch, p2, p3, blocked_color);
                        push_line(&mut self.scratch, p3, p0, blocked_color);
                    }
                }
            }

            for ally in allies {
                let state_color = match ally.state {
                    AllyState::Follow => [0.25, 0.95, 0.45, 1.0],
                    AllyState::Engage => [0.30, 0.75, 1.0, 1.0],
                    AllyState::Attack => [1.0, 0.90, 0.20, 1.0],
                    AllyState::Recover => [0.75, 0.55, 1.0, 1.0],
                    AllyState::Regroup => [1.0, 0.55, 0.15, 1.0],
                    AllyState::Down => [0.55, 0.55, 0.55, 1.0],
                };
                let y = ally.pos.y + 0.18;
                push_circle_xz(&mut self.scratch, ally.slot_pos.x, 0.08, ally.slot_pos.z, SLOT_STOP_RADIUS, 8, [0.35, 1.0, 0.85, 1.0]);
                push_circle_xz(&mut self.scratch, ally.pos.x, y, ally.pos.z, ALLY_AGGRO_RANGE, 16, state_color);

                if let Some(tid) = ally.target_enemy_id {
                    if let Some(enemy) = enemies.iter().find(|e| e.id == tid && e.is_alive()) {
                        push_line(
                            &mut self.scratch,
                            [ally.pos.x, y + 0.6, ally.pos.z],
                            [enemy.pos.x, enemy.pos.y + 0.8, enemy.pos.z],
                            [1.0, 0.95, 0.25, 1.0],
                        );
                    }
                }
                if ally.path_index < ally.path.len() {
                    let mut prev = [ally.pos.x, y + 0.05, ally.pos.z];
                    for &wp in &ally.path[ally.path_index..] {
                        let next = [wp.x, y + 0.05, wp.y];
                        push_line(&mut self.scratch, prev, next, [0.35, 0.95, 1.0, 1.0]);
                        prev = next;
                    }
                }
            }

            for enemy in enemies {
                if !enemy.is_alive() {
                    continue;
                }
                let state_color = match enemy.state {
                    EnemyState::Wander => [0.25, 0.80, 1.0, 1.0],
                    EnemyState::Chase => [1.0, 0.65, 0.10, 1.0],
                    EnemyState::Attack => [1.0, 0.15, 0.15, 1.0],
                    EnemyState::Recover => [0.80, 0.30, 0.95, 1.0],
                    EnemyState::Dead => [0.4, 0.4, 0.4, 1.0],
                };
                let y = enemy.pos.y + 0.15;
                let origin = [enemy.pos.x, y, enemy.pos.z];
                let left_a = enemy.yaw + SIGHT_HALF_ANGLE_RAD;
                let right_a = enemy.yaw - SIGHT_HALF_ANGLE_RAD;
                let left_pt = [enemy.pos.x - left_a.sin() * SIGHT_RANGE, y, enemy.pos.z - left_a.cos() * SIGHT_RANGE];
                let right_pt = [enemy.pos.x - right_a.sin() * SIGHT_RANGE, y, enemy.pos.z - right_a.cos() * SIGHT_RANGE];
                push_line(&mut self.scratch, origin, left_pt, state_color);
                push_line(&mut self.scratch, origin, right_pt, state_color);

                let segs = 8;
                for s in 0..segs {
                    let t0 = (s as f32) / (segs as f32);
                    let t1 = ((s + 1) as f32) / (segs as f32);
                    let a0 = right_a + (left_a - right_a) * t0;
                    let a1 = right_a + (left_a - right_a) * t1;
                    let p0 = [enemy.pos.x - a0.sin() * SIGHT_RANGE, y, enemy.pos.z - a0.cos() * SIGHT_RANGE];
                    let p1 = [enemy.pos.x - a1.sin() * SIGHT_RANGE, y, enemy.pos.z - a1.cos() * SIGHT_RANGE];
                    push_line(&mut self.scratch, p0, p1, state_color);
                }
                push_circle_xz(&mut self.scratch, enemy.pos.x, y, enemy.pos.z, ATTACK_RANGE, 12, [1.0, 0.25, 0.25, 1.0]);

                if enemy.path_index < enemy.path.len() {
                    let mut prev = [enemy.pos.x, y + 0.05, enemy.pos.z];
                    for &wp in &enemy.path[enemy.path_index..] {
                        let next = [wp.x, y + 0.05, wp.y];
                        push_line(&mut self.scratch, prev, next, [0.20, 1.0, 0.55, 1.0]);
                        prev = next;
                    }
                }
            }
        }

        let count = self.scratch.len().min(MAX_LINE_VERTICES);
        self.vertex_count = count as u32;
        if count > 0 {
            queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&self.scratch[..count]));
        }
    }

    pub fn draw<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        scene_bind_group: &'a wgpu::BindGroup,
    ) -> u32 {
        if self.vertex_count == 0 {
            return 0;
        }
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, scene_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.draw(0..self.vertex_count, 0..1);
        1
    }
}

#[inline]
fn push_line(out: &mut Vec<LineVertex>, a: [f32; 3], b: [f32; 3], color: [f32; 4]) {
    if out.len() + 2 <= MAX_LINE_VERTICES {
        out.push(LineVertex { position: a, color });
        out.push(LineVertex { position: b, color });
    }
}

fn push_circle_xz(out: &mut Vec<LineVertex>, cx: f32, y: f32, cz: f32, radius: f32, segs: usize, color: [f32; 4]) {
    for s in 0..segs {
        let a0 = (s as f32) * std::f32::consts::TAU / (segs as f32);
        let a1 = ((s + 1) as f32) * std::f32::consts::TAU / (segs as f32);
        let p0 = [cx + a0.cos() * radius, y, cz + a0.sin() * radius];
        let p1 = [cx + a1.cos() * radius, y, cz + a1.sin() * radius];
        push_line(out, p0, p1, color);
    }
}

fn push_aabb_wireframe(out: &mut Vec<LineVertex>, aabb: &Aabb, color: [f32; 4]) {
    let (min, max) = (aabb.min, aabb.max);
    let c = [
        [min.x, min.y, min.z], [max.x, min.y, min.z], [max.x, min.y, max.z], [min.x, min.y, max.z],
        [min.x, max.y, min.z], [max.x, max.y, min.z], [max.x, max.y, max.z], [min.x, max.y, max.z],
    ];
    for (a, b) in [(0,1),(1,2),(2,3),(3,0),(4,5),(5,6),(6,7),(7,4),(0,4),(1,5),(2,6),(3,7)] {
        push_line(out, c[a], c[b], color);
    }
}
