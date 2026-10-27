use crate::ally::{REVIVE_HOLD_TIME, SUMMON_COST};
use crate::enemy::EnemyState;
use crate::font::GLYPH_SIZE;
use crate::hud::{project_world_to_screen, push_solid_quad, push_text_line, HudVertex, HUD_SCALE};
use crate::squad::STATUS_MESSAGE_DURATION;
use crate::stats::HudTelemetry;
use glam::Vec3;
use std::fmt::Write;

pub const RADAR_SIZE_PX: f32 = 128.0;
pub const RADAR_RANGE: f32 = 40.0;

pub fn draw_squad_hud_elements(
    out: &mut Vec<HudVertex>,
    text_scratch: &mut String,
    sw: f32,
    sh: f32,
    telemetry: &HudTelemetry<'_>,
) {
    let glyph_px = (GLYPH_SIZE as f32) * HUD_SCALE;
    let small_px = 12.0;

    // (a) Left-edge vertically centered Squad Panel
    {
        let row_h = 34.0;
        let header_h = 40.0;
        let panel_w = 228.0;
        let rows = telemetry.allies.len().max(1) as f32;
        let panel_h = header_h + rows * row_h + 10.0;
        let px = 10.0;
        let py = ((sh - panel_h) * 0.5).max(160.0);

        push_solid_quad(out, px, py, panel_w, panel_h, [0.02, 0.03, 0.05, 0.76]);

        text_scratch.clear();
        let _ = write!(text_scratch, "MODE: {} [1/2/3]", telemetry.squad_mode.label());
        push_text_line(out, text_scratch, px + 8.0, py + 6.0, small_px, [1.0, 0.90, 0.40, 1.0]);

        text_scratch.clear();
        let _ = write!(text_scratch, "G:Summon({}p) E:Rev T:Fuse", SUMMON_COST);
        push_text_line(out, text_scratch, px + 8.0, py + 22.0, 10.0, [0.78, 0.85, 0.95, 1.0]);

        let mut ry = py + header_h;
        if telemetry.allies.is_empty() {
            push_text_line(out, "No allies [Press G]", px + 8.0, ry + 8.0, small_px, [0.65, 0.70, 0.75, 1.0]);
        } else {
            for ally in telemetry.allies {
                let tint = if ally.is_down() {
                    [0.60, 0.60, 0.62, 1.0]
                } else {
                    let c = ally.rarity.tint();
                    [c[0], c[1], c[2], 1.0]
                };

                text_scratch.clear();
                let _ = write!(text_scratch, "{:<9} {:>7}", ally.name, ally.state.hud_label());
                push_text_line(out, text_scratch, px + 8.0, ry + 2.0, small_px, tint);

                let bar_x = px + 8.0;
                let bar_y = ry + 18.0;
                let bar_w = panel_w - 16.0;
                let bar_h = 8.0;
                let hp_ratio = (ally.health / ally.max_health.max(1.0)).clamp(0.0, 1.0);

                push_solid_quad(out, bar_x - 1.0, bar_y - 1.0, bar_w + 2.0, bar_h + 2.0, [0.01, 0.01, 0.01, 0.85]);
                let fill_col = if ally.is_down() {
                    [0.45, 0.45, 0.45, 0.85]
                } else {
                    [0.25, 0.92, 0.42, 0.95]
                };
                push_solid_quad(out, bar_x, bar_y, bar_w * hp_ratio, bar_h, fill_col);
                ry += row_h;
            }
        }
    }

    // (b) & (c) World-space ally green health bars, name tags, and revive progress bars
    for ally in telemetry.allies {
        let head_world = ally.pos + Vec3::new(0.0, 2.38, 0.0);
        let Some(screen_pt) = project_world_to_screen(head_world, telemetry.view_proj, sw, sh) else {
            continue;
        };

        let bar_w = 54.0;
        let bar_h = 6.0;
        let bx = screen_pt.x - bar_w * 0.5;
        let by = screen_pt.y - bar_h;
        let ratio = (ally.health / ally.max_health.max(1.0)).clamp(0.0, 1.0);

        let name_px = 10.0;
        let name_w = (ally.name.len() as f32) * name_px;
        let c = ally.rarity.tint();
        let name_col = if ally.is_down() {
            [0.65, 0.65, 0.65, 1.0]
        } else {
            [c[0], c[1], c[2], 1.0]
        };
        push_text_line(out, ally.name, screen_pt.x - name_w * 0.5, by - name_px - 3.0, name_px, name_col);

        push_solid_quad(out, bx - 1.0, by - 1.0, bar_w + 2.0, bar_h + 2.0, [0.02, 0.02, 0.02, 0.85]);
        push_solid_quad(out, bx, by, bar_w * ratio, bar_h, [0.22, 0.92, 0.38, 0.95]);

        if ally.is_down() && ally.revive_progress > 0.0 {
            let rev_ratio = (ally.revive_progress / REVIVE_HOLD_TIME).clamp(0.0, 1.0);
            let ry = by + bar_h + 3.0;
            push_solid_quad(out, bx - 1.0, ry - 1.0, bar_w + 2.0, bar_h + 2.0, [0.02, 0.02, 0.02, 0.90]);
            push_solid_quad(out, bx, ry, bar_w * rev_ratio, bar_h, [0.35, 0.95, 1.0, 0.98]);
        }
    }

    // (d) 128x128 Rotating Camera-Yaw Radar at Top-Right below the Wave Panel
    {
        let rx = (sw - RADAR_SIZE_PX - 12.0).max(10.0);
        let ry = 104.0;
        let half = RADAR_SIZE_PX * 0.5;
        let cx = rx + half;
        let cy = ry + half;

        push_solid_quad(out, rx - 2.0, ry - 2.0, RADAR_SIZE_PX + 4.0, RADAR_SIZE_PX + 4.0, [0.45, 0.55, 0.68, 0.45]);
        push_solid_quad(out, rx, ry, RADAR_SIZE_PX, RADAR_SIZE_PX, [0.02, 0.04, 0.07, 0.78]);

        let (sin_y, cos_y) = telemetry.camera_yaw.sin_cos();
        let scale = (half - 6.0) / RADAR_RANGE;

        let map_to_radar = |world_pos: Vec3| -> Option<(f32, f32)> {
            let dx = world_pos.x - telemetry.player_pos.x;
            let dz = world_pos.z - telemetry.player_pos.z;
            if dx * dx + dz * dz > RADAR_RANGE * RADAR_RANGE {
                return None;
            }
            let local_right = dx * cos_y - dz * sin_y;
            let local_fwd = -dx * sin_y - dz * cos_y;
            let px = (cx + local_right * scale).clamp(rx + 3.0, rx + RADAR_SIZE_PX - 3.0);
            let py = (cy - local_fwd * scale).clamp(ry + 3.0, ry + RADAR_SIZE_PX - 3.0);
            Some((px, py))
        };

        for enemy in telemetry.enemies {
            if !enemy.is_alive() {
                continue;
            }
            if let Some((ex, ey)) = map_to_radar(enemy.pos) {
                let sz = if enemy.state == EnemyState::Chase || enemy.state == EnemyState::Attack { 6.0 } else { 4.0 };
                push_solid_quad(out, ex - sz * 0.5, ey - sz * 0.5, sz, sz, [1.0, 0.22, 0.20, 0.95]);
            }
        }

        for ally in telemetry.allies {
            if let Some((ax, ay)) = map_to_radar(ally.pos) {
                let col = if ally.is_down() {
                    [0.55, 0.55, 0.58, 0.95]
                } else {
                    let c = ally.rarity.tint();
                    [c[0], c[1], c[2], 0.98]
                };
                push_solid_quad(out, ax - 2.5, ay - 2.5, 5.0, 5.0, col);
            }
        }

        push_solid_quad(out, cx - 3.0, cy - 3.0, 6.0, 6.0, [1.0, 1.0, 1.0, 1.0]);
    }

    // (e) Centered short status message (fades after 2s)
    if telemetry.status_timer > 0.0 && !telemetry.status_message.is_empty() {
        let alpha = (telemetry.status_timer / STATUS_MESSAGE_DURATION).clamp(0.0, 1.0);
        let msg_w = (telemetry.status_message.len() as f32) * glyph_px;
        let mx = (sw - msg_w) * 0.5;
        let my = sh * 0.24;
        push_solid_quad(out, mx - 10.0, my - 6.0, msg_w + 20.0, glyph_px + 12.0, [0.02, 0.03, 0.05, 0.78 * alpha]);
        push_text_line(out, telemetry.status_message, mx, my, glyph_px, [1.0, 0.95, 0.45, alpha]);
    }
}
