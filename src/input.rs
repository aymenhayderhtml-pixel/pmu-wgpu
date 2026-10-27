use std::collections::HashSet;
use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

#[derive(Default)]
pub struct Input {
    pressed: HashSet<KeyCode>,
    just_pressed: HashSet<KeyCode>,
    mouse_delta: (f32, f32),
    attack_requested: bool,
}

impl Input {
    pub fn new() -> Self {
        Self {
            pressed: HashSet::with_capacity(16),
            just_pressed: HashSet::with_capacity(8),
            mouse_delta: (0.0, 0.0),
            attack_requested: false,
        }
    }

    pub fn handle_key_event(&mut self, event: &KeyEvent) {
        if let PhysicalKey::Code(code) = event.physical_key {
            self.set_key_state(code, event.state == ElementState::Pressed, event.repeat);
        }
    }

    pub fn set_key_state(&mut self, code: KeyCode, is_pressed: bool, is_repeat: bool) {
        if is_pressed {
            if !is_repeat && !self.pressed.contains(&code) {
                self.just_pressed.insert(code);
            }
            self.pressed.insert(code);
        } else {
            self.pressed.remove(&code);
        }
    }

    pub fn request_attack(&mut self) {
        self.attack_requested = true;
    }

    pub fn take_attack_requested(&mut self) -> bool {
        let req = self.attack_requested;
        self.attack_requested = false;
        req
    }

    pub fn accumulate_mouse_delta(&mut self, dx: f32, dy: f32) {
        self.mouse_delta.0 += dx;
        self.mouse_delta.1 += dy;
    }

    pub fn take_mouse_delta(&mut self) -> (f32, f32) {
        let delta = self.mouse_delta;
        self.mouse_delta = (0.0, 0.0);
        delta
    }

    pub fn is_pressed(&self, code: KeyCode) -> bool {
        self.pressed.contains(&code)
    }

    pub fn take_just_pressed(&mut self, code: KeyCode) -> bool {
        self.just_pressed.remove(&code)
    }

    pub fn clear(&mut self) {
        self.pressed.clear();
        self.just_pressed.clear();
        self.mouse_delta = (0.0, 0.0);
        self.attack_requested = false;
    }
}
