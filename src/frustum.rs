use glam::{Mat4, Vec3, Vec4};

pub struct Frustum {
    planes: [Vec4; 6],
}

impl Frustum {
    /// Extracts the 6 inward-facing frustum planes from a right-handed `[0, 1]` view-projection matrix.
    pub fn from_view_proj(view_proj: Mat4) -> Self {
        let r0 = view_proj.row(0);
        let r1 = view_proj.row(1);
        let r2 = view_proj.row(2);
        let r3 = view_proj.row(3);

        Self {
            planes: [
                normalize_plane(r3 + r0), // Left
                normalize_plane(r3 - r0), // Right
                normalize_plane(r3 + r1), // Bottom
                normalize_plane(r3 - r1), // Top
                normalize_plane(r2),      // Near (z_clip >= 0)
                normalize_plane(r3 - r2), // Far  (w_clip - z_clip >= 0)
            ],
        }
    }

    /// Tests a local-space AABB transformed by `model` against the 6 frustum planes.
    pub fn intersects_local_aabb(&self, model: Mat4, local_min: Vec3, local_max: Vec3) -> bool {
        let local_center = (local_min + local_max) * 0.5;
        let local_extents = (local_max - local_min) * 0.5;
        let world_center = model.transform_point3(local_center);

        let col0 = model.x_axis.truncate().abs();
        let col1 = model.y_axis.truncate().abs();
        let col2 = model.z_axis.truncate().abs();
        let world_extents =
            col0 * local_extents.x + col1 * local_extents.y + col2 * local_extents.z;

        for plane in &self.planes {
            let normal = plane.truncate();
            let radius = world_extents.dot(normal.abs());
            let signed_dist = normal.dot(world_center) + plane.w;
            if signed_dist < -radius {
                return false;
            }
        }
        true
    }
}

fn normalize_plane(plane: Vec4) -> Vec4 {
    let len = plane.truncate().length();
    if len > 1e-6 {
        plane / len
    } else {
        plane
    }
}
