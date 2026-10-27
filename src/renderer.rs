use crate::camera::Camera;
use crate::debug_lines::DebugLinesRenderer;
use crate::frustum::Frustum;
use crate::gpu_model::{
    skinned_model_vertex_buffer_layout, static_model_vertex_buffer_layout, ModelRenderer,
};
use crate::hud::HudRenderer;
use crate::mesh::{cube_mesh, ground_plane_mesh, Vertex};
use crate::model::CpuModel;
use crate::post::PostProcess;
use crate::shadow::{
    compute_light_view_proj, ShadowPass, ShadowQuality, SHADOW_NORMAL_BIAS, SHADOW_ORTHO_RADIUS,
};
use crate::sky::SkyRenderer;
use crate::stats::{FrameMetrics, GpuTimer, HudTelemetry};
use crate::world::{InstanceData, RenderActorTag, World, GROUND_SIZE, TOTAL_CUBE_INSTANCES};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::dpi::PhysicalSize;
use winit::window::Window;

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const FOG_START: f32 = 24.0;
pub const FOG_END: f32 = 72.0;

pub const SKY_HORIZON_COLOR: [f32; 3] = [0.60, 0.73, 0.86];
pub const SKY_ZENITH_COLOR: [f32; 3] = [0.16, 0.34, 0.66];
pub const FOG_COLOR: [f32; 3] = SKY_HORIZON_COLOR;

pub const GROUND_COLOR: [f32; 4] = [0.24, 0.25, 0.27, 1.0];
pub const GROUND_AMBIENT_COLOR: [f32; 3] = [0.22, 0.20, 0.18];
pub const AMBIENT_STRENGTH: f32 = 0.34;
pub const DIFFUSE_STRENGTH: f32 = 0.85;
pub const LIGHT_DIRECTION: Vec3 = Vec3::new(0.45, 0.78, 0.42);
pub const LIGHT_COLOR: [f32; 3] = [1.0, 0.97, 0.90];

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct InstanceRaw {
    pub model: [[f32; 4]; 4],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct SceneUniform {
    pub view_proj: [[f32; 4]; 4],
    pub inv_view_proj: [[f32; 4]; 4],
    pub light_view_proj: [[f32; 4]; 4],
    pub camera_pos: [f32; 4],
    pub light_dir: [f32; 4],
    pub light_color: [f32; 4],
    pub sky_horizon: [f32; 4],
    pub sky_zenith: [f32; 4],
    pub ground_ambient: [f32; 4],
    pub fog_params: [f32; 4],
    pub shadow_params: [f32; 4],
}

pub enum RenderOutcome {
    Rendered(FrameMetrics),
    Skipped,
    OutOfMemory,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    fast_present_mode: wgpu::PresentMode,
    size: PhysicalSize<u32>,
    is_surface_configured: bool,
    depth_view: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    scene_bind_group_layout: wgpu::BindGroupLayout,
    scene_uniform_buffer: wgpu::Buffer,
    scene_bind_group: wgpu::BindGroup,
    ground_vertex_buffer: wgpu::Buffer,
    ground_index_buffer: wgpu::Buffer,
    ground_index_count: u32,
    ground_instance_buffer: wgpu::Buffer,
    cube_vertex_buffer: wgpu::Buffer,
    cube_index_buffer: wgpu::Buffer,
    cube_index_count: u32,
    cube_instance_buffer: wgpu::Buffer,
    cube_shadow_instance_buffer: wgpu::Buffer,
    model_renderer: ModelRenderer,
    shadow_pass: ShadowPass,
    sky_renderer: SkyRenderer,
    debug_lines: DebugLinesRenderer,
    post: PostProcess,
    hud: HudRenderer,
    gpu_timer: Option<GpuTimer>,
    cpu_instance_scratch: Vec<InstanceData>,
    gpu_instance_scratch: Vec<InstanceRaw>,
    actor_tag_scratch: Vec<RenderActorTag>,
    skin_slot_scratch: Vec<u32>,
    ally_skin_slots: Vec<u32>,
    enemy_skin_slots: Vec<u32>,
}

impl Renderer {
    pub fn new(window: Arc<Window>, cpu_models: &[CpuModel]) -> Self {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        let surface = instance.create_surface(Arc::clone(&window)).expect("Failed to create wgpu surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("Failed to find a suitable GPU adapter");

        let info = adapter.get_info();
        println!("PMU Adapter: {} (Backend: {:?})", info.name, info.backend);

        let supports_timestamps = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        println!("PMU Timestamp Queries (TIMESTAMP_QUERY): {}", if supports_timestamps { "Supported" } else { "Unsupported" });

        let required_features = if supports_timestamps { wgpu::Features::TIMESTAMP_QUERY } else { wgpu::Features::empty() };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("PMU Device"),
            required_features,
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .expect("Failed to create logical wgpu device");

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(surface_caps.formats[0]);

        let fast_present_mode = if surface_caps.present_modes.contains(&wgpu::PresentMode::Immediate) {
            wgpu::PresentMode::Immediate
        } else if surface_caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::Fifo
        };
        println!("PMU Present Modes: default = Fifo, benchmark toggle (F1) = {:?} (available: {:?})", fast_present_mode, surface_caps.present_modes);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        let is_surface_configured = if size.width > 0 && size.height > 0 {
            surface.configure(&device, &config);
            true
        } else {
            false
        };

        let post = PostProcess::new(&device, config.format, config.width, config.height);
        let (scene_w, scene_h) = post.scene_target_size();
        let depth_view = create_depth_view(&device, scene_w, scene_h);

        let scene_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Scene Uniform Buffer"),
            size: std::mem::size_of::<SceneUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let scene_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Scene Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { multisampled: false, view_dimension: wgpu::TextureViewDimension::D2, sample_type: wgpu::TextureSampleType::Depth },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });

        let model_renderer = ModelRenderer::new(
            &device,
            &queue,
            &scene_bind_group_layout,
            config.format,
            DEPTH_FORMAT,
            instance_buffer_layout(3),
            instance_buffer_layout(5),
            cpu_models,
        );

        let shadow_pass = ShadowPass::new(
            &device,
            &scene_uniform_buffer,
            model_renderer.skin_bind_group_layout(),
            vertex_buffer_layout(),
            instance_buffer_layout(2),
            static_model_vertex_buffer_layout(),
            instance_buffer_layout(3),
            skinned_model_vertex_buffer_layout(),
            instance_buffer_layout(5),
        );

        let scene_bind_group = create_scene_bind_group(
            &device,
            &scene_bind_group_layout,
            &scene_uniform_buffer,
            shadow_pass.depth_view(),
            shadow_pass.comparison_sampler(),
        );

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Main WGSL Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Main Pipeline Layout"),
            bind_group_layouts: &[&scene_bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Main Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_buffer_layout(), instance_buffer_layout(2)],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sky_renderer = SkyRenderer::new(&device, &scene_bind_group_layout, config.format, DEPTH_FORMAT);
        let debug_lines = DebugLinesRenderer::new(&device, &scene_bind_group_layout, config.format, DEPTH_FORMAT);
        let hud = HudRenderer::new(&device, &queue, config.format);

        let ground = ground_plane_mesh(GROUND_SIZE);
        let ground_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Ground Vertex Buffer"),
            contents: bytemuck::cast_slice(&ground.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ground_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Ground Index Buffer"),
            contents: bytemuck::cast_slice(&ground.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let ground_instance = [InstanceRaw { model: Mat4::IDENTITY.to_cols_array_2d(), color: GROUND_COLOR }];
        let ground_instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Ground Instance Buffer"),
            contents: bytemuck::cast_slice(&ground_instance),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let cube = cube_mesh();
        let cube_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Cube Vertex Buffer"),
            contents: bytemuck::cast_slice(&cube.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let cube_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Cube Index Buffer"),
            contents: bytemuck::cast_slice(&cube.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let cube_buf_size = (TOTAL_CUBE_INSTANCES * std::mem::size_of::<InstanceRaw>()) as u64;
        let cube_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Cube Main Instance Buffer"),
            size: cube_buf_size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cube_shadow_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Cube Shadow Instance Buffer"),
            size: cube_buf_size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let gpu_timer = GpuTimer::new(&device, &queue);

        Self {
            surface,
            device,
            queue,
            config,
            fast_present_mode,
            size,
            is_surface_configured,
            depth_view,
            pipeline,
            scene_bind_group_layout,
            scene_uniform_buffer,
            scene_bind_group,
            ground_vertex_buffer,
            ground_index_buffer,
            ground_index_count: ground.indices.len() as u32,
            ground_instance_buffer,
            cube_vertex_buffer,
            cube_index_buffer,
            cube_index_count: cube.indices.len() as u32,
            cube_instance_buffer,
            cube_shadow_instance_buffer,
            model_renderer,
            shadow_pass,
            sky_renderer,
            debug_lines,
            post,
            hud,
            gpu_timer,
            cpu_instance_scratch: Vec::with_capacity(TOTAL_CUBE_INSTANCES),
            gpu_instance_scratch: Vec::with_capacity(TOTAL_CUBE_INSTANCES),
            actor_tag_scratch: Vec::with_capacity(64),
            skin_slot_scratch: Vec::with_capacity(64),
            ally_skin_slots: Vec::with_capacity(8),
            enemy_skin_slots: Vec::with_capacity(32),
        }
    }

    pub fn supports_timestamps(&self) -> bool {
        self.gpu_timer.is_some()
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        self.size = new_size;
        if new_size.width == 0 || new_size.height == 0 {
            self.is_surface_configured = false;
            return;
        }
        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.surface.configure(&self.device, &self.config);
        self.post.resize(&self.device, new_size.width, new_size.height);
        let (scene_w, scene_h) = self.post.scene_target_size();
        self.depth_view = create_depth_view(&self.device, scene_w, scene_h);
        self.is_surface_configured = true;
    }

    pub fn toggle_present_mode(&mut self) -> wgpu::PresentMode {
        self.config.present_mode = if self.config.present_mode == wgpu::PresentMode::Fifo {
            self.fast_present_mode
        } else {
            wgpu::PresentMode::Fifo
        };
        if self.size.width > 0 && self.size.height > 0 {
            self.surface.configure(&self.device, &self.config);
            self.is_surface_configured = true;
        }
        println!("Present mode toggled (F1): {:?}", self.config.present_mode);
        self.config.present_mode
    }

    pub fn toggle_render_scale(&mut self) -> f32 {
        let new_scale = self.post.toggle_scale(&self.device, self.config.width, self.config.height);
        let (scene_w, scene_h) = self.post.scene_target_size();
        self.depth_view = create_depth_view(&self.device, scene_w, scene_h);
        println!("Render scale toggled (F2): {:.2}x ({}x{})", new_scale, scene_w, scene_h);
        new_scale
    }

    pub fn cycle_shadow_quality(&mut self) -> ShadowQuality {
        let quality = self.shadow_pass.cycle_quality(&self.device);
        self.scene_bind_group = create_scene_bind_group(
            &self.device,
            &self.scene_bind_group_layout,
            &self.scene_uniform_buffer,
            self.shadow_pass.depth_view(),
            self.shadow_pass.comparison_sampler(),
        );
        println!("Shadow quality cycled (F4): {}", quality.label());
        quality
    }

    pub fn render(
        &mut self,
        world: &World,
        camera: &Camera,
        alpha: f32,
        mut hud_telemetry: HudTelemetry<'_>,
    ) -> RenderOutcome {
        if !self.is_surface_configured || self.size.width == 0 || self.size.height == 0 {
            return RenderOutcome::Skipped;
        }

        let gpu_timings = self.gpu_timer.as_mut().and_then(|timer| timer.poll_and_read(&self.device));

        let output = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.resize(self.size);
                return RenderOutcome::Skipped;
            }
            Err(wgpu::SurfaceError::Timeout) => return RenderOutcome::Skipped,
            Err(wgpu::SurfaceError::OutOfMemory) => {
                eprintln!("Fatal wgpu error: SurfaceError::OutOfMemory. Exiting.");
                return RenderOutcome::OutOfMemory;
            }
            Err(wgpu::SurfaceError::Other) => {
                eprintln!("Warning: wgpu SurfaceError::Other encountered; skipping frame.");
                return RenderOutcome::Skipped;
            }
        };

        let surface_view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let (view_proj, eye) = camera.interpolated_view_proj_and_eye(alpha);
        let inv_view_proj = view_proj.inverse();
        let main_frustum = Frustum::from_view_proj(view_proj);
        let light_dir = LIGHT_DIRECTION.normalize();

        let shadow_quality = self.shadow_pass.quality();
        let shadows_enabled = shadow_quality.is_enabled();
        let shadow_res = shadow_quality.resolution();
        let player_pos = world.player.interpolated_pos(alpha);
        let light_view_proj = compute_light_view_proj(player_pos, light_dir, shadow_res);

        let scene_uniform = SceneUniform {
            view_proj: view_proj.to_cols_array_2d(),
            inv_view_proj: inv_view_proj.to_cols_array_2d(),
            light_view_proj: light_view_proj.to_cols_array_2d(),
            camera_pos: [eye.x, eye.y, eye.z, 1.0],
            light_dir: [light_dir.x, light_dir.y, light_dir.z, 0.0],
            light_color: [LIGHT_COLOR[0], LIGHT_COLOR[1], LIGHT_COLOR[2], AMBIENT_STRENGTH],
            sky_horizon: [SKY_HORIZON_COLOR[0], SKY_HORIZON_COLOR[1], SKY_HORIZON_COLOR[2], DIFFUSE_STRENGTH],
            sky_zenith: [SKY_ZENITH_COLOR[0], SKY_ZENITH_COLOR[1], SKY_ZENITH_COLOR[2], 0.0],
            ground_ambient: [GROUND_AMBIENT_COLOR[0], GROUND_AMBIENT_COLOR[1], GROUND_AMBIENT_COLOR[2], 0.0],
            fog_params: [FOG_START, FOG_END, 0.0, 0.0],
            shadow_params: [
                if shadows_enabled { 1.0 } else { 0.0 },
                1.0 / (shadow_res as f32),
                SHADOW_NORMAL_BIAS,
                SHADOW_ORTHO_RADIUS,
            ],
        };
        self.queue.write_buffer(&self.scene_uniform_buffer, 0, bytemuck::bytes_of(&scene_uniform));

        // Upload dynamic skin joint palettes for the player, allies, and enemies
        self.model_renderer.begin_frame_skin_uploads();
        let player_skin_slot = self.model_renderer.upload_skin_palette(&self.queue, world.animator.joint_matrices());

        self.ally_skin_slots.clear();
        for ally in &world.squad.allies {
            let slot = self.model_renderer.upload_skin_palette(&self.queue, &ally.pose_scratch.joint_matrices);
            self.ally_skin_slots.push(slot);
        }

        self.enemy_skin_slots.clear();
        for enemy in &world.enemies {
            let slot = self.model_renderer.upload_skin_palette(&self.queue, &enemy.pose_scratch.joint_matrices);
            self.enemy_skin_slots.push(slot);
        }

        let model_count = self.model_renderer.models.len();

        // 1. Shadow frustum culling & uploads
        let mut shadow_cube_count = 0u32;
        let mut shadow_model_instances = 0u32;
        if shadows_enabled {
            let light_frustum = Frustum::from_view_proj(light_view_proj);
            world.fill_visible_cubes(alpha, &light_frustum, &mut self.cpu_instance_scratch);
            self.gpu_instance_scratch.clear();
            for inst in &self.cpu_instance_scratch {
                self.gpu_instance_scratch.push(InstanceRaw { model: inst.model.to_cols_array_2d(), color: inst.color });
            }
            if self.gpu_instance_scratch.len() > TOTAL_CUBE_INSTANCES {
                self.gpu_instance_scratch.truncate(TOTAL_CUBE_INSTANCES);
            }
            shadow_cube_count = self.gpu_instance_scratch.len() as u32;
            if shadow_cube_count > 0 {
                self.queue.write_buffer(&self.cube_shadow_instance_buffer, 0, bytemuck::cast_slice(&self.gpu_instance_scratch));
            }

            for model_id in 0..model_count {
                let (aabb_min, aabb_max) = {
                    let m = &self.model_renderer.models[model_id];
                    (m.aabb_min, m.aabb_max)
                };
                world.fill_visible_models(
                    model_id, aabb_min, aabb_max, alpha, &light_frustum,
                    &mut self.cpu_instance_scratch, &mut self.actor_tag_scratch,
                );
                self.gpu_instance_scratch.clear();
                self.skin_slot_scratch.clear();
                for (i, inst) in self.cpu_instance_scratch.iter().enumerate() {
                    self.gpu_instance_scratch.push(InstanceRaw { model: inst.model.to_cols_array_2d(), color: inst.color });
                    let slot = match self.actor_tag_scratch.get(i).copied().unwrap_or(RenderActorTag::StaticProp) {
                        RenderActorTag::Player => player_skin_slot,
                        RenderActorTag::Ally(idx) => self.ally_skin_slots.get(idx).copied().unwrap_or(0),
                        RenderActorTag::Enemy(idx) => self.enemy_skin_slots.get(idx).copied().unwrap_or(0),
                        RenderActorTag::StaticProp => 0,
                    };
                    self.skin_slot_scratch.push(slot);
                }
                shadow_model_instances += self.gpu_instance_scratch.len() as u32;
                self.model_renderer.upload_shadow_instances(&self.queue, model_id, &self.gpu_instance_scratch, &self.skin_slot_scratch);
            }
        }

        // 2. Main camera frustum culling & uploads
        world.fill_visible_cubes(alpha, &main_frustum, &mut self.cpu_instance_scratch);
        self.gpu_instance_scratch.clear();
        for inst in &self.cpu_instance_scratch {
            self.gpu_instance_scratch.push(InstanceRaw { model: inst.model.to_cols_array_2d(), color: inst.color });
        }
        if self.gpu_instance_scratch.len() > TOTAL_CUBE_INSTANCES {
            self.gpu_instance_scratch.truncate(TOTAL_CUBE_INSTANCES);
        }
        let visible_cube_count = self.gpu_instance_scratch.len() as u32;
        if visible_cube_count > 0 {
            self.queue.write_buffer(&self.cube_instance_buffer, 0, bytemuck::cast_slice(&self.gpu_instance_scratch));
        }

        let mut visible_model_instances = 0u32;
        for model_id in 0..model_count {
            let (aabb_min, aabb_max) = {
                let m = &self.model_renderer.models[model_id];
                (m.aabb_min, m.aabb_max)
            };
            world.fill_visible_models(
                model_id, aabb_min, aabb_max, alpha, &main_frustum,
                &mut self.cpu_instance_scratch, &mut self.actor_tag_scratch,
            );
            self.gpu_instance_scratch.clear();
            self.skin_slot_scratch.clear();
            for (i, inst) in self.cpu_instance_scratch.iter().enumerate() {
                self.gpu_instance_scratch.push(InstanceRaw { model: inst.model.to_cols_array_2d(), color: inst.color });
                let slot = match self.actor_tag_scratch.get(i).copied().unwrap_or(RenderActorTag::StaticProp) {
                    RenderActorTag::Player => player_skin_slot,
                    RenderActorTag::Ally(idx) => self.ally_skin_slots.get(idx).copied().unwrap_or(0),
                    RenderActorTag::Enemy(idx) => self.enemy_skin_slots.get(idx).copied().unwrap_or(0),
                    RenderActorTag::StaticProp => 0,
                };
                self.skin_slot_scratch.push(slot);
            }
            visible_model_instances += self.gpu_instance_scratch.len() as u32;
            self.model_renderer.upload_instances(&self.queue, model_id, &self.gpu_instance_scratch, &self.skin_slot_scratch);
        }

        // 3. Prepare F7 and F8 debug lines
        self.debug_lines.prepare(
            &self.queue,
            world.debug_draw_enabled,
            world.ai_debug_enabled,
            world.collision.aabbs(),
            world.player_world_matrix(alpha),
            world.animator.bone_segments(),
            &world.squad.allies,
            &world.enemies,
            &world.nav,
        );

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Main Command Encoder"),
        });

        let mut draw_calls = 0u32;
        let mut triangles = 0u32;
        let mut skinned_triangles = 0u32;

        if shadows_enabled {
            let shadow_timestamp_writes = self.gpu_timer.as_ref().map(|timer| wgpu::RenderPassTimestampWrites {
                query_set: timer.query_set(),
                beginning_of_pass_write_index: Some(0),
                end_of_pass_write_index: Some(1),
            });

            let mut shadow_rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Directional Shadow Depth Pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: self.shadow_pass.depth_view(),
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: shadow_timestamp_writes,
                occlusion_query_set: None,
            });

            if shadow_cube_count > 0 {
                shadow_rp.set_pipeline(self.shadow_pass.cube_pipeline());
                shadow_rp.set_bind_group(0, self.shadow_pass.bind_group(), &[]);
                shadow_rp.set_vertex_buffer(0, self.cube_vertex_buffer.slice(..));
                shadow_rp.set_vertex_buffer(1, self.cube_shadow_instance_buffer.slice(..));
                shadow_rp.set_index_buffer(self.cube_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                shadow_rp.draw_indexed(0..self.cube_index_count, 0, 0..shadow_cube_count);
                draw_calls += 1;
                triangles += (self.cube_index_count / 3) * shadow_cube_count;
            }

            let (shd_model_draws, shd_model_tris, shd_skin_tris) = self.model_renderer.draw_shadow(
                &mut shadow_rp,
                self.shadow_pass.model_pipeline(),
                self.shadow_pass.skinned_model_pipeline(),
                self.shadow_pass.bind_group(),
            );
            draw_calls += shd_model_draws;
            triangles += shd_model_tris;
            skinned_triangles += shd_skin_tris;
        }

        let use_scaled_target = self.post.is_scaled();
        {
            let main_color_view = if use_scaled_target { self.post.offscreen_view() } else { &surface_view };
            let main_timestamp_writes = self.gpu_timer.as_ref().map(|timer| wgpu::RenderPassTimestampWrites {
                query_set: timer.query_set(),
                beginning_of_pass_write_index: Some(2),
                end_of_pass_write_index: Some(3),
            });

            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Main Scene Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: main_color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: FOG_COLOR[0] as f64, g: FOG_COLOR[1] as f64, b: FOG_COLOR[2] as f64, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: main_timestamp_writes,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.pipeline);
            render_pass.set_bind_group(0, &self.scene_bind_group, &[]);

            render_pass.set_vertex_buffer(0, self.ground_vertex_buffer.slice(..));
            render_pass.set_vertex_buffer(1, self.ground_instance_buffer.slice(..));
            render_pass.set_index_buffer(self.ground_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            render_pass.draw_indexed(0..self.ground_index_count, 0, 0..1);
            draw_calls += 1;
            triangles += self.ground_index_count / 3;

            if visible_cube_count > 0 {
                render_pass.set_vertex_buffer(0, self.cube_vertex_buffer.slice(..));
                render_pass.set_vertex_buffer(1, self.cube_instance_buffer.slice(..));
                render_pass.set_index_buffer(self.cube_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                render_pass.draw_indexed(0..self.cube_index_count, 0, 0..visible_cube_count);
                draw_calls += 1;
                triangles += (self.cube_index_count / 3) * visible_cube_count;
            }

            let (model_draws, model_tris, main_skin_tris) =
                self.model_renderer.draw(&mut render_pass, &self.scene_bind_group);
            draw_calls += model_draws;
            triangles += model_tris;
            skinned_triangles += main_skin_tris;

            self.sky_renderer.draw(&mut render_pass, &self.scene_bind_group);
            draw_calls += 1;
            triangles += 1;

            draw_calls += self.debug_lines.draw(&mut render_pass, &self.scene_bind_group);
        }

        if let Some(timer) = self.gpu_timer.as_mut() {
            timer.resolve_after_passes(&mut encoder, shadows_enabled);
        }

        if use_scaled_target {
            self.post.blit(&mut encoder, &surface_view);
            draw_calls += 1;
            triangles += 1;
        }

        let visible_instances = 1 + visible_cube_count + visible_model_instances;
        let shadow_instances = shadow_cube_count + shadow_model_instances;
        let total_instances = world.total_instance_count();

        hud_telemetry.player_pos = player_pos;
        hud_telemetry.visible_instances = visible_instances;
        hud_telemetry.total_instances = total_instances;
        hud_telemetry.shadow_instances = shadow_instances;
        hud_telemetry.shadow_quality = shadow_quality;
        hud_telemetry.present_mode = self.config.present_mode;
        hud_telemetry.render_scale = self.post.scale();
        hud_telemetry.model_yaw_offset_rad = world.model_yaw_offset();
        hud_telemetry.skinned_triangles = skinned_triangles;
        hud_telemetry.view_proj = view_proj;
        hud_telemetry.camera_pos = eye;

        self.hud.prepare(&self.queue, self.config.width, self.config.height, &hud_telemetry);
        let (hud_draws, hud_tris) = self.hud.draw(&mut encoder, &surface_view);
        draw_calls += hud_draws;
        triangles += hud_tris;

        self.queue.submit(std::iter::once(encoder.finish()));

        if let Some(timer) = self.gpu_timer.as_mut() {
            timer.initiate_map_after_submit();
        }

        output.present();

        RenderOutcome::Rendered(FrameMetrics {
            draw_calls,
            triangles,
            skinned_triangles,
            visible_instances,
            total_instances,
            shadow_instances,
            present_mode: self.config.present_mode,
            render_scale: self.post.scale(),
            shadow_quality,
            model_yaw_offset_rad: world.model_yaw_offset(),
            gpu_timings,
        })
    }
}

fn create_scene_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform_buffer: &wgpu::Buffer,
    shadow_depth_view: &wgpu::TextureView,
    shadow_sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Scene Bind Group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: uniform_buffer.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(shadow_depth_view) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(shadow_sampler) },
        ],
    })
}

fn create_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Depth Buffer Texture"),
        size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

fn vertex_buffer_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
            wgpu::VertexAttribute { offset: 12, shader_location: 1, format: wgpu::VertexFormat::Float32x3 },
        ],
    }
}

fn instance_buffer_layout(base_shader_location: u32) -> wgpu::VertexBufferLayout<'static> {
    let attributes: &'static [wgpu::VertexAttribute] = if base_shader_location == 2 {
        &[
            wgpu::VertexAttribute { offset: 0, shader_location: 2, format: wgpu::VertexFormat::Float32x4 },
            wgpu::VertexAttribute { offset: 16, shader_location: 3, format: wgpu::VertexFormat::Float32x4 },
            wgpu::VertexAttribute { offset: 32, shader_location: 4, format: wgpu::VertexFormat::Float32x4 },
            wgpu::VertexAttribute { offset: 48, shader_location: 5, format: wgpu::VertexFormat::Float32x4 },
            wgpu::VertexAttribute { offset: 64, shader_location: 6, format: wgpu::VertexFormat::Float32x4 },
        ]
    } else if base_shader_location == 3 {
        &[
            wgpu::VertexAttribute { offset: 0, shader_location: 3, format: wgpu::VertexFormat::Float32x4 },
            wgpu::VertexAttribute { offset: 16, shader_location: 4, format: wgpu::VertexFormat::Float32x4 },
            wgpu::VertexAttribute { offset: 32, shader_location: 5, format: wgpu::VertexFormat::Float32x4 },
            wgpu::VertexAttribute { offset: 48, shader_location: 6, format: wgpu::VertexFormat::Float32x4 },
            wgpu::VertexAttribute { offset: 64, shader_location: 7, format: wgpu::VertexFormat::Float32x4 },
        ]
    } else {
        &[
            wgpu::VertexAttribute { offset: 0, shader_location: 5, format: wgpu::VertexFormat::Float32x4 },
            wgpu::VertexAttribute { offset: 16, shader_location: 6, format: wgpu::VertexFormat::Float32x4 },
            wgpu::VertexAttribute { offset: 32, shader_location: 7, format: wgpu::VertexFormat::Float32x4 },
            wgpu::VertexAttribute { offset: 48, shader_location: 8, format: wgpu::VertexFormat::Float32x4 },
            wgpu::VertexAttribute { offset: 64, shader_location: 9, format: wgpu::VertexFormat::Float32x4 },
        ]
    };
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<InstanceRaw>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes,
    }
}
