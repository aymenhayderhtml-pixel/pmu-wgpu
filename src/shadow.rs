use glam::{Mat4, Vec3};

pub const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const SHADOW_ORTHO_RADIUS: f32 = 40.0;
pub const SHADOW_DEPTH_HALF_EXTENT: f32 = 60.0;
pub const SHADOW_NORMAL_BIAS: f32 = 0.035;

const SHADOW_SHADER_WGSL: &str = r#"
struct SceneUniform {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    light_dir: vec4<f32>,
    light_color: vec4<f32>,
    sky_horizon: vec4<f32>,
    sky_zenith: vec4<f32>,
    ground_ambient: vec4<f32>,
    fog_params: vec4<f32>,
    shadow_params: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> scene: SceneUniform;

struct CubeShadowInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) model_0: vec4<f32>,
    @location(3) model_1: vec4<f32>,
    @location(4) model_2: vec4<f32>,
    @location(5) model_3: vec4<f32>,
    @location(6) color: vec4<f32>,
};

struct ModelShadowInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) model_0: vec4<f32>,
    @location(4) model_1: vec4<f32>,
    @location(5) model_2: vec4<f32>,
    @location(6) model_3: vec4<f32>,
    @location(7) tint: vec4<f32>,
};

@vertex
fn vs_cube(in: CubeShadowInput) -> @builtin(position) vec4<f32> {
    let model = mat4x4<f32>(in.model_0, in.model_1, in.model_2, in.model_3);
    return scene.light_view_proj * (model * vec4<f32>(in.position, 1.0));
}

@vertex
fn vs_model(in: ModelShadowInput) -> @builtin(position) vec4<f32> {
    let model = mat4x4<f32>(in.model_0, in.model_1, in.model_2, in.model_3);
    return scene.light_view_proj * (model * vec4<f32>(in.position, 1.0));
}
"#;

const SKINNED_SHADOW_SHADER_WGSL: &str = r#"
struct SceneUniform {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    light_dir: vec4<f32>,
    light_color: vec4<f32>,
    sky_horizon: vec4<f32>,
    sky_zenith: vec4<f32>,
    ground_ambient: vec4<f32>,
    fog_params: vec4<f32>,
    shadow_params: vec4<f32>,
};

struct SkinUniform {
    joints: array<mat4x4<f32>, 128>,
};

@group(0) @binding(0)
var<uniform> scene: SceneUniform;
@group(1) @binding(0)
var<uniform> skin: SkinUniform;

struct SkinnedShadowInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) joints: vec4<u32>,
    @location(4) weights: vec4<f32>,
    @location(5) model_0: vec4<f32>,
    @location(6) model_1: vec4<f32>,
    @location(7) model_2: vec4<f32>,
    @location(8) model_3: vec4<f32>,
    @location(9) tint: vec4<f32>,
};

@vertex
fn vs_skinned(in: SkinnedShadowInput) -> @builtin(position) vec4<f32> {
    let skin_mat =
        skin.joints[in.joints.x] * in.weights.x +
        skin.joints[in.joints.y] * in.weights.y +
        skin.joints[in.joints.z] * in.weights.z +
        skin.joints[in.joints.w] * in.weights.w;
    let model = mat4x4<f32>(in.model_0, in.model_1, in.model_2, in.model_3);
    return scene.light_view_proj * (model * skin_mat * vec4<f32>(in.position, 1.0));
}
"#;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ShadowQuality {
    Off,
    Res1024,
    Res2048,
}

impl ShadowQuality {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::Res1024,
            Self::Res1024 => Self::Res2048,
            Self::Res2048 => Self::Off,
        }
    }

    pub fn resolution(self) -> u32 {
        match self {
            Self::Off => 1,
            Self::Res1024 => 1024,
            Self::Res2048 => 2048,
        }
    }

    pub fn is_enabled(self) -> bool {
        self != Self::Off
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Res1024 => "1024",
            Self::Res2048 => "2048",
        }
    }
}

pub fn compute_light_view_proj(player_pos: Vec3, light_dir: Vec3, resolution: u32) -> Mat4 {
    let forward = -light_dir.normalize();
    let up = if forward.y.abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    let light_view = Mat4::look_to_rh(Vec3::ZERO, forward, up);

    let anchor_world = Vec3::new(player_pos.x, 0.0, player_pos.z);
    let mut center_ls = light_view.transform_point3(anchor_world);

    let texel_size = (SHADOW_ORTHO_RADIUS * 2.0) / (resolution.max(1) as f32);
    center_ls.x = (center_ls.x / texel_size).floor() * texel_size;
    center_ls.y = (center_ls.y / texel_size).floor() * texel_size;

    let left = center_ls.x - SHADOW_ORTHO_RADIUS;
    let right = center_ls.x + SHADOW_ORTHO_RADIUS;
    let bottom = center_ls.y - SHADOW_ORTHO_RADIUS;
    let top = center_ls.y + SHADOW_ORTHO_RADIUS;
    let near = -center_ls.z - SHADOW_DEPTH_HALF_EXTENT;
    let far = -center_ls.z + SHADOW_DEPTH_HALF_EXTENT;

    let light_proj = Mat4::orthographic_rh(left, right, bottom, top, near, far);
    light_proj * light_view
}

pub struct ShadowPass {
    quality: ShadowQuality,
    depth_view: wgpu::TextureView,
    comparison_sampler: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
    cube_pipeline: wgpu::RenderPipeline,
    model_pipeline: wgpu::RenderPipeline,
    skinned_model_pipeline: wgpu::RenderPipeline,
}

impl ShadowPass {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        scene_uniform_buffer: &wgpu::Buffer,
        skin_bind_group_layout: &wgpu::BindGroupLayout,
        cube_vertex_layout: wgpu::VertexBufferLayout<'static>,
        cube_instance_layout: wgpu::VertexBufferLayout<'static>,
        static_model_vertex_layout: wgpu::VertexBufferLayout<'static>,
        static_model_instance_layout: wgpu::VertexBufferLayout<'static>,
        skinned_model_vertex_layout: wgpu::VertexBufferLayout<'static>,
        skinned_model_instance_layout: wgpu::VertexBufferLayout<'static>,
    ) -> Self {
        let quality = ShadowQuality::Res2048;
        let depth_view = create_shadow_depth_view(device, quality.resolution());

        let comparison_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Shadow Comparison Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Shadow Pass Uniform Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Shadow Pass Uniform Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: scene_uniform_buffer.as_entire_binding(),
            }],
        });

        let static_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shadow Depth-Only WGSL Shader"),
            source: wgpu::ShaderSource::Wgsl(SHADOW_SHADER_WGSL.into()),
        });
        let skinned_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Skinned Shadow Depth-Only WGSL Shader"),
            source: wgpu::ShaderSource::Wgsl(SKINNED_SHADOW_SHADER_WGSL.into()),
        });

        let static_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Shadow Static Pipeline Layout"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });
        let skinned_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Shadow Skinned Pipeline Layout"),
                bind_group_layouts: &[&bind_group_layout, skin_bind_group_layout],
                push_constant_ranges: &[],
            });

        let depth_stencil = Some(wgpu::DepthStencilState {
            format: SHADOW_FORMAT,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::LessEqual,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState {
                constant: 2,
                slope_scale: 2.0,
                clamp: 0.0,
            },
        });

        let primitive = wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        };

        let cube_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Cube Shadow Pipeline"),
            layout: Some(&static_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &static_shader,
                entry_point: Some("vs_cube"),
                buffers: &[cube_vertex_layout, cube_instance_layout],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: None,
            primitive,
            depth_stencil: depth_stencil.clone(),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let model_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Static Model Shadow Pipeline"),
            layout: Some(&static_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &static_shader,
                entry_point: Some("vs_model"),
                buffers: &[static_model_vertex_layout, static_model_instance_layout],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: None,
            primitive,
            depth_stencil: depth_stencil.clone(),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let skinned_model_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Skinned Model Shadow Pipeline"),
                layout: Some(&skinned_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &skinned_shader,
                    entry_point: Some("vs_skinned"),
                    buffers: &[skinned_model_vertex_layout, skinned_model_instance_layout],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: None,
                primitive,
                depth_stencil,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        Self {
            quality,
            depth_view,
            comparison_sampler,
            bind_group,
            cube_pipeline,
            model_pipeline,
            skinned_model_pipeline,
        }
    }

    pub fn quality(&self) -> ShadowQuality {
        self.quality
    }

    pub fn depth_view(&self) -> &wgpu::TextureView {
        &self.depth_view
    }

    pub fn comparison_sampler(&self) -> &wgpu::Sampler {
        &self.comparison_sampler
    }

    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }

    pub fn cube_pipeline(&self) -> &wgpu::RenderPipeline {
        &self.cube_pipeline
    }

    pub fn model_pipeline(&self) -> &wgpu::RenderPipeline {
        &self.model_pipeline
    }

    pub fn skinned_model_pipeline(&self) -> &wgpu::RenderPipeline {
        &self.skinned_model_pipeline
    }

    pub fn cycle_quality(&mut self, device: &wgpu::Device) -> ShadowQuality {
        self.quality = self.quality.cycle();
        self.depth_view = create_shadow_depth_view(device, self.quality.resolution());
        self.quality
    }
}

fn create_shadow_depth_view(device: &wgpu::Device, resolution: u32) -> wgpu::TextureView {
    let res = resolution.max(1);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Directional Shadow Map Texture"),
        size: wgpu::Extent3d {
            width: res,
            height: res,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: SHADOW_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}
