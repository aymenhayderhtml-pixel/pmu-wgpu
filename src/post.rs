pub const SCALE_FULL: f32 = 1.0;
pub const SCALE_REDUCED: f32 = 0.75;

pub struct PostProcess {
    scale: f32,
    format: wgpu::TextureFormat,
    target_width: u32,
    target_height: u32,
    offscreen_view: wgpu::TextureView,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
}

impl PostProcess {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        window_width: u32,
        window_height: u32,
    ) -> Self {
        let scale = SCALE_FULL;
        let (target_width, target_height) = compute_scaled_size(window_width, window_height, scale);

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("PostProcess Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("PostProcess Linear Clamp Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let offscreen_view =
            create_offscreen_color_view(device, format, target_width, target_height);
        let bind_group =
            create_post_bind_group(device, &bind_group_layout, &offscreen_view, &sampler);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("PostProcess Blit Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("post.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("PostProcess Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("PostProcess Blit Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
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

        Self {
            scale,
            format,
            target_width,
            target_height,
            offscreen_view,
            bind_group_layout,
            bind_group,
            sampler,
            pipeline,
        }
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn is_scaled(&self) -> bool {
        (self.scale - SCALE_FULL).abs() > 1e-4
    }

    pub fn scene_target_size(&self) -> (u32, u32) {
        (self.target_width, self.target_height)
    }

    pub fn offscreen_view(&self) -> &wgpu::TextureView {
        &self.offscreen_view
    }

    pub fn set_scale(
        &mut self,
        device: &wgpu::Device,
        new_scale: f32,
        window_width: u32,
        window_height: u32,
    ) {
        let clamped = new_scale.clamp(0.5, SCALE_FULL);
        if (self.scale - clamped).abs() > 1e-4 {
            self.scale = clamped;
            self.resize(device, window_width, window_height);
        }
    }

    pub fn resize(&mut self, device: &wgpu::Device, window_width: u32, window_height: u32) {
        let (w, h) = compute_scaled_size(window_width, window_height, self.scale);
        self.target_width = w;
        self.target_height = h;
        self.offscreen_view = create_offscreen_color_view(device, self.format, w, h);
        self.bind_group = create_post_bind_group(
            device,
            &self.bind_group_layout,
            &self.offscreen_view,
            &self.sampler,
        );
    }

    /// Blits the offscreen scene texture to the swapchain surface using a single fullscreen triangle.
    pub fn blit(&self, encoder: &mut wgpu::CommandEncoder, dst_view: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("PostProcess Fullscreen Blit Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: dst_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn compute_scaled_size(width: u32, height: u32, scale: f32) -> (u32, u32) {
    let w = ((width.max(1) as f32) * scale).round() as u32;
    let h = ((height.max(1) as f32) * scale).round() as u32;
    (w.max(1), h.max(1))
}

fn create_offscreen_color_view(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Offscreen Render Scale Color Texture"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

fn create_post_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("PostProcess Bind Group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}
