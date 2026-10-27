use crate::model::CpuTexture;

/// Generates a full mipmap chain on the CPU at load time using sRGB-aware 2x2 box filtering
/// and uploads all mip levels to a `wgpu::TextureFormat::Rgba8UnormSrgb` GPU texture.
///
/// Justification for CPU-side sRGB-aware mipmap generation at load time:
/// 1. It avoids adding `RENDER_ATTACHMENT` usage to every model texture and avoids creating
///    per-mip transient `TextureView`s, `BindGroup`s, and render passes during startup.
/// 2. Converting sRGB texels to linear light before 2x2 averaging and re-encoding to sRGB
///    prevents gamma-darkening at distant mip levels and runs once at asset load time.
pub fn upload_srgb_texture_with_mipmaps(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    tex: &CpuTexture,
    label: &str,
) -> wgpu::TextureView {
    let base_w = tex.width.max(1);
    let base_h = tex.height.max(1);
    let mip_level_count = base_w.max(base_h).ilog2() + 1;

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: base_w,
            height: base_h,
            depth_or_array_layers: 1,
        },
        mip_level_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    write_mip_level(queue, &texture, 0, base_w, base_h, &tex.rgba8);

    let mut prev_w = base_w;
    let mut prev_h = base_h;
    let mut prev_data: Vec<u8> = Vec::new();

    for level in 1..mip_level_count {
        let src_slice: &[u8] = if level == 1 { &tex.rgba8 } else { &prev_data };
        let next_w = (prev_w / 2).max(1);
        let next_h = (prev_h / 2).max(1);
        let next_data = downsample_srgb_2x2(src_slice, prev_w, prev_h, next_w, next_h);

        write_mip_level(queue, &texture, level, next_w, next_h, &next_data);

        prev_w = next_w;
        prev_h = next_h;
        prev_data = next_data;
    }

    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

fn write_mip_level(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    mip_level: u32,
    width: u32,
    height: u32,
    rgba8: &[u8],
) {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba8,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * width),
            rows_per_image: Some(height),
        },
        size,
    );
}

fn downsample_srgb_2x2(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
) -> Vec<u8> {
    let mut dst = vec![0u8; (dst_w * dst_h * 4) as usize];

    for y in 0..dst_h {
        let sy0 = (y * 2).min(src_h - 1);
        let sy1 = (y * 2 + 1).min(src_h - 1);
        for x in 0..dst_w {
            let sx0 = (x * 2).min(src_w - 1);
            let sx1 = (x * 2 + 1).min(src_w - 1);

            let p00 = sample_rgba(src, src_w, sx0, sy0);
            let p10 = sample_rgba(src, src_w, sx1, sy0);
            let p01 = sample_rgba(src, src_w, sx0, sy1);
            let p11 = sample_rgba(src, src_w, sx1, sy1);

            let r_lin = 0.25
                * (srgb_to_linear(p00[0])
                    + srgb_to_linear(p10[0])
                    + srgb_to_linear(p01[0])
                    + srgb_to_linear(p11[0]));
            let g_lin = 0.25
                * (srgb_to_linear(p00[1])
                    + srgb_to_linear(p10[1])
                    + srgb_to_linear(p01[1])
                    + srgb_to_linear(p11[1]));
            let b_lin = 0.25
                * (srgb_to_linear(p00[2])
                    + srgb_to_linear(p10[2])
                    + srgb_to_linear(p01[2])
                    + srgb_to_linear(p11[2]));
            let a = ((u16::from(p00[3])
                + u16::from(p10[3])
                + u16::from(p01[3])
                + u16::from(p11[3])
                + 2)
                / 4) as u8;

            let out_idx = ((y * dst_w + x) * 4) as usize;
            dst[out_idx] = linear_to_srgb(r_lin);
            dst[out_idx + 1] = linear_to_srgb(g_lin);
            dst[out_idx + 2] = linear_to_srgb(b_lin);
            dst[out_idx + 3] = a;
        }
    }

    dst
}

#[inline]
fn sample_rgba(src: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * width + x) * 4) as usize;
    [src[idx], src[idx + 1], src[idx + 2], src[idx + 3]]
}

#[inline]
fn srgb_to_linear(c: u8) -> f32 {
    let v = f32::from(c) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

#[inline]
fn linear_to_srgb(l: f32) -> u8 {
    let c = l.clamp(0.0, 1.0);
    let s = if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0 + 0.5) as u8
}
