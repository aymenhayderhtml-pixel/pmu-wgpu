use crate::animation::MAX_JOINTS;
use crate::mipmap::upload_srgb_texture_with_mipmaps;
use crate::model::{CpuModel, CpuTexture, IndexBufferData, ModelVertex};
use crate::renderer::InstanceRaw;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use std::num::NonZeroU64;
use wgpu::util::DeviceExt;

pub const MAX_SKINNED_INSTANCES: usize = 24;
pub const JOINT_PALETTE_BYTES: u64 = (MAX_JOINTS * std::mem::size_of::<[[f32; 4]; 4]>()) as u64; // 8192

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct MaterialUniform {
    pub base_color_factor: [f32; 4],
}

pub struct GpuPrimitive {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
    pub index_format: wgpu::IndexFormat,
    pub material_bind_group: wgpu::BindGroup,
    pub triangle_count: u32,
}

pub struct GpuModel {
    pub primitives: Vec<GpuPrimitive>,
    pub instance_buffer: wgpu::Buffer,
    pub shadow_instance_buffer: wgpu::Buffer,
    pub is_skinned: bool,
    pub max_instances: usize,
    pub visible_instances: u32,
    pub shadow_visible_instances: u32,
    pub main_skin_slots: Vec<u32>,
    pub shadow_skin_slots: Vec<u32>,
    pub aabb_min: Vec3,
    pub aabb_max: Vec3,
}

pub struct ModelRenderer {
    static_pipeline: wgpu::RenderPipeline,
    skinned_pipeline: wgpu::RenderPipeline,
    skin_bind_group_layout: wgpu::BindGroupLayout,
    skin_uniform_buffer: wgpu::Buffer,
    skin_bind_group: wgpu::BindGroup,
    skin_slot_stride: u32,
    used_skin_slots: usize,
    overflow_warned: bool,
    pub models: Vec<GpuModel>,
}

impl ModelRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene_bind_group_layout: &wgpu::BindGroupLayout,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
        static_instance_layout: wgpu::VertexBufferLayout<'static>,
        skinned_instance_layout: wgpu::VertexBufferLayout<'static>,
        cpu_models: &[CpuModel],
    ) -> Self {
        let min_align = u64::from(device.limits().min_uniform_buffer_offset_alignment.max(1));
        let skin_slot_stride =
            (((JOINT_PALETTE_BYTES + min_align - 1) / min_align) * min_align) as u32;
        let total_slots = (MAX_SKINNED_INSTANCES + 1) as u64;
        let total_skin_buf_size = u64::from(skin_slot_stride) * total_slots;

        let skin_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Dynamic Skinned Joint Palette Buffer"),
            size: total_skin_buf_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let identity_joints = [Mat4::IDENTITY.to_cols_array_2d(); MAX_JOINTS];
        queue.write_buffer(
            &skin_uniform_buffer,
            0,
            bytemuck::cast_slice(&identity_joints),
        );

        let skin_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Dynamic Skin Joint Uniform Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: NonZeroU64::new(JOINT_PALETTE_BYTES),
                    },
                    count: None,
                }],
            });

        let skin_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Dynamic Skin Joint Bind Group"),
            layout: &skin_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &skin_uniform_buffer,
                    offset: 0,
                    size: NonZeroU64::new(JOINT_PALETTE_BYTES),
                }),
            }],
        });

        let material_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Model Material Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
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

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Model Linear Repeat Aniso4x Sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 4,
            ..Default::default()
        });

        let fallback_white = CpuTexture {
            width: 1,
            height: 1,
            rgba8: vec![255, 255, 255, 255],
        };
        let fallback_view =
            upload_srgb_texture_with_mipmaps(device, queue, &fallback_white, "Fallback White");

        let static_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Static Model WGSL Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("model.wgsl").into()),
        });
        let skinned_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Skinned Model WGSL Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("skinned.wgsl").into()),
        });

        let static_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Static Model Pipeline Layout"),
                bind_group_layouts: &[scene_bind_group_layout, &material_bind_group_layout],
                push_constant_ranges: &[],
            });
        let skinned_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Skinned Model Pipeline Layout"),
                bind_group_layouts: &[
                    scene_bind_group_layout,
                    &material_bind_group_layout,
                    &skin_bind_group_layout,
                ],
                push_constant_ranges: &[],
            });

        let primitive_state = wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        };
        let depth_stencil = Some(wgpu::DepthStencilState {
            format: depth_format,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        });

        let static_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Static Model Render Pipeline"),
            layout: Some(&static_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &static_shader,
                entry_point: Some("vs_main"),
                buffers: &[static_model_vertex_buffer_layout(), static_instance_layout],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &static_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: primitive_state,
            depth_stencil: depth_stencil.clone(),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let skinned_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Skinned Model Render Pipeline"),
            layout: Some(&skinned_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &skinned_shader,
                entry_point: Some("vs_main"),
                buffers: &[skinned_model_vertex_buffer_layout(), skinned_instance_layout],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &skinned_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: primitive_state,
            depth_stencil,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let mut models = Vec::with_capacity(cpu_models.len());
        for cpu_model in cpu_models {
            let mut gpu_primitives = Vec::with_capacity(cpu_model.primitives.len());
            for prim in &cpu_model.primitives {
                let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Model Vertex Buffer"),
                    contents: bytemuck::cast_slice(&prim.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });

                let (index_buffer, index_format) = match &prim.indices {
                    IndexBufferData::U16(indices) => {
                        let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("Model Index Buffer U16"),
                            contents: bytemuck::cast_slice(indices),
                            usage: wgpu::BufferUsages::INDEX,
                        });
                        (buf, wgpu::IndexFormat::Uint16)
                    }
                    IndexBufferData::U32(indices) => {
                        let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("Model Index Buffer U32"),
                            contents: bytemuck::cast_slice(indices),
                            usage: wgpu::BufferUsages::INDEX,
                        });
                        (buf, wgpu::IndexFormat::Uint32)
                    }
                };

                let index_count = prim.indices.count();
                let material_uniform = MaterialUniform {
                    base_color_factor: prim.material.base_color_factor,
                };
                let material_buffer =
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Model Material Uniform Buffer"),
                        contents: bytemuck::bytes_of(&material_uniform),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });

                let custom_tex_view = prim.material.base_color_texture.as_ref().map(|tex| {
                    upload_srgb_texture_with_mipmaps(
                        device,
                        queue,
                        tex,
                        "Model BaseColor Texture (Mipmapped)",
                    )
                });
                let tex_view_ref = custom_tex_view.as_ref().unwrap_or(&fallback_view);

                let material_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Model Material Bind Group"),
                    layout: &material_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: material_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(tex_view_ref),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&sampler),
                        },
                    ],
                });

                gpu_primitives.push(GpuPrimitive {
                    vertex_buffer,
                    index_buffer,
                    index_count,
                    index_format,
                    material_bind_group,
                    triangle_count: index_count / 3,
                });
            }

            let is_skinned = cpu_model.skeleton.is_some() && !cpu_model.animations.is_empty();
            let max_instances = cpu_model.max_instances.max(1);
            let instance_buffer_size =
                (max_instances * std::mem::size_of::<InstanceRaw>()) as u64;
            let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Per-Model Main Instance Buffer"),
                size: instance_buffer_size,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let shadow_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Per-Model Shadow Instance Buffer"),
                size: instance_buffer_size,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            models.push(GpuModel {
                primitives: gpu_primitives,
                instance_buffer,
                shadow_instance_buffer,
                is_skinned,
                max_instances,
                visible_instances: 0,
                shadow_visible_instances: 0,
                main_skin_slots: Vec::with_capacity(max_instances),
                shadow_skin_slots: Vec::with_capacity(max_instances),
                aabb_min: cpu_model.aabb_min,
                aabb_max: cpu_model.aabb_max,
            });
        }

        Self {
            static_pipeline,
            skinned_pipeline,
            skin_bind_group_layout,
            skin_uniform_buffer,
            skin_bind_group,
            skin_slot_stride,
            used_skin_slots: 0,
            overflow_warned: false,
            models,
        }
    }

    pub fn skin_bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.skin_bind_group_layout
    }

    pub fn begin_frame_skin_uploads(&mut self) {
        self.used_skin_slots = 0;
    }

    pub fn upload_skin_palette(
        &mut self,
        queue: &wgpu::Queue,
        joints: &[[[f32; 4]; 4]],
    ) -> u32 {
        if self.used_skin_slots >= MAX_SKINNED_INSTANCES {
            if !self.overflow_warned {
                eprintln!(
                    "Warning: more than {} visible skinned instances; farthest instances using static bind pose",
                    MAX_SKINNED_INSTANCES
                );
                self.overflow_warned = true;
            }
            return 0;
        }
        self.used_skin_slots += 1;
        let slot = self.used_skin_slots as u32;
        let byte_offset = u64::from(slot) * u64::from(self.skin_slot_stride);
        let count = joints.len().min(MAX_JOINTS);
        if count > 0 {
            queue.write_buffer(
                &self.skin_uniform_buffer,
                byte_offset,
                bytemuck::cast_slice(&joints[..count]),
            );
        }
        slot
    }

    pub fn upload_instances(
        &mut self,
        queue: &wgpu::Queue,
        model_id: usize,
        instances: &[InstanceRaw],
        skin_slots: &[u32],
    ) {
        if let Some(gpu_model) = self.models.get_mut(model_id) {
            let count = instances.len().min(gpu_model.max_instances);
            gpu_model.visible_instances = count as u32;
            gpu_model.main_skin_slots.clear();
            gpu_model
                .main_skin_slots
                .extend_from_slice(&skin_slots[..skin_slots.len().min(count)]);
            if count > 0 {
                queue.write_buffer(
                    &gpu_model.instance_buffer,
                    0,
                    bytemuck::cast_slice(&instances[..count]),
                );
            }
        }
    }

    pub fn upload_shadow_instances(
        &mut self,
        queue: &wgpu::Queue,
        model_id: usize,
        instances: &[InstanceRaw],
        skin_slots: &[u32],
    ) {
        if let Some(gpu_model) = self.models.get_mut(model_id) {
            let count = instances.len().min(gpu_model.max_instances);
            gpu_model.shadow_visible_instances = count as u32;
            gpu_model.shadow_skin_slots.clear();
            gpu_model
                .shadow_skin_slots
                .extend_from_slice(&skin_slots[..skin_slots.len().min(count)]);
            if count > 0 {
                queue.write_buffer(
                    &gpu_model.shadow_instance_buffer,
                    0,
                    bytemuck::cast_slice(&instances[..count]),
                );
            }
        }
    }

    pub fn draw_shadow<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        static_shadow_pipeline: &'a wgpu::RenderPipeline,
        skinned_shadow_pipeline: &'a wgpu::RenderPipeline,
        shadow_bind_group: &'a wgpu::BindGroup,
    ) -> (u32, u32, u32) {
        let mut draw_calls = 0;
        let mut triangles = 0;
        let mut skinned_triangles = 0;

        for model in &self.models {
            if model.shadow_visible_instances == 0 {
                continue;
            }
            render_pass.set_vertex_buffer(1, model.shadow_instance_buffer.slice(..));

            if model.is_skinned {
                render_pass.set_pipeline(skinned_shadow_pipeline);
                render_pass.set_bind_group(0, shadow_bind_group, &[]);
                for inst_idx in 0..model.shadow_visible_instances {
                    let slot = model
                        .shadow_skin_slots
                        .get(inst_idx as usize)
                        .copied()
                        .unwrap_or(0);
                    let dyn_offset = slot * self.skin_slot_stride;
                    render_pass.set_bind_group(1, &self.skin_bind_group, &[dyn_offset]);
                    for prim in &model.primitives {
                        render_pass.set_vertex_buffer(0, prim.vertex_buffer.slice(..));
                        render_pass
                            .set_index_buffer(prim.index_buffer.slice(..), prim.index_format);
                        render_pass.draw_indexed(
                            0..prim.index_count,
                            0,
                            inst_idx..(inst_idx + 1),
                        );
                        draw_calls += 1;
                        triangles += prim.triangle_count;
                        skinned_triangles += prim.triangle_count;
                    }
                }
            } else {
                render_pass.set_pipeline(static_shadow_pipeline);
                render_pass.set_bind_group(0, shadow_bind_group, &[]);
                for prim in &model.primitives {
                    render_pass.set_vertex_buffer(0, prim.vertex_buffer.slice(..));
                    render_pass.set_index_buffer(prim.index_buffer.slice(..), prim.index_format);
                    render_pass.draw_indexed(
                        0..prim.index_count,
                        0,
                        0..model.shadow_visible_instances,
                    );
                    draw_calls += 1;
                    triangles += prim.triangle_count * model.shadow_visible_instances;
                }
            }
        }

        (draw_calls, triangles, skinned_triangles)
    }

    pub fn draw<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        scene_bind_group: &'a wgpu::BindGroup,
    ) -> (u32, u32, u32) {
        let mut draw_calls = 0;
        let mut triangles = 0;
        let mut skinned_triangles = 0;

        for model in &self.models {
            if model.visible_instances == 0 {
                continue;
            }
            render_pass.set_vertex_buffer(1, model.instance_buffer.slice(..));

            if model.is_skinned {
                render_pass.set_pipeline(&self.skinned_pipeline);
                render_pass.set_bind_group(0, scene_bind_group, &[]);
                for inst_idx in 0..model.visible_instances {
                    let slot = model
                        .main_skin_slots
                        .get(inst_idx as usize)
                        .copied()
                        .unwrap_or(0);
                    let dyn_offset = slot * self.skin_slot_stride;
                    render_pass.set_bind_group(2, &self.skin_bind_group, &[dyn_offset]);
                    for prim in &model.primitives {
                        render_pass.set_bind_group(1, &prim.material_bind_group, &[]);
                        render_pass.set_vertex_buffer(0, prim.vertex_buffer.slice(..));
                        render_pass
                            .set_index_buffer(prim.index_buffer.slice(..), prim.index_format);
                        render_pass.draw_indexed(
                            0..prim.index_count,
                            0,
                            inst_idx..(inst_idx + 1),
                        );
                        draw_calls += 1;
                        triangles += prim.triangle_count;
                        skinned_triangles += prim.triangle_count;
                    }
                }
            } else {
                render_pass.set_pipeline(&self.static_pipeline);
                render_pass.set_bind_group(0, scene_bind_group, &[]);
                for prim in &model.primitives {
                    render_pass.set_bind_group(1, &prim.material_bind_group, &[]);
                    render_pass.set_vertex_buffer(0, prim.vertex_buffer.slice(..));
                    render_pass.set_index_buffer(prim.index_buffer.slice(..), prim.index_format);
                    render_pass.draw_indexed(0..prim.index_count, 0, 0..model.visible_instances);
                    draw_calls += 1;
                    triangles += prim.triangle_count * model.visible_instances;
                }
            }
        }

        (draw_calls, triangles, skinned_triangles)
    }
}

pub fn static_model_vertex_buffer_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<ModelVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: 12,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: 24,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x2,
            },
        ],
    }
}

pub fn skinned_model_vertex_buffer_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<ModelVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: 12,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: 24,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 32,
                shader_location: 3,
                format: wgpu::VertexFormat::Uint16x4,
            },
            wgpu::VertexAttribute {
                offset: 40,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32x4,
            },
        ],
    }
}
