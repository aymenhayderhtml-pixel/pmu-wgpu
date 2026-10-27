use crate::shadow::ShadowQuality;
use std::fmt::Write;
use std::path::Path;

pub const SETTINGS_FILE_NAME: &str = "settings.cfg";

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PresentModeSetting {
    Fifo,
    Fast,
}

impl PresentModeSetting {
    pub fn toggle(self) -> Self {
        match self {
            Self::Fifo => Self::Fast,
            Self::Fast => Self::Fifo,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Fifo => "Fifo",
            Self::Fast => "Fast",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Settings {
    pub mouse_sensitivity: f32,
    pub fov_degrees: f32,
    pub shadow_quality: ShadowQuality,
    pub render_scale: f32,
    pub present_mode: PresentModeSetting,
    pub hud_scale: u32,
    pub show_title_stats: bool,
    pub master_volume: f32,
    pub music_volume: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 1.0,
            fov_degrees: 70.0,
            shadow_quality: ShadowQuality::Res2048,
            render_scale: 1.0,
            present_mode: PresentModeSetting::Fifo,
            hud_scale: 2,
            show_title_stats: true,
            master_volume: 0.8,
            music_volume: 0.6,
        }
    }
}

impl Settings {
    pub fn clamp_all(&mut self) {
        self.mouse_sensitivity = sanitize_f32(self.mouse_sensitivity, 1.0).clamp(0.2, 3.0);
        self.mouse_sensitivity = (self.mouse_sensitivity * 10.0).round() / 10.0;

        self.fov_degrees = sanitize_f32(self.fov_degrees, 70.0).clamp(50.0, 100.0);
        self.fov_degrees = (self.fov_degrees / 5.0).round() * 5.0;

        self.render_scale = sanitize_f32(self.render_scale, 1.0).clamp(0.5, 1.0);
        self.render_scale = (self.render_scale * 20.0).round() / 20.0;

        self.hud_scale = self.hud_scale.clamp(1, 3);

        self.master_volume = sanitize_f32(self.master_volume, 0.8).clamp(0.0, 1.0);
        self.master_volume = (self.master_volume * 10.0).round() / 10.0;

        self.music_volume = sanitize_f32(self.music_volume, 0.6).clamp(0.0, 1.0);
        self.music_volume = (self.music_volume * 10.0).round() / 10.0;
    }

    /// Key F2 cycles render scale through `1.0 -> 0.85 -> 0.75 -> 0.60 -> 1.0`.
    pub fn cycle_render_scale_f2(&mut self) -> f32 {
        let s = self.render_scale;
        self.render_scale = if (s - 1.0).abs() < 0.02 {
            0.85
        } else if (s - 0.85).abs() < 0.02 {
            crate::post::SCALE_REDUCED
        } else if (s - crate::post::SCALE_REDUCED).abs() < 0.02 {
            0.60
        } else {
            1.0
        };
        self.render_scale
    }

    pub fn to_text(&self) -> String {
        let mut s = String::with_capacity(256);
        let shd = match self.shadow_quality {
            ShadowQuality::Off => "Off",
            ShadowQuality::Res1024 => "1024",
            ShadowQuality::Res2048 => "2048",
        };
        let _ = writeln!(&mut s, "mouse_sensitivity={:.2}", self.mouse_sensitivity);
        let _ = writeln!(&mut s, "fov_degrees={:.0}", self.fov_degrees);
        let _ = writeln!(&mut s, "shadow_quality={}", shd);
        let _ = writeln!(&mut s, "render_scale={:.2}", self.render_scale);
        let _ = writeln!(&mut s, "present_mode={}", self.present_mode.label());
        let _ = writeln!(&mut s, "hud_scale={}", self.hud_scale);
        let _ = writeln!(&mut s, "show_title_stats={}", self.show_title_stats);
        let _ = writeln!(&mut s, "master_volume={:.2}", self.master_volume);
        let _ = writeln!(&mut s, "music_volume={:.2}", self.music_volume);
        s
    }

    pub fn from_text_tolerant(input: &str) -> Self {
        let mut cfg = Self::default();
        for raw_line in input.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let key = k.trim();
            let val = v.trim();
            match key {
                "mouse_sensitivity" => {
                    if let Ok(f) = val.parse::<f32>() {
                        cfg.mouse_sensitivity = f;
                    }
                }
                "fov_degrees" => {
                    if let Ok(f) = val.parse::<f32>() {
                        cfg.fov_degrees = f;
                    }
                }
                "shadow_quality" => {
                    cfg.shadow_quality = match val {
                        "Off" | "off" | "0" => ShadowQuality::Off,
                        "1024" => ShadowQuality::Res1024,
                        "2048" => ShadowQuality::Res2048,
                        _ => cfg.shadow_quality,
                    };
                }
                "render_scale" => {
                    if let Ok(f) = val.parse::<f32>() {
                        cfg.render_scale = f;
                    }
                }
                "present_mode" => {
                    cfg.present_mode = match val {
                        "Fast" | "fast" | "Immediate" | "Mailbox" => PresentModeSetting::Fast,
                        "Fifo" | "fifo" => PresentModeSetting::Fifo,
                        _ => cfg.present_mode,
                    };
                }
                "hud_scale" => {
                    if let Ok(u) = val.parse::<u32>() {
                        cfg.hud_scale = u;
                    }
                }
                "show_title_stats" => {
                    cfg.show_title_stats = match val {
                        "true" | "1" | "On" | "on" => true,
                        "false" | "0" | "Off" | "off" => false,
                        _ => cfg.show_title_stats,
                    };
                }
                "master_volume" => {
                    if let Ok(f) = val.parse::<f32>() {
                        cfg.master_volume = f;
                    }
                }
                "music_volume" => {
                    if let Ok(f) = val.parse::<f32>() {
                        cfg.music_volume = f;
                    }
                }
                _ => {}
            }
        }
        cfg.clamp_all();
        cfg
    }

    pub fn load_from_dir(base_dir: &Path) -> Self {
        let path = base_dir.join(SETTINGS_FILE_NAME);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::from_text_tolerant(&text),
            Err(_) => Self::default(),
        }
    }

    pub fn save_to_dir(&self, base_dir: &Path) -> Result<(), String> {
        let path = base_dir.join(SETTINGS_FILE_NAME);
        std::fs::write(&path, self.to_text())
            .map_err(|e| format!("failed to write settings.cfg: {e}"))
    }
}

#[inline]
fn sanitize_f32(v: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_round_trip_defaults_and_clamping() {
        let dir = std::env::temp_dir().join("pmu_test_settings_phase9");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");

        // Missing file gives defaults
        let def = Settings::load_from_dir(&dir);
        assert_eq!(def, Settings::default());

        // Round-trip save and load
        let custom = Settings {
            mouse_sensitivity: 1.8,
            fov_degrees: 85.0,
            shadow_quality: ShadowQuality::Res1024,
            render_scale: 0.75,
            present_mode: PresentModeSetting::Fast,
            hud_scale: 3,
            show_title_stats: false,
            master_volume: 0.4,
            music_volume: 0.2,
        };
        custom.save_to_dir(&dir).expect("save settings");
        let loaded = Settings::load_from_dir(&dir);
        assert_eq!(loaded, custom);

        // Out-of-range and NaN values are clamped cleanly
        let bad_txt = "mouse_sensitivity=99.0\nfov_degrees=10\nrender_scale=0.1\nhud_scale=9\nmaster_volume=NaN\nmusic_volume=-5.0\nunknown=123\n";
        let clamped = Settings::from_text_tolerant(bad_txt);
        assert!((clamped.mouse_sensitivity - 3.0).abs() < 1e-4);
        assert!((clamped.fov_degrees - 50.0).abs() < 1e-4);
        assert!((clamped.render_scale - 0.5).abs() < 1e-4);
        assert_eq!(clamped.hud_scale, 3);
        assert!((clamped.master_volume - 0.8).abs() < 1e-4);
        assert!((clamped.music_volume - 0.0).abs() < 1e-4);

        // F2 render scale cycling: 1.0 -> 0.85 -> 0.75 -> 0.60 -> 1.0
        let mut s = Settings::default();
        assert!((s.cycle_render_scale_f2() - 0.85).abs() < 1e-4);
        assert!((s.cycle_render_scale_f2() - 0.75).abs() < 1e-4);
        assert!((s.cycle_render_scale_f2() - 0.60).abs() < 1e-4);
        assert!((s.cycle_render_scale_f2() - 1.00).abs() < 1e-4);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
