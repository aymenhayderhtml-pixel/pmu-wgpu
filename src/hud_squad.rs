use crate::ally::{REVIVE_HOLD_TIME, SUMMON_COST};
use crate::enemy::EnemyState;
use crate::hud::{project_world_to_screen, push_solid_quad, push_text_line, HudVertex};
use crate::skill_menu::{format_skill_details, MenuSection, MenuState};
use crate::skills::SKILL_TABLE;
use crate::squad::STATUS_MESSAGE_DURATION;
use crate::stats::HudTelemetry;
use crate::ui_text::{
    compute_skill_menu_layout, fit_scale, text_height, text_width, truncate_to_width,
    BASE_GLYPH_PX,
};
use glam::Vec3;
use std::fmt::Write;

pub const RADAR_SIZE_PX: f32 = 128.0;
pub const RADAR_RANGE: f32 = 40.0;

/// Draws only the in-game Squad HUD elements: left-edge squad panel, world-space ally bars, and top-right radar.
/// Toasts and the Skill Menu are drawn in their own dedicated passes after the F3 panel.
pub fn draw_squad_hud_elements(
    out: &mut Vec<HudVertex>,
    text_scratch: &mut String,
    sw: f32,
    sh: f32,
    telemetry: &HudTelemetry<'_>,
) {
    // (a) Left-edge vertically centered Squad Panel (measured from content)
    {
        let rows = telemetry.allies.len().max(1);
        let max_chars = 24.0_f32;
        let w_at_1 = max_chars * BASE_GLYPH_PX + 16.0;
        let h_at_1 = 36.0 + (rows as f32) * 26.0 + 10.0;
        let sc = fit_scale(telemetry.hud_scale, w_at_1, h_at_1, sw * 0.32, sh * 0.55);
        let sf = sc as f32;
        let th = text_height(sc);
        let pad = 8.0 * sf;
        let row_h = th + 16.0 * sf;
        let header_h = th * 2.0 + 12.0 * sf;
        let inner_w = max_chars * th;
        let panel_w = inner_w + pad * 2.0;
        let panel_h = header_h + (rows as f32) * row_h + pad;
        let px = 10.0;
        let py = ((sh - panel_h) * 0.5).clamp(10.0, (sh - panel_h - 10.0).max(10.0));

        push_solid_quad(out, px, py, panel_w, panel_h, [0.02, 0.03, 0.05, 0.76]);

        text_scratch.clear();
        let _ = write!(text_scratch, "MODE: {} [1/2/3]", telemetry.squad_mode.label());
        let l1 = truncate_to_width(text_scratch, sc, inner_w);
        push_text_line(out, &l1, px + pad, py + 4.0 * sf, th, [1.0, 0.90, 0.40, 1.0]);

        text_scratch.clear();
        let _ = write!(text_scratch, "G:Summon({}p) E:Rev T:Fuse", SUMMON_COST);
        let l2 = truncate_to_width(text_scratch, sc, inner_w);
        push_text_line(
            out,
            &l2,
            px + pad,
            py + 4.0 * sf + th + 3.0 * sf,
            th,
            [0.78, 0.85, 0.95, 1.0],
        );

        let mut ry = py + header_h;
        if telemetry.allies.is_empty() {
            let empty_msg = truncate_to_width("No allies [Press G]", sc, inner_w);
            push_text_line(out, &empty_msg, px + pad, ry + 4.0 * sf, th, [0.65, 0.70, 0.75, 1.0]);
        } else {
            for ally in telemetry.allies {
                let tint = if ally.is_down() {
                    [0.60, 0.60, 0.62, 1.0]
                } else {
                    let c = ally.rarity.tint();
                    [c[0], c[1], c[2], 1.0]
                };

                text_scratch.clear();
                let _ = write!(
                    text_scratch,
                    "{:<8} L{:<2} {:>6}",
                    ally.name,
                    ally.profile.level,
                    ally.state.hud_label()
                );
                let row_txt = truncate_to_width(text_scratch, sc, inner_w - 10.0 * sf);
                push_text_line(out, &row_txt, px + pad, ry + 2.0 * sf, th, tint);

                // Colored hunger dot next to ally row
                let h_col = if ally.needs.hunger > 90.0 {
                    [1.0, 0.25, 0.20, 1.0]
                } else if ally.needs.hunger > 70.0 {
                    [1.0, 0.78, 0.22, 1.0]
                } else {
                    [0.35, 0.95, 0.45, 0.95]
                };
                let dot_sz = 5.0 * sf;
                push_solid_quad(
                    out,
                    px + panel_w - pad - dot_sz,
                    ry + 3.0 * sf,
                    dot_sz,
                    dot_sz,
                    h_col,
                );

                let bar_x = px + pad;
                let bar_y = ry + th + 5.0 * sf;
                let bar_w = inner_w;
                let bar_h = 6.0 * sf;
                let hp_ratio = (ally.health / ally.max_health.max(1.0)).clamp(0.0, 1.0);

                push_solid_quad(
                    out,
                    bar_x - 1.0,
                    bar_y - 1.0,
                    bar_w + 2.0,
                    bar_h + 2.0,
                    [0.01, 0.01, 0.01, 0.85],
                );
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
        let Some(screen_pt) = project_world_to_screen(head_world, telemetry.view_proj, sw, sh)
        else {
            continue;
        };

        let bar_w = 54.0;
        let bar_h = 6.0;
        let bx = screen_pt.x - bar_w * 0.5;
        let by = screen_pt.y - bar_h;
        let ratio = (ally.health / ally.max_health.max(1.0)).clamp(0.0, 1.0);

        let name_px = text_height(1);
        let name_w = text_width(ally.name, 1);
        let c = ally.rarity.tint();
        let name_col = if ally.is_down() {
            [0.65, 0.65, 0.65, 1.0]
        } else {
            [c[0], c[1], c[2], 1.0]
        };
        push_text_line(
            out,
            ally.name,
            screen_pt.x - name_w * 0.5,
            by - name_px - 3.0,
            name_px,
            name_col,
        );

        push_solid_quad(
            out,
            bx - 1.0,
            by - 1.0,
            bar_w + 2.0,
            bar_h + 2.0,
            [0.02, 0.02, 0.02, 0.85],
        );
        push_solid_quad(
            out,
            bx,
            by,
            bar_w * ratio,
            bar_h,
            [0.22, 0.92, 0.38, 0.95],
        );

        if ally.is_down() && ally.revive_progress > 0.0 {
            let rev_ratio = (ally.revive_progress / REVIVE_HOLD_TIME).clamp(0.0, 1.0);
            let ry = by + bar_h + 3.0;
            push_solid_quad(
                out,
                bx - 1.0,
                ry - 1.0,
                bar_w + 2.0,
                bar_h + 2.0,
                [0.02, 0.02, 0.02, 0.90],
            );
            push_solid_quad(
                out,
                bx,
                ry,
                bar_w * rev_ratio,
                bar_h,
                [0.35, 0.95, 1.0, 0.98],
            );
        }
    }

    // (d) 128x128 Rotating Camera-Yaw Radar at Top-Right below the Wave Panel
    {
        let rx = (sw - RADAR_SIZE_PX - 12.0).max(10.0);
        let ry = 104.0;
        let half = RADAR_SIZE_PX * 0.5;
        let cx = rx + half;
        let cy = ry + half;

        push_solid_quad(
            out,
            rx - 2.0,
            ry - 2.0,
            RADAR_SIZE_PX + 4.0,
            RADAR_SIZE_PX + 4.0,
            [0.45, 0.55, 0.68, 0.45],
        );
        push_solid_quad(
            out,
            rx,
            ry,
            RADAR_SIZE_PX,
            RADAR_SIZE_PX,
            [0.02, 0.04, 0.07, 0.78],
        );

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
                let sz =
                    if enemy.state == EnemyState::Chase || enemy.state == EnemyState::Attack {
                        6.0
                    } else {
                        4.0
                    };
                push_solid_quad(
                    out,
                    ex - sz * 0.5,
                    ey - sz * 0.5,
                    sz,
                    sz,
                    [1.0, 0.22, 0.20, 0.95],
                );
            }
        }

        for orb in telemetry.orbs {
            if !orb.active {
                continue;
            }
            if let Some((ox, oy)) = map_to_radar(orb.pos) {
                let tc = orb.tint();
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
}

/// Draws the centered status toast message (drawn after the F3 panel and before menus).
pub fn draw_toast_message(out: &mut Vec<HudVertex>, sw: f32, sh: f32, telemetry: &HudTelemetry<'_>) {
    if telemetry.status_timer <= 0.0 || telemetry.status_message.is_empty() {
        return;
    }
    let alpha = (telemetry.status_timer / STATUS_MESSAGE_DURATION).clamp(0.0, 1.0);
    let w_at_1 = (telemetry.status_message.len() as f32) * BASE_GLYPH_PX + 24.0;
    let sc = fit_scale(telemetry.hud_scale, w_at_1, 20.0, sw * 0.90, sh * 0.20);
    let th = text_height(sc);
    let max_txt_w = (sw * 0.90 - 24.0).max(80.0);
    let msg = truncate_to_width(telemetry.status_message, sc, max_txt_w);
    let msg_w = text_width(&msg, sc);
    let mx = (sw - msg_w) * 0.5;
    let my = (sh * 0.18).clamp(16.0, (sh - th - 16.0).max(16.0));
    let sc_col = telemetry.status_color;
    push_solid_quad(
        out,
        mx - 10.0,
        my - 6.0,
        msg_w + 20.0,
        th + 12.0,
        [0.02, 0.03, 0.05, 0.78 * alpha],
    );
    push_text_line(
        out,
        &msg,
        mx,
        my,
        th,
        [sc_col[0], sc_col[1], sc_col[2], alpha],
    );
}

/// Draws the centered Skill Menu (Key K) with dark backdrop over everything.
pub fn draw_skill_menu_panel(
    out: &mut Vec<HudVertex>,
    text_scratch: &mut String,
    sw: f32,
    sh: f32,
    telemetry: &HudTelemetry<'_>,
) {
    push_solid_quad(out, 0.0, 0.0, sw, sh, [0.01, 0.02, 0.04, 0.64]);

    let sel_skill = telemetry.menu.selected_skill_id();
    let sel_lvl = telemetry.skill_inv.level(sel_skill);
    let mut detail_lines: [String; 4] = Default::default();
    format_skill_details(sel_skill, sel_lvl, &mut detail_lines);
    let widest_right = detail_lines.iter().map(|s| s.len()).max().unwrap_or(34);

    let lay = compute_skill_menu_layout(telemetry.hud_scale, sw, sh, 32, widest_right, 48);
    let sc = lay.scale;
    let th = text_height(sc);
    let p = lay.panel;

    push_solid_quad(
        out,
        p.x - 3.0,
        p.y - 3.0,
        p.w + 6.0,
        p.h + 6.0,
        [0.45, 0.65, 0.90, 0.92],
    );
    push_solid_quad(out, p.x, p.y, p.w, p.h, [0.03, 0.04, 0.08, 0.95]);

    let sec = telemetry.menu.section;
    let title = truncate_to_width("SKILL GRIMOIRE & FUSION [PAUSED]", sc, p.w - lay.pad * 2.0);
    push_text_line(
        out,
        &title,
        lay.left_col_x,
        lay.title_y,
        th,
        [1.0, 0.92, 0.45, 1.0],
    );

    text_scratch.clear();
    let _ = write!(
        text_scratch,
        "Section: {:?} (Tab/Left/Right) | 1..4:Equip | K/Esc:Close",
        sec
    );
    let sec_line = truncate_to_width(text_scratch, sc, lay.inner_w);
    push_text_line(
        out,
        &sec_line,
        lay.left_col_x,
        lay.section_y,
        th,
        [0.75, 0.88, 1.0, 1.0],
    );

    // Left Column: Skills list
    let list_hdr_col = if sec == MenuSection::Skills {
        [0.40, 0.95, 1.0, 1.0]
    } else {
        [0.75, 0.80, 0.88, 1.0]
    };
    let l_hdr = truncate_to_width("[SKILLS] Up/Down:Select", sc, lay.left_col_w);
    push_text_line(
        out,
        &l_hdr,
        lay.left_col_x,
        lay.cols_header_y,
        th,
        list_hdr_col,
    );

    for (idx, def) in SKILL_TABLE.iter().enumerate() {
        let count = telemetry.skill_inv.count(def.id);
        let lvl = telemetry.skill_inv.level(def.id);
        let ry = lay.cols_rows_y + (idx as f32) * lay.row_step;
        if idx == telemetry.menu.selected_skill {
            push_solid_quad(
                out,
                lay.left_col_x - 2.0,
                ry - 1.0,
                lay.left_col_w + 4.0,
                th + 2.0,
                [0.16, 0.24, 0.38, 0.90],
            );
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
                "{:<11} LOCKED T{} {:>2.0}M {:.1}s",
                def.name, def.tier, def.mana_cost, def.cooldown
            );
        } else {
            let _ = write!(
                text_scratch,
                "{:<11} x{} L{} T{} {:>2.0}M {:.1}s",
                def.name, count, lvl, def.tier, def.mana_cost, def.cooldown
            );
        }
        let row_str = truncate_to_width(text_scratch, sc, lay.left_col_w);
        push_text_line(out, &row_str, lay.left_col_x, ry, th, row_col);
    }

    // Right Column: Selected Skill Details
    let r_hdr = truncate_to_width("[SELECTED SKILL DETAILS]", sc, lay.right_col_w);
    push_text_line(
        out,
        &r_hdr,
        lay.right_col_x,
        lay.cols_header_y,
        th,
        [0.85, 0.90, 1.0, 1.0],
    );
    let st = sel_skill.def().tint;
    for (d_idx, d_line) in detail_lines.iter().enumerate() {
        let ry = lay.cols_rows_y + (d_idx as f32) * lay.row_step;
        let col = if d_idx == 0 {
            [st[0], st[1], st[2], 1.0]
        } else {
            [0.90, 0.94, 1.0, 1.0]
        };
        let d_trunc = truncate_to_width(d_line, sc, lay.right_col_w);
        push_text_line(out, &d_trunc, lay.right_col_x, ry, th, col);
    }

    // Full-width Recipes Block below the two columns
    let rec_hdr_col = if sec == MenuSection::Recipes {
        [0.40, 0.95, 1.0, 1.0]
    } else {
        [0.75, 0.80, 0.88, 1.0]
    };
    let rec_hdr = truncate_to_width(
        "[FUSION RECIPES] Enter/T:Fuse (Need 2x each)",
        sc,
        lay.inner_w,
    );
    push_text_line(
        out,
        &rec_hdr,
        lay.left_col_x,
        lay.recipes_header_y,
        th,
        rec_hdr_col,
    );

    let statuses = MenuState::recipe_statuses(telemetry.skill_inv);
    for (r_idx, st_rec) in statuses.iter().enumerate() {
        let ry1 = lay.recipes_rows_y + (r_idx as f32 * 2.0) * lay.row_step;
        let ry2 = ry1 + lay.row_step;
        if sec == MenuSection::Recipes && r_idx == telemetry.menu.selected_recipe {
            push_solid_quad(
                out,
                lay.left_col_x - 2.0,
                ry1 - 1.0,
                lay.inner_w + 4.0,
                lay.row_step * 2.0,
                [0.18, 0.28, 0.42, 0.92],
            );
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
        let rl1 = truncate_to_width(text_scratch, sc, lay.inner_w);
        push_text_line(out, &rl1, lay.left_col_x, ry1, th, col);

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
        let rl2 = truncate_to_width(text_scratch, sc, lay.inner_w);
        push_text_line(out, &rl2, lay.left_col_x, ry2, th, col);
    }

    // Bottom: 4 Active Slots (Q, F, X, V)
    let slot_hdr_col = if sec == MenuSection::Slots {
        [0.40, 0.95, 1.0, 1.0]
    } else {
        [0.75, 0.80, 0.88, 1.0]
    };
    let sl_hdr = truncate_to_width(
        "[EQUIPPED SLOTS] 1..4:Assign  Backspace:Clear",
        sc,
        lay.inner_w,
    );
    push_text_line(
        out,
        &sl_hdr,
        lay.left_col_x,
        lay.slots_header_y,
        th,
        slot_hdr_col,
    );

    let keys = ["1:Q", "2:F", "3:X", "4:V"];
    for s_idx in 0..4 {
        let bx = lay.left_col_x + (s_i_f(s_idx)) * (lay.slot_w + lay.slot_gap);
        let by = lay.slots_box_y;
        let is_slot_sel = sec == MenuSection::Slots && telemetry.menu.selected_slot == s_idx;
        let b_col = if is_slot_sel {
            [0.35, 0.95, 1.0, 0.95]
        } else {
            [0.30, 0.38, 0.50, 0.85]
        };
        push_solid_quad(
            out,
            bx - 1.5,
            by - 1.5,
            lay.slot_w + 3.0,
            lay.slot_h + 3.0,
            b_col,
        );
        push_solid_quad(out, bx, by, lay.slot_w, lay.slot_h, [0.05, 0.08, 0.14, 0.95]);

        let box_inner_w = (lay.slot_w - 8.0).max(16.0);
        let k_str = truncate_to_width(keys[s_idx], sc, box_inner_w);
        push_text_line(out, &k_str, bx + 4.0, by + 2.0, th, [0.85, 0.90, 1.0, 1.0]);

        if let Some(sk) = telemetry.skill_inv.slots[s_idx] {
            let d = sk.def();
            let lv = telemetry.skill_inv.level(sk);
            text_scratch.clear();
            let _ = write!(text_scratch, "{} L{}", d.name, lv);
            let s_str = truncate_to_width(text_scratch, sc, box_inner_w);
            push_text_line(
                out,
                &s_str,
                bx + 4.0,
                by + 2.0 + lay.row_step,
                th,
                [d.tint[0], d.tint[1], d.tint[2], 1.0],
            );
        } else {
            let e_str = truncate_to_width("EMPTY", sc, box_inner_w);
            push_text_line(
                out,
                &e_str,
                bx + 4.0,
                by + 2.0 + lay.row_step,
                th,
                [0.45, 0.48, 0.52, 0.9],
            );
        }
    }
}

#[inline]
fn s_i_f(i: usize) -> f32 {
    i as f32
}
