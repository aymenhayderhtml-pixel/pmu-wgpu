use crate::ally::MAX_ALLIES;
use crate::animation::{
    normalize_weights, AnimChannel, AnimationClip, ChannelValues, InterpMode, LocalTrs, NodeDef,
    Skeleton, MAX_JOINTS,
};
use crate::mesh::cube_mesh;
use crate::waves::MAX_ALIVE_ENEMIES;
use crate::world::{PLAYER_COLOR, PLAYER_SCALE, PROP_MAX_INSTANCES};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Quat, Vec3};
use gltf::accessor::{DataType, Dimensions, Iter};
use std::path::{Path, PathBuf};

pub const PLAYER_MODEL_PATH: &str = "assets/models/player.glb";
pub const ALLY_MODEL_PATH: &str = "assets/models/ally.glb";
pub const ENEMY_MODEL_PATH: &str = "assets/models/enemy.glb";
pub const PROP_MODEL_PATH: &str = "assets/models/prop.glb";

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct ModelVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub joints: [u16; 4],
    pub weights: [f32; 4],
}

#[derive(Clone, Debug)]
pub enum IndexBufferData {
    U16(Vec<u16>),
    U32(Vec<u32>),
}

impl IndexBufferData {
    pub fn count(&self) -> u32 {
        match self {
            Self::U16(v) => v.len() as u32,
            Self::U32(v) => v.len() as u32,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CpuTexture {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct CpuMaterial {
    pub base_color_factor: [f32; 4],
    pub base_color_texture: Option<CpuTexture>,
}

#[derive(Clone, Debug)]
pub struct CpuPrimitive {
    pub vertices: Vec<ModelVertex>,
    pub indices: IndexBufferData,
    pub material: CpuMaterial,
}

#[derive(Clone, Debug)]
pub struct CpuModel {
    pub primitives: Vec<CpuPrimitive>,
    pub aabb_min: Vec3,
    pub aabb_max: Vec3,
    pub max_instances: usize,
    pub skeleton: Option<Skeleton>,
    pub animations: Vec<AnimationClip>,
}

impl CpuModel {
    pub fn blue_box_fallback(max_instances: usize) -> Self {
        let cube = cube_mesh();
        let vertices: Vec<ModelVertex> = cube
            .vertices
            .into_iter()
            .map(|v| ModelVertex {
                position: [
                    v.position[0] * PLAYER_SCALE.x,
                    (v.position[1] + 0.5) * PLAYER_SCALE.y,
                    v.position[2] * PLAYER_SCALE.z,
                ],
                normal: v.normal,
                uv: [0.0, 0.0],
                joints: [0, 0, 0, 0],
                weights: [1.0, 0.0, 0.0, 0.0],
            })
            .collect();

        let primitive = CpuPrimitive {
            vertices,
            indices: IndexBufferData::U16(cube.indices),
            material: CpuMaterial {
                base_color_factor: PLAYER_COLOR,
                base_color_texture: None,
            },
        };

        Self {
            primitives: vec![primitive],
            aabb_min: Vec3::new(-0.5 * PLAYER_SCALE.x, 0.0, -0.5 * PLAYER_SCALE.z),
            aabb_max: Vec3::new(0.5 * PLAYER_SCALE.x, PLAYER_SCALE.y, 0.5 * PLAYER_SCALE.z),
            max_instances,
            skeleton: None,
            animations: Vec::new(),
        }
    }

    fn recompute_bounds(&mut self) {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for prim in &self.primitives {
            for v in &prim.vertices {
                let p = Vec3::from_array(v.position);
                min = min.min(p);
                max = max.max(p);
            }
        }
        self.aabb_min = min;
        self.aabb_max = max;
    }

    pub fn normalize_to_height(&mut self, target_height: f32) -> Result<(f32, Mat4), String> {
        self.recompute_bounds();
        if !self.aabb_min.is_finite() || !self.aabb_max.is_finite() {
            return Err("bounding box contains non-finite coordinates".to_string());
        }
        let size = self.aabb_max - self.aabb_min;
        if size.y <= 1e-6 {
            return Err(format!(
                "degenerate bounding box with zero height (min: {:?}, max: {:?})",
                self.aabb_min, self.aabb_max
            ));
        }

        let scale = target_height / size.y;
        let center_x = (self.aabb_min.x + self.aabb_max.x) * 0.5;
        let center_z = (self.aabb_min.z + self.aabb_max.z) * 0.5;
        let min_y = self.aabb_min.y;

        for prim in &mut self.primitives {
            for v in &mut prim.vertices {
                v.position[0] = (v.position[0] - center_x) * scale;
                v.position[1] = (v.position[1] - min_y) * scale;
                v.position[2] = (v.position[2] - center_z) * scale;
            }
        }
        self.recompute_bounds();

        let fit_mat = Mat4::from_scale(Vec3::splat(scale))
            * Mat4::from_translation(Vec3::new(-center_x, -min_y, -center_z));
        Ok((scale, fit_mat))
    }
}

pub struct SceneModels {
    pub models: Vec<CpuModel>,
    pub player_model_id: Option<usize>,
    pub ally_model_id: Option<usize>,
    pub enemy_model_id: Option<usize>,
    pub prop_model_id: Option<usize>,
}

pub fn load_scene_models() -> SceneModels {
    let mut models = Vec::new();
    let mut player_model_id = None;
    let mut ally_model_id = None;
    let mut enemy_model_id = None;
    let mut prop_model_id = None;

    let player_path = Path::new(PLAYER_MODEL_PATH);
    let ally_path = Path::new(ALLY_MODEL_PATH);
    let enemy_path = Path::new(ENEMY_MODEL_PATH);
    let ally_has_own_file = ally_path.exists();
    let enemy_has_own_file = enemy_path.exists();

    if player_path.exists() {
        let mut max_inst = 1;
        if !ally_has_own_file {
            max_inst += MAX_ALLIES;
        }
        if !enemy_has_own_file {
            max_inst += MAX_ALIVE_ENEMIES;
        }
        let model = load_model_or_fallback(player_path, max_inst);
        let id = models.len();
        player_model_id = Some(id);
        if !ally_has_own_file {
            ally_model_id = Some(id);
            println!(
                "No '{}' found; allies will reuse player model with rarity tint.",
                ally_path.display()
            );
        }
        if !enemy_has_own_file {
            enemy_model_id = Some(id);
            println!(
                "No '{}' found; enemies will reuse player model with red tint.",
                enemy_path.display()
            );
        }
        models.push(model);
    }

    if ally_has_own_file {
        let model = load_model_or_fallback(ally_path, MAX_ALLIES);
        ally_model_id = Some(models.len());
        models.push(model);
    }

    if enemy_has_own_file {
        let model = load_model_or_fallback(enemy_path, MAX_ALIVE_ENEMIES);
        enemy_model_id = Some(models.len());
        models.push(model);
    }

    let prop_path = Path::new(PROP_MODEL_PATH);
    if prop_path.exists() {
        let model = load_model_or_fallback(prop_path, PROP_MAX_INSTANCES);
        prop_model_id = Some(models.len());
        models.push(model);
    }

    SceneModels {
        models,
        player_model_id,
        ally_model_id,
        enemy_model_id,
        prop_model_id,
    }
}

fn load_model_or_fallback(path: &Path, max_instances: usize) -> CpuModel {
    match try_load_gltf(path, max_instances) {
        Ok(model) => model,
        Err(err) => {
            eprintln!(
                "Error loading '{}': {}. Falling back to blue box.",
                path.display(),
                err
            );
            CpuModel::blue_box_fallback(max_instances)
        }
    }
}

fn is_supported_extension(ext: &str) -> bool {
    matches!(ext, "KHR_mesh_quantization" | "KHR_texture_transform")
}

fn try_load_gltf(path: &Path, max_instances: usize) -> Result<CpuModel, String> {
    let file_bytes =
        std::fs::read(path).map_err(|e| format!("failed to read model file: {e}"))?;

    let gltf = gltf::Gltf::from_slice_without_validation(&file_bytes)
        .map_err(|e| format!("failed to parse glTF container/JSON: {e}"))?;

    for req_ext in gltf.document.extensions_required() {
        if !is_supported_extension(req_ext) {
            return Err(format!("unsupported required extension '{req_ext}'"));
        }
    }

    let ignored_optional_exts: Vec<&str> = gltf
        .document
        .extensions_used()
        .filter(|ext| !is_supported_extension(ext))
        .collect();
    if !ignored_optional_exts.is_empty() {
        eprintln!(
            "Warning [{}]: ignoring optional glTF extension(s): {}",
            path.display(),
            ignored_optional_exts.join(", ")
        );
    }

    let cameras = gltf.document.cameras().count();
    if cameras > 0 {
        eprintln!(
            "Warning [{}]: ignoring {} glTF camera(s)",
            path.display(),
            cameras
        );
    }

    let base_dir = path.parent();
    let buffers = gltf::import_buffers(&gltf.document, base_dir, gltf.blob)
        .map_err(|e| format!("failed to load binary buffers: {e}"))?;

    let mut primitives = Vec::new();
    let mut skinned_mesh_world = Mat4::IDENTITY;
    let mut has_morph_targets = false;

    if let Some(scene) = gltf.document.default_scene() {
        for node in scene.nodes() {
            visit_node(
                &node,
                Mat4::IDENTITY,
                &buffers,
                base_dir,
                path,
                &mut primitives,
                &mut skinned_mesh_world,
                &mut has_morph_targets,
            );
        }
    } else if gltf.document.scenes().count() > 0 {
        for scene in gltf.document.scenes() {
            for node in scene.nodes() {
                visit_node(
                    &node,
                    Mat4::IDENTITY,
                    &buffers,
                    base_dir,
                    path,
                    &mut primitives,
                    &mut skinned_mesh_world,
                    &mut has_morph_targets,
                );
            }
        }
    } else {
        for mesh in gltf.document.meshes() {
            extract_mesh_primitives(
                &mesh,
                Mat4::IDENTITY,
                &buffers,
                base_dir,
                path,
                &mut primitives,
                &mut has_morph_targets,
            );
        }
    }

    if has_morph_targets {
        eprintln!(
            "Warning [{}]: ignoring morph targets (not supported)",
            path.display()
        );
    }

    if primitives.is_empty() {
        return Err("model contains no valid triangle primitives".to_string());
    }

    let mut model = CpuModel {
        primitives,
        aabb_min: Vec3::ZERO,
        aabb_max: Vec3::ZERO,
        max_instances,
        skeleton: None,
        animations: Vec::new(),
    };

    model.recompute_bounds();
    let raw_min = model.aabb_min;
    let raw_max = model.aabb_max;

    let (scale_factor, fit_matrix) = model.normalize_to_height(PLAYER_SCALE.y)?;

    let (skeleton, animations) =
        load_skin_and_animations(&gltf.document, &buffers, path, fit_matrix, skinned_mesh_world);
    model.skeleton = skeleton;
    model.animations = animations;

    let total_vertices: usize = model.primitives.iter().map(|p| p.vertices.len()).sum();
    let total_triangles: u32 = model.primitives.iter().map(|p| p.indices.count() / 3).sum();

    println!(
        "Model loaded: file='{}', primitives={}, vertices={}, triangles={}, bbox_raw=([{:.4}, {:.4}, {:.4}]..[{:.4}, {:.4}, {:.4}]), bbox_fit=([{:.4}, {:.4}, {:.4}]..[{:.4}, {:.4}, {:.4}]), scale_to_2m={:.6}",
        path.display(),
        model.primitives.len(),
        total_vertices,
        total_triangles,
        raw_min.x,
        raw_min.y,
        raw_min.z,
        raw_max.x,
        raw_max.y,
        raw_max.z,
        model.aabb_min.x,
        model.aabb_min.y,
        model.aabb_min.z,
        model.aabb_max.x,
        model.aabb_max.y,
        model.aabb_max.z,
        scale_factor
    );

    Ok(model)
}

fn load_skin_and_animations(
    doc: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    file_path: &Path,
    fit_matrix: Mat4,
    skinned_mesh_world: Mat4,
) -> (Option<Skeleton>, Vec<AnimationClip>) {
    let skin_count = doc.skins().count();
    let node_count = doc.nodes().count();

    let mut nodes = vec![
        NodeDef {
            parent: None,
            base_trs: LocalTrs::default(),
        };
        node_count
    ];

    for node in doc.nodes() {
        let idx = node.index();
        let (t, r, s) = node.transform().decomposed();
        if idx < node_count {
            nodes[idx].base_trs = LocalTrs {
                translation: Vec3::from_array(t),
                rotation: Quat::from_array(r).normalize(),
                scale: Vec3::from_array(s),
            };
        }
        for child in node.children() {
            if child.index() < node_count {
                nodes[child.index()].parent = Some(idx);
            }
        }
    }

    let mut skeleton_opt = None;
    let mut joint_count = 0usize;

    if let Some(skin) = doc.skins().next() {
        let joint_nodes: Vec<usize> = skin.joints().map(|j| j.index()).collect();
        joint_count = joint_nodes.len();

        if joint_count == 0 {
            eprintln!(
                "Warning [{}]: skin has 0 joints; falling back to static model",
                file_path.display()
            );
        } else if joint_count > MAX_JOINTS {
            eprintln!(
                "Error [{}]: skin has {} joints (exceeds max {}); falling back to static model",
                file_path.display(),
                joint_count,
                MAX_JOINTS
            );
        } else {
            let mut ibms = vec![Mat4::IDENTITY; joint_count];
            let ibm_valid = skin
                .inverse_bind_matrices()
                .map_or(true, |acc| validate_accessor_bounds(&acc, buffers));

            if ibm_valid {
                let reader = skin.reader(|buf| buffers.get(buf.index()).map(|d| &d[..]));
                if let Some(iter) = reader.read_inverse_bind_matrices() {
                    for (i, mat_arr) in iter.enumerate().take(joint_count) {
                        ibms[i] = Mat4::from_cols_array_2d(&mat_arr);
                    }
                }
            } else {
                eprintln!(
                    "Warning [{}]: invalid inverseBindMatrices accessor; using identity IBMs",
                    file_path.display()
                );
            }

            let inv_fit_mesh = (fit_matrix * skinned_mesh_world).inverse();
            for ibm in &mut ibms {
                *ibm = *ibm * inv_fit_mesh;
            }

            skeleton_opt = Some(Skeleton::new(nodes, joint_nodes, ibms, fit_matrix));
        }
    }

    let mut clips = Vec::new();
    let mut warned_cubicspline = false;
    let mut warned_morph_channel = false;

    if skeleton_opt.is_some() {
        for (clip_idx, anim) in doc.animations().enumerate() {
            let name = anim
                .name()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("clip_{}", clip_idx));

            let mut duration = 0.0_f32;
            let mut channels = Vec::new();

            for channel in anim.channels() {
                let target_node = channel.target().node().index();
                let prop = channel.target().property();
                if prop == gltf::animation::Property::MorphTargetWeights {
                    if !warned_morph_channel {
                        eprintln!(
                            "Warning [{}]: ignoring morph target animation channel(s)",
                            file_path.display()
                        );
                        warned_morph_channel = true;
                    }
                    continue;
                }

                let sampler = channel.sampler();
                if !validate_accessor_bounds(&sampler.input(), buffers)
                    || !validate_accessor_bounds(&sampler.output(), buffers)
                {
                    eprintln!(
                        "Warning [{}]: skipping animation channel with out-of-bounds accessor",
                        file_path.display()
                    );
                    continue;
                }

                let is_cubic =
                    sampler.interpolation() == gltf::animation::Interpolation::CubicSpline;
                if is_cubic && !warned_cubicspline {
                    eprintln!(
                        "Warning [{}]: CUBICSPLINE animation channel(s) treated as LINEAR (tangents stripped)",
                        file_path.display()
                    );
                    warned_cubicspline = true;
                }

                let interp = match sampler.interpolation() {
                    gltf::animation::Interpolation::Step => InterpMode::Step,
                    gltf::animation::Interpolation::Linear
                    | gltf::animation::Interpolation::CubicSpline => InterpMode::Linear,
                };

                let reader = channel.reader(|buf| buffers.get(buf.index()).map(|d| &d[..]));
                let Some(inputs) = reader.read_inputs() else {
                    continue;
                };
                let times: Vec<f32> = inputs.collect();
                if times.is_empty() {
                    continue;
                }
                if let Some(&last_t) = times.last() {
                    duration = duration.max(last_t);
                }

                let Some(outputs) = reader.read_outputs() else {
                    continue;
                };

                let values = match outputs {
                    gltf::animation::util::ReadOutputs::Translations(iter) => {
                        let raw: Vec<Vec3> = iter.map(Vec3::from_array).collect();
                        ChannelValues::Translations(strip_cubic_tangents(raw, times.len(), is_cubic))
                    }
                    gltf::animation::util::ReadOutputs::Rotations(iter) => {
                        let raw: Vec<Quat> = iter
                            .into_f32()
                            .map(|q| Quat::from_array(q).normalize())
                            .collect();
                        ChannelValues::Rotations(strip_cubic_tangents(raw, times.len(), is_cubic))
                    }
                    gltf::animation::util::ReadOutputs::Scales(iter) => {
                        let raw: Vec<Vec3> = iter.map(Vec3::from_array).collect();
                        ChannelValues::Scales(strip_cubic_tangents(raw, times.len(), is_cubic))
                    }
                    gltf::animation::util::ReadOutputs::MorphTargetWeights(_) => continue,
                };

                channels.push(AnimChannel {
                    node_index: target_node,
                    interpolation: interp,
                    times,
                    values,
                });
            }

            if !channels.is_empty() {
                clips.push(AnimationClip {
                    name,
                    duration,
                    channels,
                });
            }
        }
    }

    let clip_summary: Vec<String> = clips
        .iter()
        .map(|c| format!("'{}' ({:.2}s)", c.name, c.duration))
        .collect();
    println!(
        "Model animation info [{}]: skins={}, joints={}, clips={} [{}]",
        file_path.display(),
        skin_count,
        if skeleton_opt.is_some() { joint_count } else { 0 },
        clips.len(),
        clip_summary.join(", ")
    );

    (skeleton_opt, clips)
}

fn strip_cubic_tangents<T: Copy>(raw: Vec<T>, key_count: usize, is_cubic: bool) -> Vec<T> {
    if is_cubic && raw.len() == key_count * 3 {
        raw.chunks_exact(3).map(|chunk| chunk[1]).collect()
    } else {
        raw
    }
}

#[allow(clippy::too_many_arguments)]
fn visit_node(
    node: &gltf::Node,
    parent_transform: Mat4,
    buffers: &[gltf::buffer::Data],
    base_dir: Option<&Path>,
    file_path: &Path,
    out_primitives: &mut Vec<CpuPrimitive>,
    skinned_mesh_world: &mut Mat4,
    has_morph_targets: &mut bool,
) {
    let local_matrix = Mat4::from_cols_array_2d(&node.transform().matrix());
    let world_matrix = parent_transform * local_matrix;

    if let Some(mesh) = node.mesh() {
        if node.skin().is_some() {
            *skinned_mesh_world = world_matrix;
        }
        extract_mesh_primitives(
            &mesh,
            world_matrix,
            buffers,
            base_dir,
            file_path,
            out_primitives,
            has_morph_targets,
        );
    }

    for child in node.children() {
        visit_node(
            &child,
            world_matrix,
            buffers,
            base_dir,
            file_path,
            out_primitives,
            skinned_mesh_world,
            has_morph_targets,
        );
    }
}

fn extract_mesh_primitives(
    mesh: &gltf::Mesh,
    world_matrix: Mat4,
    buffers: &[gltf::buffer::Data],
    base_dir: Option<&Path>,
    file_path: &Path,
    out_primitives: &mut Vec<CpuPrimitive>,
    has_morph_targets: &mut bool,
) {
    let c0 = world_matrix.x_axis.truncate();
    let c1 = world_matrix.y_axis.truncate();
    let c2 = world_matrix.z_axis.truncate();
    let det = c0.dot(c1.cross(c2));
    let sign = if det < 0.0 { -1.0 } else { 1.0 };
    let adj0 = c1.cross(c2) * sign;
    let adj1 = c2.cross(c0) * sign;
    let adj2 = c0.cross(c1) * sign;

    for primitive in mesh.primitives() {
        if primitive.morph_targets().next().is_some() {
            *has_morph_targets = true;
        }

        if primitive.mode() != gltf::mesh::Mode::Triangles {
            eprintln!(
                "Warning [{}]: skipping non-triangle primitive mode {:?}",
                file_path.display(),
                primitive.mode()
            );
            continue;
        }

        let Some(pos_accessor) = primitive.get(&gltf::Semantic::Positions) else {
            eprintln!(
                "Warning [{}]: skipping primitive with missing POSITION attribute",
                file_path.display()
            );
            continue;
        };

        let raw_positions = match read_vec3_accessor(pos_accessor, buffers) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "Warning [{}]: skipping primitive with invalid POSITION accessor: {}",
                    file_path.display(),
                    e
                );
                continue;
            }
        };

        let positions: Vec<Vec3> = raw_positions
            .into_iter()
            .map(|p| world_matrix.transform_point3(Vec3::from_array(p)))
            .collect();
        if positions.is_empty() {
            continue;
        }

        let joints_vec: Vec<[u16; 4]> = match primitive.get(&gltf::Semantic::Joints(0)) {
            Some(acc) => read_joints_accessor(acc, buffers).unwrap_or_else(|e| {
                eprintln!(
                    "Warning [{}]: invalid JOINTS_0 accessor ({}), defaulting to joint 0",
                    file_path.display(),
                    e
                );
                vec![[0, 0, 0, 0]; positions.len()]
            }),
            None => vec![[0, 0, 0, 0]; positions.len()],
        };

        let weights_vec: Vec<[f32; 4]> = match primitive.get(&gltf::Semantic::Weights(0)) {
            Some(acc) => read_weights_accessor(acc, buffers).unwrap_or_else(|e| {
                eprintln!(
                    "Warning [{}]: invalid WEIGHTS_0 accessor ({}), defaulting to [1,0,0,0]",
                    file_path.display(),
                    e
                );
                vec![[1.0, 0.0, 0.0, 0.0]; positions.len()]
            }),
            None => vec![[1.0, 0.0, 0.0, 0.0]; positions.len()],
        };

        let mat = primitive.material();
        let pbr = mat.pbr_metallic_roughness();
        let tex_info = pbr.base_color_texture();

        let uv_set = tex_info
            .as_ref()
            .and_then(|info| info.texture_transform().and_then(|t| t.tex_coord()))
            .or_else(|| tex_info.as_ref().map(|info| info.tex_coord()))
            .unwrap_or(0);

        let mut uvs: Vec<[f32; 2]> = match primitive.get(&gltf::Semantic::TexCoords(uv_set)) {
            Some(uv_accessor) => match read_vec2_accessor(uv_accessor, buffers) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!(
                        "Warning [{}]: invalid TEXCOORD_{} accessor ({}), defaulting to [0, 0]",
                        file_path.display(),
                        uv_set,
                        e
                    );
                    vec![[0.0, 0.0]; positions.len()]
                }
            },
            None => vec![[0.0, 0.0]; positions.len()],
        };

        if let Some(tt) = tex_info.as_ref().and_then(|info| info.texture_transform()) {
            let offset = tt.offset();
            let scale = tt.scale();
            let (sin_r, cos_r) = tt.rotation().sin_cos();
            for uv in &mut uvs {
                let su = uv[0] * scale[0];
                let sv = uv[1] * scale[1];
                uv[0] = cos_r * su + sin_r * sv + offset[0];
                uv[1] = -sin_r * su + cos_r * sv + offset[1];
            }
        }

        if let Some(idx_accessor) = primitive.indices() {
            if !validate_accessor_bounds(&idx_accessor, buffers) {
                eprintln!(
                    "Warning [{}]: skipping primitive with out-of-bounds index buffer view",
                    file_path.display()
                );
                continue;
            }
        }

        let reader = primitive.reader(|buf| buffers.get(buf.index()).map(|d| &d[..]));
        let (mut raw_indices, prefer_u32) = match reader.read_indices() {
            Some(gltf::mesh::util::ReadIndices::U8(iter)) => {
                (iter.map(u32::from).collect::<Vec<u32>>(), false)
            }
            Some(gltf::mesh::util::ReadIndices::U16(iter)) => {
                (iter.map(u32::from).collect::<Vec<u32>>(), false)
            }
            Some(gltf::mesh::util::ReadIndices::U32(iter)) => (iter.collect::<Vec<u32>>(), true),
            None => ((0..positions.len() as u32).collect::<Vec<u32>>(), false),
        };

        let tri_len = (raw_indices.len() / 3) * 3;
        raw_indices.truncate(tri_len);
        if raw_indices.is_empty()
            || raw_indices
                .iter()
                .any(|&idx| (idx as usize) >= positions.len())
        {
            eprintln!(
                "Warning [{}]: skipping primitive with invalid triangle indices",
                file_path.display()
            );
            continue;
        }

        if det < 0.0 {
            for tri in raw_indices.chunks_exact_mut(3) {
                tri.swap(1, 2);
            }
        }

        let maybe_normals = primitive
            .get(&gltf::Semantic::Normals)
            .and_then(|norm_acc| match read_vec3_accessor(norm_acc, buffers) {
                Ok(n) => Some(n),
                Err(e) => {
                    eprintln!(
                        "Warning [{}]: invalid NORMAL accessor ({}), computing flat normals",
                        file_path.display(),
                        e
                    );
                    None
                }
            });

        let (vertices, indices) = if let Some(raw_normals) = maybe_normals {
            let normals: Vec<[f32; 3]> = raw_normals
                .into_iter()
                .map(|n| {
                    let v = Vec3::from_array(n);
                    let wn = (adj0 * v.x + adj1 * v.y + adj2 * v.z).normalize_or_zero();
                    if wn.length_squared() > 0.0 {
                        wn.to_array()
                    } else {
                        [0.0, 1.0, 0.0]
                    }
                })
                .collect();

            let mut verts = Vec::with_capacity(positions.len());
            for i in 0..positions.len() {
                let raw_j = joints_vec.get(i).copied().unwrap_or([0, 0, 0, 0]);
                let clamped_j = [
                    raw_j[0].min((MAX_JOINTS - 1) as u16),
                    raw_j[1].min((MAX_JOINTS - 1) as u16),
                    raw_j[2].min((MAX_JOINTS - 1) as u16),
                    raw_j[3].min((MAX_JOINTS - 1) as u16),
                ];
                let norm_w = normalize_weights(
                    weights_vec
                        .get(i)
                        .copied()
                        .unwrap_or([1.0, 0.0, 0.0, 0.0]),
                );
                verts.push(ModelVertex {
                    position: positions[i].to_array(),
                    normal: normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]),
                    uv: uvs.get(i).copied().unwrap_or([0.0, 0.0]),
                    joints: clamped_j,
                    weights: norm_w,
                });
            }

            let idx_data = pack_indices(raw_indices, prefer_u32, verts.len());
            (verts, idx_data)
        } else {
            let mut verts = Vec::with_capacity(raw_indices.len());
            let mut flat_indices = Vec::with_capacity(raw_indices.len());

            for tri in raw_indices.chunks_exact(3) {
                let i0 = tri[0] as usize;
                let i1 = tri[1] as usize;
                let i2 = tri[2] as usize;
                let p0 = positions[i0];
                let p1 = positions[i1];
                let p2 = positions[i2];
                let n = (p1 - p0).cross(p2 - p0).normalize_or_zero();
                let normal = if n.length_squared() > 0.0 {
                    n.to_array()
                } else {
                    [0.0, 1.0, 0.0]
                };

                for &orig_idx in &[i0, i1, i2] {
                    let new_idx = verts.len() as u32;
                    let raw_j = joints_vec.get(orig_idx).copied().unwrap_or([0, 0, 0, 0]);
                    let clamped_j = [
                        raw_j[0].min((MAX_JOINTS - 1) as u16),
                        raw_j[1].min((MAX_JOINTS - 1) as u16),
                        raw_j[2].min((MAX_JOINTS - 1) as u16),
                        raw_j[3].min((MAX_JOINTS - 1) as u16),
                    ];
                    let norm_w = normalize_weights(
                        weights_vec
                            .get(orig_idx)
                            .copied()
                            .unwrap_or([1.0, 0.0, 0.0, 0.0]),
                    );
                    verts.push(ModelVertex {
                        position: positions[orig_idx].to_array(),
                        normal,
                        uv: uvs.get(orig_idx).copied().unwrap_or([0.0, 0.0]),
                        joints: clamped_j,
                        weights: norm_w,
                    });
                    flat_indices.push(new_idx);
                }
            }

            let idx_data = pack_indices(flat_indices, prefer_u32, verts.len());
            (verts, idx_data)
        };

        let base_color_factor = pbr.base_color_factor();
        let base_color_texture = tex_info.and_then(|info| {
            load_texture_image(&info.texture().source(), buffers, base_dir, file_path)
        });

        out_primitives.push(CpuPrimitive {
            vertices,
            indices,
            material: CpuMaterial {
                base_color_factor,
                base_color_texture,
            },
        });
    }
}

fn validate_accessor_bounds(
    accessor: &gltf::Accessor,
    buffers: &[gltf::buffer::Data],
) -> bool {
    if let Some(view) = accessor.view() {
        let Some(buf) = buffers.get(view.buffer().index()) else {
            return false;
        };
        let Some(view_end) = view.offset().checked_add(view.length()) else {
            return false;
        };
        if view_end > buf.len() || accessor.offset() > view.length() {
            return false;
        }
        if accessor.count() > 0 {
            let elem_size = accessor.size();
            let stride = view.stride().unwrap_or(elem_size);
            let span = (accessor.count() - 1)
                .saturating_mul(stride)
                .saturating_add(elem_size);
            if accessor.offset().saturating_add(span) > view.length() {
                return false;
            }
        }
    }
    if let Some(sparse) = accessor.sparse() {
        let idx_view = sparse.indices().view();
        let Some(idx_buf) = buffers.get(idx_view.buffer().index()) else {
            return false;
        };
        if idx_view.offset().saturating_add(idx_view.length()) > idx_buf.len() {
            return false;
        }
        let val_view = sparse.values().view();
        let Some(val_buf) = buffers.get(val_view.buffer().index()) else {
            return false;
        };
        if val_view.offset().saturating_add(val_view.length()) > val_buf.len() {
            return false;
        }
    }
    true
}

#[inline]
fn dequantize_i8(v: i8, normalized: bool) -> f32 {
    if normalized {
        (v as f32 / 127.0).max(-1.0)
    } else {
        v as f32
    }
}

#[inline]
fn dequantize_u8(v: u8, normalized: bool) -> f32 {
    if normalized {
        v as f32 / 255.0
    } else {
        v as f32
    }
}

#[inline]
fn dequantize_i16(v: i16, normalized: bool) -> f32 {
    if normalized {
        (v as f32 / 32767.0).max(-1.0)
    } else {
        v as f32
    }
}

#[inline]
fn dequantize_u16(v: u16, normalized: bool) -> f32 {
    if normalized {
        v as f32 / 65535.0
    } else {
        v as f32
    }
}

fn read_joints_accessor(
    accessor: gltf::Accessor,
    buffers: &[gltf::buffer::Data],
) -> Result<Vec<[u16; 4]>, String> {
    if accessor.dimensions() != Dimensions::Vec4 {
        return Err(format!(
            "expected VEC4 JOINTS_0 accessor, found {:?}",
            accessor.dimensions()
        ));
    }
    if !validate_accessor_bounds(&accessor, buffers) {
        return Err("JOINTS_0 buffer view out of bounds".to_string());
    }
    let get_buf = |buf: gltf::Buffer| buffers.get(buf.index()).map(|d| &d[..]);
    match accessor.data_type() {
        DataType::U8 => {
            let iter = Iter::<[u8; 4]>::new(accessor, get_buf)
                .ok_or_else(|| "failed to create u8 JOINTS_0 iterator".to_string())?;
            Ok(iter
                .map(|j| [u16::from(j[0]), u16::from(j[1]), u16::from(j[2]), u16::from(j[3])])
                .collect())
        }
        DataType::U16 => {
            let iter = Iter::<[u16; 4]>::new(accessor, get_buf)
                .ok_or_else(|| "failed to create u16 JOINTS_0 iterator".to_string())?;
            Ok(iter.collect())
        }
        other => Err(format!("unsupported JOINTS_0 component type {:?}", other)),
    }
}

fn read_weights_accessor(
    accessor: gltf::Accessor,
    buffers: &[gltf::buffer::Data],
) -> Result<Vec<[f32; 4]>, String> {
    if accessor.dimensions() != Dimensions::Vec4 {
        return Err(format!(
            "expected VEC4 WEIGHTS_0 accessor, found {:?}",
            accessor.dimensions()
        ));
    }
    if !validate_accessor_bounds(&accessor, buffers) {
        return Err("WEIGHTS_0 buffer view out of bounds".to_string());
    }
    let normalized = accessor.normalized();
    let get_buf = |buf: gltf::Buffer| buffers.get(buf.index()).map(|d| &d[..]);
    match accessor.data_type() {
        DataType::F32 => {
            let iter = Iter::<[f32; 4]>::new(accessor, get_buf)
                .ok_or_else(|| "failed to create f32 WEIGHTS_0 iterator".to_string())?;
            Ok(iter.collect())
        }
        DataType::U8 => {
            let iter = Iter::<[u8; 4]>::new(accessor, get_buf)
                .ok_or_else(|| "failed to create u8 WEIGHTS_0 iterator".to_string())?;
            Ok(iter
                .map(|w| {
                    [
                        dequantize_u8(w[0], normalized),
                        dequantize_u8(w[1], normalized),
                        dequantize_u8(w[2], normalized),
                        dequantize_u8(w[3], normalized),
                    ]
                })
                .collect())
        }
        DataType::U16 => {
            let iter = Iter::<[u16; 4]>::new(accessor, get_buf)
                .ok_or_else(|| "failed to create u16 WEIGHTS_0 iterator".to_string())?;
            Ok(iter
                .map(|w| {
                    [
                        dequantize_u16(w[0], normalized),
                        dequantize_u16(w[1], normalized),
                        dequantize_u16(w[2], normalized),
                        dequantize_u16(w[3], normalized),
                    ]
                })
                .collect())
        }
        other => Err(format!("unsupported WEIGHTS_0 component type {:?}", other)),
    }
}

fn read_vec3_accessor(
    accessor: gltf::Accessor,
    buffers: &[gltf::buffer::Data],
) -> Result<Vec<[f32; 3]>, String> {
    if accessor.dimensions() != Dimensions::Vec3 {
        return Err(format!(
            "expected VEC3 accessor, found {:?}",
            accessor.dimensions()
        ));
    }
    if !validate_accessor_bounds(&accessor, buffers) {
        return Err("accessor buffer view is out of bounds".to_string());
    }

    let normalized = accessor.normalized();
    let data_type = accessor.data_type();
    let get_buf = |buf: gltf::Buffer| buffers.get(buf.index()).map(|d| &d[..]);

    match data_type {
        DataType::F32 => {
            let iter = Iter::<[f32; 3]>::new(accessor, get_buf)
                .ok_or_else(|| "failed to create f32 VEC3 iterator".to_string())?;
            Ok(iter.collect())
        }
        DataType::I8 => {
            let iter = Iter::<[i8; 3]>::new(accessor, get_buf)
                .ok_or_else(|| "failed to create i8 VEC3 iterator".to_string())?;
            Ok(iter
                .map(|v| {
                    [
                        dequantize_i8(v[0], normalized),
                        dequantize_i8(v[1], normalized),
                        dequantize_i8(v[2], normalized),
                    ]
                })
                .collect())
        }
        DataType::U8 => {
            let iter = Iter::<[u8; 3]>::new(accessor, get_buf)
                .ok_or_else(|| "failed to create u8 VEC3 iterator".to_string())?;
            Ok(iter
                .map(|v| {
                    [
                        dequantize_u8(v[0], normalized),
                        dequantize_u8(v[1], normalized),
                        dequantize_u8(v[2], normalized),
                    ]
                })
                .collect())
        }
        DataType::I16 => {
            let iter = Iter::<[i16; 3]>::new(accessor, get_buf)
                .ok_or_else(|| "failed to create i16 VEC3 iterator".to_string())?;
            Ok(iter
                .map(|v| {
                    [
                        dequantize_i16(v[0], normalized),
                        dequantize_i16(v[1], normalized),
                        dequantize_i16(v[2], normalized),
                    ]
                })
                .collect())
        }
        DataType::U16 => {
            let iter = Iter::<[u16; 3]>::new(accessor, get_buf)
                .ok_or_else(|| "failed to create u16 VEC3 iterator".to_string())?;
            Ok(iter
                .map(|v| {
                    [
                        dequantize_u16(v[0], normalized),
                        dequantize_u16(v[1], normalized),
                        dequantize_u16(v[2], normalized),
                    ]
                })
                .collect())
        }
        other => Err(format!("unsupported VEC3 component type {:?}", other)),
    }
}

fn read_vec2_accessor(
    accessor: gltf::Accessor,
    buffers: &[gltf::buffer::Data],
) -> Result<Vec<[f32; 2]>, String> {
    if accessor.dimensions() != Dimensions::Vec2 {
        return Err(format!(
            "expected VEC2 accessor, found {:?}",
            accessor.dimensions()
        ));
    }
    if !validate_accessor_bounds(&accessor, buffers) {
        return Err("accessor buffer view is out of bounds".to_string());
    }

    let normalized = accessor.normalized();
    let data_type = accessor.data_type();
    let get_buf = |buf: gltf::Buffer| buffers.get(buf.index()).map(|d| &d[..]);

    match data_type {
        DataType::F32 => {
            let iter = Iter::<[f32; 2]>::new(accessor, get_buf)
                .ok_or_else(|| "failed to create f32 VEC2 iterator".to_string())?;
            Ok(iter.collect())
        }
        DataType::I8 => {
            let iter = Iter::<[i8; 2]>::new(accessor, get_buf)
                .ok_or_else(|| "failed to create i8 VEC2 iterator".to_string())?;
            Ok(iter
                .map(|v| [dequantize_i8(v[0], normalized), dequantize_i8(v[1], normalized)])
                .collect())
        }
        DataType::U8 => {
            let iter = Iter::<[u8; 2]>::new(accessor, get_buf)
                .ok_or_else(|| "failed to create u8 VEC2 iterator".to_string())?;
            Ok(iter
                .map(|v| [dequantize_u8(v[0], normalized), dequantize_u8(v[1], normalized)])
                .collect())
        }
        DataType::I16 => {
            let iter = Iter::<[i16; 2]>::new(accessor, get_buf)
                .ok_or_else(|| "failed to create i16 VEC2 iterator".to_string())?;
            Ok(iter
                .map(|v| {
                    [
                        dequantize_i16(v[0], normalized),
                        dequantize_i16(v[1], normalized),
                    ]
                })
                .collect())
        }
        DataType::U16 => {
            let iter = Iter::<[u16; 2]>::new(accessor, get_buf)
                .ok_or_else(|| "failed to create u16 VEC2 iterator".to_string())?;
            Ok(iter
                .map(|v| {
                    [
                        dequantize_u16(v[0], normalized),
                        dequantize_u16(v[1], normalized),
                    ]
                })
                .collect())
        }
        other => Err(format!("unsupported VEC2 component type {:?}", other)),
    }
}

fn pack_indices(raw: Vec<u32>, prefer_u32: bool, vertex_count: usize) -> IndexBufferData {
    if !prefer_u32 && vertex_count <= u16::MAX as usize {
        IndexBufferData::U16(raw.into_iter().map(|i| i as u16).collect())
    } else {
        IndexBufferData::U32(raw)
    }
}

fn load_texture_image(
    img: &gltf::Image,
    buffers: &[gltf::buffer::Data],
    base_dir: Option<&Path>,
    file_path: &Path,
) -> Option<CpuTexture> {
    match img.source() {
        gltf::image::Source::View { view, .. } => {
            let buf = buffers.get(view.buffer().index())?;
            let start = view.offset();
            let end = start.saturating_add(view.length());
            if end > buf.len() {
                eprintln!(
                    "Warning [{}]: embedded image buffer view out of bounds; using base color factor only",
                    file_path.display()
                );
                return None;
            }
            decode_image_bytes(&buf[start..end], file_path)
        }
        gltf::image::Source::Uri { uri, .. } => {
            let resolved: PathBuf = match base_dir {
                Some(dir) => dir.join(uri),
                None => PathBuf::from(uri),
            };
            match std::fs::read(&resolved) {
                Ok(bytes) => decode_image_bytes(&bytes, file_path),
                Err(e) => {
                    eprintln!(
                        "Warning [{}]: missing texture '{}': {}. Using base color factor only.",
                        file_path.display(),
                        resolved.display(),
                        e
                    );
                    None
                }
            }
        }
    }
}

fn decode_image_bytes(bytes: &[u8], file_path: &Path) -> Option<CpuTexture> {
    match image::load_from_memory(bytes) {
        Ok(decoded) => {
            let rgba = decoded.to_rgba8();
            Some(CpuTexture {
                width: rgba.width(),
                height: rgba.height(),
                rgba8: rgba.into_raw(),
            })
        }
        Err(e) => {
            eprintln!(
                "Warning [{}]: failed to decode PNG/JPEG texture: {}. Using base color factor only.",
                file_path.display(),
                e
            );
            None
        }
    }
}
