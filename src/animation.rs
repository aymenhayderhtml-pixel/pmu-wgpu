use glam::{Mat4, Quat, Vec3};

pub const MAX_JOINTS: usize = 128;

#[derive(Copy, Clone, Debug)]
pub struct LocalTrs {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for LocalTrs {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

impl LocalTrs {
    pub fn to_mat4(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }

    pub fn blend(self, other: Self, weight: f32) -> Self {
        let t = weight.clamp(0.0, 1.0);
        Self {
            translation: self.translation.lerp(other.translation, t),
            rotation: slerp_shortest(self.rotation, other.rotation, t),
            scale: self.scale.lerp(other.scale, t),
        }
    }
}

#[derive(Clone, Debug)]
pub struct NodeDef {
    pub parent: Option<usize>,
    pub base_trs: LocalTrs,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InterpMode {
    Linear,
    Step,
}

#[derive(Clone, Debug)]
pub enum ChannelValues {
    Translations(Vec<Vec3>),
    Rotations(Vec<Quat>),
    Scales(Vec<Vec3>),
}

#[derive(Clone, Debug)]
pub struct AnimChannel {
    pub node_index: usize,
    pub interpolation: InterpMode,
    pub times: Vec<f32>,
    pub values: ChannelValues,
}

#[derive(Clone, Debug)]
pub struct AnimationClip {
    pub name: String,
    pub duration: f32,
    pub channels: Vec<AnimChannel>,
}

#[derive(Clone, Debug)]
pub struct Skeleton {
    pub nodes: Vec<NodeDef>,
    pub eval_order: Vec<usize>,
    pub joint_nodes: Vec<usize>,
    pub inverse_bind_matrices: Vec<Mat4>,
    pub root_correction: Mat4,
}

impl Skeleton {
    pub fn new(
        nodes: Vec<NodeDef>,
        joint_nodes: Vec<usize>,
        inverse_bind_matrices: Vec<Mat4>,
        root_correction: Mat4,
    ) -> Self {
        let eval_order = build_topological_order(&nodes);
        Self {
            nodes,
            eval_order,
            joint_nodes,
            inverse_bind_matrices,
            root_correction,
        }
    }

    pub fn joint_count(&self) -> usize {
        self.joint_nodes.len()
    }
}

/// Pre-sized scratch buffers for evaluating node and joint matrices with zero per-frame allocations.
pub struct PoseScratch {
    pub local_a: Vec<LocalTrs>,
    pub local_b: Vec<LocalTrs>,
    pub node_world: Vec<Mat4>,
    pub joint_matrices: Vec<[[f32; 4]; 4]>,
    pub bone_segments: Vec<(Vec3, Vec3)>,
}

impl PoseScratch {
    pub fn new(node_count: usize) -> Self {
        let mut joint_matrices = vec![Mat4::IDENTITY.to_cols_array_2d(); MAX_JOINTS];
        joint_matrices.truncate(MAX_JOINTS);
        Self {
            local_a: vec![LocalTrs::default(); node_count],
            local_b: vec![LocalTrs::default(); node_count],
            node_world: vec![Mat4::IDENTITY; node_count],
            joint_matrices,
            bone_segments: Vec::with_capacity(MAX_JOINTS),
        }
    }

    pub fn ensure_capacity(&mut self, node_count: usize) {
        if self.local_a.len() < node_count {
            self.local_a.resize(node_count, LocalTrs::default());
            self.local_b.resize(node_count, LocalTrs::default());
            self.node_world.resize(node_count, Mat4::IDENTITY);
        }
    }
}

/// Normalizes 4 vertex skin weights so they sum to 1.0 (falls back to `[1, 0, 0, 0]` if all zero).
pub fn normalize_weights(w: [f32; 4]) -> [f32; 4] {
    let clamped = [
        w[0].max(0.0),
        w[1].max(0.0),
        w[2].max(0.0),
        w[3].max(0.0),
    ];
    let sum = clamped[0] + clamped[1] + clamped[2] + clamped[3];
    if sum > 1e-6 {
        let inv = 1.0 / sum;
        [
            clamped[0] * inv,
            clamped[1] * inv,
            clamped[2] * inv,
            clamped[3] * inv,
        ]
    } else {
        [1.0, 0.0, 0.0, 0.0]
    }
}

/// Shortest-path normalized quaternion slerp.
pub fn slerp_shortest(a: Quat, b: Quat, t: f32) -> Quat {
    let q1 = a.normalize();
    let mut q2 = b.normalize();
    if q1.dot(q2) < 0.0 {
        q2 = -q2;
    }
    q1.slerp(q2, t.clamp(0.0, 1.0)).normalize()
}

/// Wraps or clamps time `t` against clip `duration`.
pub fn resolve_clip_time(t: f32, duration: f32, looping: bool) -> f32 {
    if duration <= 1e-6 {
        return 0.0;
    }
    if looping {
        t.rem_euclid(duration)
    } else {
        t.clamp(0.0, duration)
    }
}

/// Binary searches sorted `times` to find `(idx0, idx1, alpha)` for sample time `t`.
pub fn find_keyframe_span(times: &[f32], t: f32) -> (usize, usize, f32) {
    if times.is_empty() {
        return (0, 0, 0.0);
    }
    if times.len() == 1 || t <= times[0] {
        return (0, 0, 0.0);
    }
    let last = times.len() - 1;
    if t >= times[last] {
        return (last, last, 0.0);
    }

    let idx1 = match times.binary_search_by(|probe| probe.total_cmp(&t)) {
        Ok(exact) => return (exact, exact, 0.0),
        Err(insert_pos) => insert_pos,
    };
    let idx0 = idx1 - 1;
    let t0 = times[idx0];
    let t1 = times[idx1];
    let span = t1 - t0;
    let alpha = if span > 1e-6 {
        ((t - t0) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (idx0, idx1, alpha)
}

/// Samples a single animation clip at `sample_time` into `out_trs` (initialized from `skeleton.nodes`).
pub fn sample_clip_into(
    skeleton: &Skeleton,
    clip: &AnimationClip,
    sample_time: f32,
    looping: bool,
    out_trs: &mut [LocalTrs],
) {
    for (i, node) in skeleton.nodes.iter().enumerate() {
        out_trs[i] = node.base_trs;
    }

    let t = resolve_clip_time(sample_time, clip.duration, looping);

    for channel in &clip.channels {
        if channel.node_index >= out_trs.len() || channel.times.is_empty() {
            continue;
        }
        let (i0, i1, alpha) = find_keyframe_span(&channel.times, t);
        let eff_alpha = match channel.interpolation {
            InterpMode::Step => 0.0,
            InterpMode::Linear => alpha,
        };

        let trs = &mut out_trs[channel.node_index];
        match &channel.values {
            ChannelValues::Translations(vals) => {
                if i1 < vals.len() {
                    trs.translation = if eff_alpha == 0.0 {
                        vals[i0]
                    } else {
                        vals[i0].lerp(vals[i1], eff_alpha)
                    };
                }
            }
            ChannelValues::Rotations(vals) => {
                if i1 < vals.len() {
                    trs.rotation = if eff_alpha == 0.0 {
                        vals[i0].normalize()
                    } else {
                        slerp_shortest(vals[i0], vals[i1], eff_alpha)
                    };
                }
            }
            ChannelValues::Scales(vals) => {
                if i1 < vals.len() {
                    trs.scale = if eff_alpha == 0.0 {
                        vals[i0]
                    } else {
                        vals[i0].lerp(vals[i1], eff_alpha)
                    };
                }
            }
        }
    }
}

/// Evaluates world matrices through the node hierarchy and computes final GPU joint matrices
/// and debug bone line segments in model space.
pub fn compute_hierarchy_and_joints(
    skeleton: &Skeleton,
    scratch: &mut PoseScratch,
) {
    let n = skeleton.nodes.len();
    for &node_idx in &skeleton.eval_order {
        if node_idx >= n {
            continue;
        }
        let local_mat = scratch.local_a[node_idx].to_mat4();
        scratch.node_world[node_idx] = match skeleton.nodes[node_idx].parent {
            Some(parent_idx) if parent_idx < n => scratch.node_world[parent_idx] * local_mat,
            _ => skeleton.root_correction * local_mat,
        };
    }

    let joint_count = skeleton.joint_nodes.len().min(MAX_JOINTS);
    for j in 0..joint_count {
        let node_idx = skeleton.joint_nodes[j];
        let world_mat = if node_idx < n {
            scratch.node_world[node_idx]
        } else {
            Mat4::IDENTITY
        };
        let ibm = skeleton
            .inverse_bind_matrices
            .get(j)
            .copied()
            .unwrap_or(Mat4::IDENTITY);
        scratch.joint_matrices[j] = (world_mat * ibm).to_cols_array_2d();
    }

    // Build debug skeleton bone segments in model space
    scratch.bone_segments.clear();
    for &node_idx in &skeleton.joint_nodes {
        if node_idx >= n {
            continue;
        }
        if let Some(parent_idx) = skeleton.nodes[node_idx].parent {
            if parent_idx < n {
                let p0 = scratch.node_world[parent_idx].w_axis.truncate();
                let p1 = scratch.node_world[node_idx].w_axis.truncate();
                if p0.distance_squared(p1) > 1e-6 {
                    scratch.bone_segments.push((p0, p1));
                }
            }
        }
    }
}

fn build_topological_order(nodes: &[NodeDef]) -> Vec<usize> {
    let n = nodes.len();
    let mut visited = vec![false; n];
    let mut order = Vec::with_capacity(n);

    fn visit(idx: usize, nodes: &[NodeDef], visited: &mut [bool], order: &mut Vec<usize>) {
        if idx >= nodes.len() || visited[idx] {
            return;
        }
        if let Some(p) = nodes[idx].parent {
            if p < nodes.len() && p != idx {
                visit(p, nodes, visited, order);
            }
        }
        visited[idx] = true;
        order.push(idx);
    }

    for i in 0..n {
        visit(i, nodes, &mut visited, &mut order);
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_weight_normalization() {
        let w = normalize_weights([2.0, 2.0, 0.0, 0.0]);
        assert!((w[0] - 0.5).abs() < 1e-5);
        assert!((w[1] - 0.5).abs() < 1e-5);
        assert!((w[0] + w[1] + w[2] + w[3] - 1.0).abs() < 1e-5);

        let zero_fallback = normalize_weights([0.0, 0.0, 0.0, 0.0]);
        assert_eq!(zero_fallback, [1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_sampler_lerp_slerp_shortest_and_step() {
        // Shortest-path slerp between q and -q (representing the same rotation) stays constant
        let q_a = Quat::from_rotation_y(0.5);
        let q_b = -q_a;
        let mid = slerp_shortest(q_a, q_b, 0.5);
        assert!(mid.dot(q_a).abs() > 0.9999);

        let skeleton = Skeleton::new(
            vec![NodeDef {
                parent: None,
                base_trs: LocalTrs::default(),
            }],
            vec![0],
            vec![Mat4::IDENTITY],
            Mat4::IDENTITY,
        );

        let linear_clip = AnimationClip {
            name: "move".to_string(),
            duration: 2.0,
            channels: vec![AnimChannel {
                node_index: 0,
                interpolation: InterpMode::Linear,
                times: vec![0.0, 2.0],
                values: ChannelValues::Translations(vec![
                    Vec3::new(0.0, 0.0, 0.0),
                    Vec3::new(10.0, 0.0, 0.0),
                ]),
            }],
        };

        let mut out = vec![LocalTrs::default(); 1];
        sample_clip_into(&skeleton, &linear_clip, 0.5, false, &mut out);
        assert!((out[0].translation.x - 2.5).abs() < 1e-5);

        // STEP interpolation holds the first value until the next keyframe time
        let step_clip = AnimationClip {
            name: "step_move".to_string(),
            duration: 2.0,
            channels: vec![AnimChannel {
                node_index: 0,
                interpolation: InterpMode::Step,
                times: vec![0.0, 1.0, 2.0],
                values: ChannelValues::Translations(vec![
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(5.0, 0.0, 0.0),
                    Vec3::new(9.0, 0.0, 0.0),
                ]),
            }],
        };
        sample_clip_into(&skeleton, &step_clip, 0.9, false, &mut out);
        assert!((out[0].translation.x - 1.0).abs() < 1e-5);
        sample_clip_into(&skeleton, &step_clip, 1.5, false, &mut out);
        assert!((out[0].translation.x - 5.0).abs() < 1e-5);
    }

    #[test]
    fn test_looping_and_clamping_at_clip_end() {
        assert!((resolve_clip_time(2.5, 2.0, true) - 0.5).abs() < 1e-5);
        assert!((resolve_clip_time(2.5, 2.0, false) - 2.0).abs() < 1e-5);
    }

    #[test]
    fn test_three_joint_chain_world_composition_and_crossfade() {
        // 3-joint vertical chain: root at y=1, child 1 offset +1y, child 2 offset +1y
        let nodes = vec![
            NodeDef {
                parent: None,
                base_trs: LocalTrs {
                    translation: Vec3::new(0.0, 1.0, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            },
            NodeDef {
                parent: Some(0),
                base_trs: LocalTrs {
                    translation: Vec3::new(0.0, 1.0, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            },
            NodeDef {
                parent: Some(1),
                base_trs: LocalTrs {
                    translation: Vec3::new(0.0, 1.0, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            },
        ];

        let skeleton = Skeleton::new(
            nodes,
            vec![0, 1, 2],
            vec![Mat4::IDENTITY; 3],
            Mat4::IDENTITY,
        );
        let mut scratch = PoseScratch::new(3);

        // Cross-fade two local poses at weight 0.25
        let pose_a = [
            skeleton.nodes[0].base_trs,
            skeleton.nodes[1].base_trs,
            skeleton.nodes[2].base_trs,
        ];
        let mut pose_b = pose_a;
        pose_b[0].translation = Vec3::new(4.0, 1.0, 0.0);

        for i in 0..3 {
            scratch.local_a[i] = pose_a[i].blend(pose_b[i], 0.25);
        }
        compute_hierarchy_and_joints(&skeleton, &mut scratch);

        // Joint 2 world position should be x = 1.0 (25% of 4.0) and y = 1 + 1 + 1 = 3.0
        let j2_pos = scratch.node_world[2].w_axis.truncate();
        assert!((j2_pos.x - 1.0).abs() < 1e-5);
        assert!((j2_pos.y - 3.0).abs() < 1e-5);
    }
}
