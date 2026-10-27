use crate::ally::{REVIVE_HOLD_TIME, SUMMON_COST};
use crate::enemy::EnemyState;
use crate::font::GLYPH_SIZE;
use crate::hud::{project_world_to_screen, push_solid_quad, push_text_line, HudVertex, HUD_SCALE};
use crate::skill_menu::{format_skill_details, MenuSection, MenuState};
use crate::skills::SKILL_TABLE;
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

        // Orbs on radar as small dots in their skill color
        for orb in telemetry.orbs {
            if !orb.active {
                continue;
            }
            if let Some((ox, oy)) = map_to_radar(orb.pos) {
                let tc = orb.skill.def().tint;
                push_solid_quad(out, ox - 2.0, oy - 2.0, 4.0, 4.0, [tc[0], tc[1], tc[2], 0.95]);
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

    // (e) Centered short status toast message in skill/event color (fades after 2s)
    if telemetry.status_timer > 0.0 && !telemetry.status_message.is_empty() {
        let alpha = (telemetry.status_timer / STATUS_MESSAGE_DURATION).clamp(0.0, 1.0);
        let msg_w = (telemetry.status_message.len() as f32) * glyph_px;
        let mx = (sw - msg_w) * 0.5;
        let my = sh * 0.20;
        let sc = telemetry.status_color;
        push_solid_quad(out, mx - 10.0, my - 6.0, msg_w + 20.0, glyph_px + 12.0, [0.02, 0.03, 0.05, 0.78 * alpha]);
        push_text_line(out, telemetry.status_message, mx, my, glyph_px, [sc[0], sc[1], sc[2], alpha]);
    }

    // (f) Centered Skill & Fusion Menu when open (Key K)
    if telemetry.menu.open {
        draw_skill_menu_panel(out, text_scratch, sw, sh, telemetry);
    }
}

fn draw_skill_menu_panel(
    out: &mut Vec<HudVertex>,
    text_scratch: &mut String,
    sw: f32,
    sh: f32,
    telemetry: &HudTelemetry<'_>,
) {
    let pw = 760.0;
    let ph = 450.0;
    let px = ((sw - pw) * 0.5).max(12.0);
    let py = ((sh - ph) * 0.5).max(12.0);

    push_solid_quad(out, px - 3.0, py - 3.0, pw + 6.0, ph + 6.0, [0.45, 0.65, 0.90, 0.90]);
    push_solid_quad(out, px, py, pw, ph, [0.03, 0.04, 0.08, 0.94]);

    let sec = telemetry.menu.section;
    text_scratch.clear();
    let _ = write!(
        text_scratch,
        "SKILL GRIMOIRE & FUSION [PAUSED]  |  Section: {:?} (Tab/Left/Right)  |  K/Esc: Close",
        sec
    );
    push_text_line(out, text_scratch, px + 14.0, py + 10.0, 12.0, [1.0, 0.92, 0.45, 1.0]);

    // (a) Left side: all 9 skills in table order
    let list_x = px + 14.0;
    let mut list_y = py + 36.0;
    let list_hdr_col = if sec == MenuSection::Skills { [0.40, 0.95, 1.0, 1.0] } else { [0.75, 0.80, 0.88, 1.0] };
    push_text_line(out, "[SKILLS] Up/Down:Select  1..4:Equip", list_x, list_y, 11.0, list_hdr_col);
    list_y += 18.0;

    for (idx, def) in SKILL_TABLE.iter().enumerate() {
        let count = telemetry.skill_inv.count(def.id);
        let lvl = telemetry.skill_inv.level(def.id);
        let is_sel = idx == telemetry.menu.selected_skill;

        if is_sel {
            push_solid_quad(out, list_x - 4.0, list_y - 2.0, 356.0, 18.0, [0.16, 0.24, 0.38, 0.90]);
        }

        let row_col = if count == 0 {
            [0.48, 0.50, 0.55, 0.85]
        } else {
            [def.tint[0], def.tint[1], def.tint[2], 1.0]
        };

        text_scratch.clear();
        if count == 0 {
            let _ = write!(
                text_scratch,
                "{:<12} LOCKED   T{} {:>2.0}MP {:>3.1}s",
                def.name, def.tier, def.mana_cost, def.cooldown
            );
        } else {
            let _ = write!(
                text_scratch,
                "{:<12} x{} L{}   T{} {:>2.0}MP {:>3.1}s",
                def.name, count, lvl, def.tier, def.mana_cost, def.cooldown
            );
        }
        push_text_line(out, text_scratch, list_x, list_y, 11.0, row_col);
        list_y += 20.0;
    }

    // (b) Right side: selected skill details
    let right_x = px + 384.0;
    let mut right_y = py + 36.0;
    push_text_line(out, "[SELECTED SKILL DETAILS]", right_x, right_y, 11.0, [0.85, 0.90, 1.0, 1.0]);
    right_y += 18.0;

    let sel_skill = telemetry.menu.selected_skill_id();
    let sel_lvl = telemetry.skill_inv.level(sel_skill);
    let mut detail_lines: [String; 4] = Default::default();
    format_skill_details(sel_skill, sel_lvl, &mut detail_lines);
    let st = sel_skill.def().tint;
    push_text_line(out, &detail_lines[0], right_x, right_y, 12.0, [st[0], st[1], st[2], 1.0]);
    right_y += 18.0;
    for line in &detail_lines[1..] {
        push_text_line(out, line, right_x, right_y, 10.5, [0.90, 0.94, 1.0, 1.0]);
        right_y += 16.0;
    }

    // (d) Right-bottom: Fusion Recipes section
    right_y += 14.0;
    let rec_hdr_col = if sec == MenuSection::Recipes { [0.40, 0.95, 1.0, 1.0] } else { [0.75, 0.80, 0.88, 1.0] };
    push_text_line(out, "[FUSION RECIPES] Enter/T:Fuse (Need 2x each)", right_x, right_y, 11.0, rec_hdr_col);
    right_y += 18.0;

    let statuses = MenuState::recipe_statuses(telemetry.skill_inv);
    for (r_idx, st_rec) in statuses.iter().enumerate() {
        let is_sel = sec == MenuSection::Recipes && r_idx == telemetry.menu.selected_recipe;
        if is_sel {
            push_solid_quad(out, right_x - 4.0, right_y - 2.0, 364.0, 28.0, [0.18, 0.28, 0.42, 0.92]);
        }
        let col = if st_rec.craftable {
            [0.30, 0.98, 0.45, 1.0]
        } else {
            [0.58, 0.60, 0.65, 0.90]
        };
        text_scratch.clear();
        let _ = write!(
            text_scratch,
            "{} + {} = {}",
            st_rec.src_a.def().name,
            st_rec.src_b.def().name,
            st_rec.result.def().name
        );
        push_text_line(out, text_scratch, right_x, right_y, 10.5, col);

        text_scratch.clear();
        if st_rec.craftable {
            let _ = write!(text_scratch, "  -> READY TO FUSE");
        } else {
            let _ = write!(
                text_scratch,
                "  -> Need +{} {}, +{} {}",
                st_rec.missing_a,
                st_rec.src_a.def().short_name,
                st_rec.missing_b,
                st_rec.src_b.def().short_name
            );
        }
        push_text_line(out, text_scratch, right_x, right_y + 12.0, 9.5, col);
        right_y += 30.0;
    }

    // (c) Bottom: 4 Active Slots (Q, F, X, V)
    let bot_y = py + ph - 76.0;
    let slot_hdr_col = if sec == MenuSection::Slots { [0.40, 0.95, 1.0, 1.0] } else { [0.75, 0.80, 0.88, 1.0] };
    push_text_line(
        out,
        "[EQUIPPED SLOTS] 1..4:Assign Selected Skill   Backspace:Clear Slot",
        list_x,
        bot_y,
        11.0,
        slot_hdr_col,
    );

    let keys = ["1:Q", "2:F", "3:X", "4:V"];
    for s_idx in 0..4 {
        let bx = list_x + (s_idx as f32) * 180.0;
        let by = bot_y + 18.0;
        let bw = 168.0;
        let bh = 42.0;
        let is_slot_sel = sec == MenuSection::Slots && telemetry.menu.selected_slot == s_idx;
        let b_col = if is_slot_sel {
            [0.35, 0.95, 1.0, 0.95]
        } else {
            [0.30, 0.38, 0.50, 0.85]
        };
        push_solid_quad(out, bx - 2.0, by - 2.0, bw + 4.0, bh + 4.0, b_col);
        push_solid_quad(out, bx, by, bw, bh, [0.05, 0.08, 0.14, 0.95]);

        push_text_line(out, keys[s_idx], bx + 6.0, by + 5.0, 10.0, [0.85, 0.90, 1.0, 1.0]);
        if let Some(sk) = telemetry.skill_inv.slots[s_idx] {
            let d = sk.def();
            let lv = telemetry.skill_inv.level(sk);
            text_scratch.clear();
            let _ = write!(text_scratch, "{} (L{})", d.name, lv);
            push_text_line(out, text_scratch, bx + 6.0, by + 22.0, 10.5, [d.tint[0], d.tint[1], d.tint[2], 1.0]);
        } else {
            push_text_line(out, "EMPTY", bx + 6.0, by + 22.0, 10.5, [0.45, 0.48, 0.52, 0.9]);
        }
    }
}
