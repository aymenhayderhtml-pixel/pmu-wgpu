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
@group(0) @binding(1)
var shadow_map: texture_depth_2d;
@group(0) @binding(2)
var shadow_sampler: sampler_comparison;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) model_0: vec4<f32>,
    @location(3) model_1: vec4<f32>,
    @location(4) model_2: vec4<f32>,
    @location(5) model_3: vec4<f32>,
    @location(6) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    let model = mat4x4<f32>(in.model_0, in.model_1, in.model_2, in.model_3);
    let world_pos4 = model * vec4<f32>(in.position, 1.0);

    let c0 = in.model_0.xyz;
    let c1 = in.model_1.xyz;
    let c2 = in.model_2.xyz;
    let adj_t = mat3x3<f32>(
        cross(c1, c2),
        cross(c2, c0),
        cross(c0, c1),
    );

    var out: VertexOutput;
    out.clip_position = scene.view_proj * world_pos4;
    out.world_pos = world_pos4.xyz;
    out.world_normal = normalize(adj_t * in.normal);
    out.color = in.color;
    return out;
}

fn compute_shadow(world_pos: vec3<f32>, normal: vec3<f32>) -> f32 {
    if scene.shadow_params.x < 0.5 {
        return 1.0;
    }

    let normal_bias = scene.shadow_params.z;
    let biased_pos = world_pos + normal * normal_bias;
    let light_clip = scene.light_view_proj * vec4<f32>(biased_pos, 1.0);
    let ndc = light_clip.xyz / light_clip.w;

    let shadow_uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let depth_ref = clamp(ndc.z, 0.0, 1.0);
    let texel_size = scene.shadow_params.y;

    var pcf_sum: f32 = 0.0;
    for (var y: i32 = -1; y <= 1; y = y + 1) {
        for (var x: i32 = -1; x <= 1; x = x + 1) {
            let offset = vec2<f32>(f32(x), f32(y)) * texel_size;
            pcf_sum = pcf_sum + textureSampleCompare(
                shadow_map,
                shadow_sampler,
                shadow_uv + offset,
                depth_ref
            );
        }
    }
    let raw_shadow = pcf_sum / 9.0;

    let box_dist = max(abs(ndc.x), abs(ndc.y));
    let xy_fade = 1.0 - smoothstep(0.82, 0.98, box_dist);
    let z_fade = 1.0 - smoothstep(0.90, 0.99, abs(ndc.z * 2.0 - 1.0));
    return mix(1.0, raw_shadow, xy_fade * z_fade);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let normal = normalize(in.world_normal);
    let light_dir = normalize(scene.light_dir.xyz);

    let hemi_t = clamp(normal.y * 0.5 + 0.5, 0.0, 1.0);
    let sky_amb = mix(scene.sky_horizon.rgb, scene.sky_zenith.rgb, 0.35);
    let ambient_rgb = mix(scene.ground_ambient.rgb, sky_amb, hemi_t) * scene.light_color.w;

    let ndotl = max(dot(normal, light_dir), 0.0);
    let shadow = compute_shadow(in.world_pos, normal);
    let diffuse_strength = scene.sky_horizon.w;
    let diffuse_rgb = (diffuse_strength * ndotl * shadow) * scene.light_color.rgb;

    let flash = clamp(in.color.w - 1.0, 0.0, 1.0);
    let lit_color = mix(in.color.rgb * (ambient_rgb + diffuse_rgb), vec3<f32>(1.0, 0.32, 0.28), flash);

    let dist = distance(scene.camera_pos.xyz, in.world_pos);
    let fog_start = scene.fog_params.x;
    let fog_end = scene.fog_params.y;
    let fog_factor = clamp((dist - fog_start) / (fog_end - fog_start), 0.0, 1.0);

    let final_rgb = mix(lit_color, scene.sky_horizon.rgb, fog_factor);
    return vec4<f32>(final_rgb, 1.0);
}
