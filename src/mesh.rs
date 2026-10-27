use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
}

/// Unit 1x1x1 cube centered at the origin, CCW front-face winding in RH (+Y up).
pub fn cube_mesh() -> MeshData {
    let p = 0.5_f32;
    let vertices = vec![
        // +Y (Top)
        Vertex { position: [-p,  p,  p], normal: [ 0.0,  1.0,  0.0] },
        Vertex { position: [ p,  p,  p], normal: [ 0.0,  1.0,  0.0] },
        Vertex { position: [ p,  p, -p], normal: [ 0.0,  1.0,  0.0] },
        Vertex { position: [-p,  p, -p], normal: [ 0.0,  1.0,  0.0] },
        // -Y (Bottom)
        Vertex { position: [-p, -p, -p], normal: [ 0.0, -1.0,  0.0] },
        Vertex { position: [ p, -p, -p], normal: [ 0.0, -1.0,  0.0] },
        Vertex { position: [ p, -p,  p], normal: [ 0.0, -1.0,  0.0] },
        Vertex { position: [-p, -p,  p], normal: [ 0.0, -1.0,  0.0] },
        // +Z (Front / South)
        Vertex { position: [-p, -p,  p], normal: [ 0.0,  0.0,  1.0] },
        Vertex { position: [ p, -p,  p], normal: [ 0.0,  0.0,  1.0] },
        Vertex { position: [ p,  p,  p], normal: [ 0.0,  0.0,  1.0] },
        Vertex { position: [-p,  p,  p], normal: [ 0.0,  0.0,  1.0] },
        // -Z (Back / North)
        Vertex { position: [ p, -p, -p], normal: [ 0.0,  0.0, -1.0] },
        Vertex { position: [-p, -p, -p], normal: [ 0.0,  0.0, -1.0] },
        Vertex { position: [-p,  p, -p], normal: [ 0.0,  0.0, -1.0] },
        Vertex { position: [ p,  p, -p], normal: [ 0.0,  0.0, -1.0] },
        // +X (Right / East)
        Vertex { position: [ p, -p,  p], normal: [ 1.0,  0.0,  0.0] },
        Vertex { position: [ p, -p, -p], normal: [ 1.0,  0.0,  0.0] },
        Vertex { position: [ p,  p, -p], normal: [ 1.0,  0.0,  0.0] },
        Vertex { position: [ p,  p,  p], normal: [ 1.0,  0.0,  0.0] },
        // -X (Left / West)
        Vertex { position: [-p, -p, -p], normal: [-1.0,  0.0,  0.0] },
        Vertex { position: [-p, -p,  p], normal: [-1.0,  0.0,  0.0] },
        Vertex { position: [-p,  p,  p], normal: [-1.0,  0.0,  0.0] },
        Vertex { position: [-p,  p, -p], normal: [-1.0,  0.0,  0.0] },
    ];

    let mut indices = Vec::with_capacity(36);
    for face in 0..6u16 {
        let base = face * 4;
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    MeshData { vertices, indices }
}

/// Horizontal ground quad on Y = 0 of total side length `size` (-size/2 .. +size/2).
pub fn ground_plane_mesh(size: f32) -> MeshData {
    let h = size * 0.5;
    let vertices = vec![
        Vertex { position: [-h, 0.0,  h], normal: [0.0, 1.0, 0.0] },
        Vertex { position: [ h, 0.0,  h], normal: [0.0, 1.0, 0.0] },
        Vertex { position: [ h, 0.0, -h], normal: [0.0, 1.0, 0.0] },
        Vertex { position: [-h, 0.0, -h], normal: [0.0, 1.0, 0.0] },
    ];
    let indices = vec![0, 1, 2, 0, 2, 3];
    MeshData { vertices, indices }
}
