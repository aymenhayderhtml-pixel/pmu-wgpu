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

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ray_dir: vec3<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    let ndc = vec2<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0);

    let clip_far = vec4<f32>(ndc, 1.0, 1.0);
    let world_far = scene.inv_view_proj * clip_far;
    let world_pos = world_far.xyz / world_far.w;

    var out: VsOut;
    out.clip_position = clip_far;
    out.ray_dir = world_pos - scene.camera_pos.xyz;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dir = normalize(in.ray_dir);
    let zenith_t = pow(clamp(dir.y, 0.0, 1.0), 0.62);
    var sky_rgb = mix(scene.sky_horizon.rgb, scene.sky_zenith.rgb, zenith_t);

    let sun_dir = normalize(scene.light_dir.xyz);
    let sun_cos = max(dot(dir, sun_dir), 0.0);
    let sun_glow = pow(sun_cos, 48.0) * 0.30;
    let sun_disc = smoothstep(0.9982, 0.9995, sun_cos) * 1.35;
    let above_horizon = clamp(dir.y * 4.0 + 0.2, 0.0, 1.0);

    sky_rgb = sky_rgb + scene.light_color.rgb * (sun_glow + sun_disc) * above_horizon;
    return vec4<f32>(sky_rgb, 1.0);
}
