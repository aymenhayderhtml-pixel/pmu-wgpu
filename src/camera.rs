use crate::collision::CollisionGrid;
use crate::input::Input;
use glam::{Mat4, Vec3};
use winit::keyboard::KeyCode;

pub const CAMERA_DISTANCE: f32 = 8.5;
pub const CAMERA_MIN_DISTANCE: f32 = 0.6;
pub const CAMERA_HEIGHT_OFFSET: f32 = 1.75;
pub const CAMERA_SENSITIVITY: f32 = 2.2;
pub const MOUSE_SENSITIVITY: f32 = 0.0028;
pub const CAMERA_FOLLOW_SPEED: f32 = 14.0;
pub const CAMERA_RECOVERY_SPEED: f32 = 10.0;
pub const CAMERA_COLLISION_RADIUS: f32 = 0.20;
pub const CAMERA_COLLISION_MARGIN: f32 = 0.15;

pub const PITCH_MIN: f32 = -80.0_f32.to_radians();
pub const PITCH_MAX: f32 = 80.0_f32.to_radians();
pub const FOV_Y_RADIANS: f32 = 70.0_f32.to_radians();
pub const Z_NEAR: f32 = 0.1;
pub const Z_FAR: f32 = 200.0;

pub const MAIN_MENU_ORBIT_RADIUS: f32 = 30.0;
pub const MAIN_MENU_ORBIT_HEIGHT: f32 = 14.0;
pub const MAIN_MENU_ORBIT_SPEED: f32 = 0.08;

pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub focus: Vec3,
    pub current_dist: f32,
    pub fov_y_radians: f32,
    pub mouse_sens_mult: f32,
    pub menu_orbit_angle: f32,
    pub in_main_menu_orbit: bool,
    prev_yaw: f32,
    prev_pitch: f32,
    prev_focus: Vec3,
    prev_dist: f32,
    aspect: f32,
}

impl Camera {
    pub fn new(initial_target: Vec3, aspect: f32) -> Self {
        let initial_pitch = 20.0_f32.to_radians();
        Self {
            yaw: 0.0,
            pitch: initial_pitch,
            focus: initial_target,
            current_dist: CAMERA_DISTANCE,
            fov_y_radians: FOV_Y_RADIANS,
            mouse_sens_mult: 1.0,
            menu_orbit_angle: 0.0,
            in_main_menu_orbit: false,
            prev_yaw: 0.0,
            prev_pitch: initial_pitch,
            prev_focus: initial_target,
            prev_dist: CAMERA_DISTANCE,
            aspect: aspect.max(0.01),
        }
    }

    pub fn update_main_menu_orbit(&mut self, dt: f32) {
        self.in_main_menu_orbit = true;
        self.menu_orbit_angle =
            (self.menu_orbit_angle + MAIN_MENU_ORBIT_SPEED * dt).rem_euclid(std::f32::consts::TAU);
        self.yaw = self.menu_orbit_angle;
        self.prev_yaw = self.yaw;
    }

    pub fn set_aspect(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.aspect = width as f32 / height as f32;
        }
    }

    pub fn update(
        &mut self,
        input: &Input,
        mouse_delta: (f32, f32),
        target_pos: Vec3,
        collision: &CollisionGrid,
        dt: f32,
    ) {
        self.in_main_menu_orbit = false;
        self.prev_yaw = self.yaw;
        self.prev_pitch = self.pitch;
        self.prev_focus = self.focus;
        self.prev_dist = self.current_dist;

        let eff_sens = MOUSE_SENSITIVITY * self.mouse_sens_mult;
        let mut yaw_delta = -mouse_delta.0 * eff_sens;
        if input.is_pressed(KeyCode::ArrowLeft) {
            yaw_delta += CAMERA_SENSITIVITY * dt;
        }
        if input.is_pressed(KeyCode::ArrowRight) {
            yaw_delta -= CAMERA_SENSITIVITY * dt;
        }
        self.yaw = (self.yaw + yaw_delta).rem_euclid(std::f32::consts::TAU);

        let mut pitch_delta = mouse_delta.1 * eff_sens;
        if input.is_pressed(KeyCode::ArrowUp) {
            pitch_delta += CAMERA_SENSITIVITY * dt;
        }
        if input.is_pressed(KeyCode::ArrowDown) {
            pitch_delta -= CAMERA_SENSITIVITY * dt;
        }
        self.pitch = (self.pitch + pitch_delta).clamp(PITCH_MIN, PITCH_MAX);

        let follow_alpha = 1.0 - (-CAMERA_FOLLOW_SPEED * dt).exp();
        self.focus = self.focus.lerp(target_pos, follow_alpha);

        // Raycast from player head toward desired orbit camera position for collision pull-in
        let head = self.focus + Vec3::new(0.0, CAMERA_HEIGHT_OFFSET, 0.0);
        let orbit_dir = self.orbit_offset_unit(self.yaw, self.pitch);
        let hit_dist = collision.cast_camera_ray(
            head,
            orbit_dir,
            CAMERA_DISTANCE + CAMERA_COLLISION_MARGIN,
            CAMERA_COLLISION_RADIUS,
        );
        let safe_dist = (hit_dist - CAMERA_COLLISION_MARGIN).clamp(CAMERA_MIN_DISTANCE, CAMERA_DISTANCE);

        if safe_dist < self.current_dist {
            // Pull in immediately to prevent clipping into cubes or ground
            self.current_dist = safe_dist;
        } else {
            // Recover smoothly when the obstruction clears
            let rec_alpha = 1.0 - (-CAMERA_RECOVERY_SPEED * dt).exp();
            self.current_dist += (safe_dist - self.current_dist) * rec_alpha;
        }
    }

    fn orbit_offset_unit(&self, yaw: f32, pitch: f32) -> Vec3 {
        let forward_xz = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
        let horiz = pitch.cos();
        let vert = pitch.sin();
        (-forward_xz * horiz + Vec3::new(0.0, vert, 0.0)).normalize_or_zero()
    }

    /// Forward unit vector projected onto the XZ ground plane (-Z at yaw 0).
    pub fn forward_xz(&self) -> Vec3 {
        Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos())
    }

    /// Right unit vector on the XZ ground plane (+X at yaw 0).
    pub fn right_xz(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin())
    }

    /// Computes the interpolated camera eye position and view-projection matrix.
    pub fn interpolated_view_proj_and_eye(&self, alpha: f32) -> (Mat4, Vec3) {
        let proj = Mat4::perspective_rh(self.fov_y_radians, self.aspect, Z_NEAR, Z_FAR);
        if self.in_main_menu_orbit {
            let a = self.menu_orbit_angle;
            let eye = Vec3::new(
                a.sin() * MAIN_MENU_ORBIT_RADIUS,
                MAIN_MENU_ORBIT_HEIGHT,
                a.cos() * MAIN_MENU_ORBIT_RADIUS,
            );
            let view = Mat4::look_at_rh(eye, Vec3::new(0.0, 1.0, 0.0), Vec3::Y);
            return (proj * view, eye);
        }

        let t = alpha.clamp(0.0, 1.0);
        let focus = self.prev_focus.lerp(self.focus, t);
        let yaw = lerp_angle(self.prev_yaw, self.yaw, t);
        let pitch = self.prev_pitch + (self.pitch - self.prev_pitch) * t;
        let dist = self.prev_dist + (self.current_dist - self.prev_dist) * t;

        let target = focus + Vec3::new(0.0, CAMERA_HEIGHT_OFFSET, 0.0);
        let orbit_dir = self.orbit_offset_unit(yaw, pitch);
        let eye = target + orbit_dir * dist;

        let view = Mat4::look_at_rh(eye, target, Vec3::Y);
        (proj * view, eye)
    }
}

pub fn lerp_angle(from: f32, to: f32, t: f32) -> f32 {
    let diff = (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    from + diff * t
}
