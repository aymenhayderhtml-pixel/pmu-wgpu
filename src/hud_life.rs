use crate::ally::{Ally, AllyState};
use crate::camp::CampActivity;
use crate::hud::{project_world_to_screen, push_solid_quad, push_text_line, HudVertex};
use crate::life::emotion_label;
use crate::offline::OfflineReport;
use crate::stats::HudTelemetry;
use crate::traits::ALL_PASSIVE_SKILLS;
use crate::ui_text::{
    fit_scale, text_height, text_width, truncate_to_width, MeasuredPanelLayout, UiRect,
    BASE_GLYPH_PX,
};
use glam::Vec3;
use std::fmt::Write;

pub fn compute_camp_banner_layout(
    banner_text: &str,
    hud_scale: u32,
    sw: f32,
    sh: f32,
) -> MeasuredPanelLayout {
    let w_1 = (banner_text.len() as f32) * BASE_GLYPH_PX + 20.0;
    let h_1 = BASE_GLYPH_PX + 12.0;
    let scale = fit_scale(hud_scale, w_1, h_1, sw * 0.90, sh * 0.15);
    let sf = scale as f32;
    let th = text_height(scale);
    let pad = 8.0 * sf;
    let max_txt_w = (sw * 0.90 - pad * 2.0).max(80.0);
    let trunc = truncate_to_width(banner_text, scale, max_txt_w);
    let tw = text_width(&trunc, scale);
    let bw = tw + pad * 2.0;
    let bh = th + pad * 2.0;
    let bx = ((sw - bw) * 0.5).max(4.0);
    let by = 10.0;
    let panel = UiRect {
        x: bx,
        y: by,
        w: bw,
        h: bh,
    };
    let text_rect = UiRect {
        x: bx + pad,
        y: by + pad,
        w: tw,
        h: th,
    };
    MeasuredPanelLayout {
        scale,
        panel,
        text_rects: vec![text_rect],
    }
}

pub fn draw_camp_banner_and_ally_icons(
    out: &mut Vec<HudVertex>,
    text_scratch: &mut String,
    sw: f32,
    sh: f32,
    telemetry: &HudTelemetry<'_>,
) {
    if telemetry.in_camp_phase {
        text_scratch.clear();
        let _ = write!(
            text_scratch,
            "CAMP {:2.0}s - N: skip  |  Food: {}/20  |  H: Squad Status",
            telemetry.camp_timer.ceil(),
            telemetry.food_stock
        );
        let lay = compute_camp_banner_layout(text_scratch, telemetry.hud_scale, sw, sh);
        let p = lay.panel;
        let tr = lay.text_rects[0];
        let trunc = truncate_to_width(text_scratch, lay.scale, p.w - 16.0);
        push_solid_quad(out, p.x, p.y, p.w, p.h, [0.03, 0.06, 0.10, 0.86]);
        push_text_line(out, &trunc, tr.x, tr.y, tr.h, [0.55, 1.0, 0.72, 1.0]);
    }

    for ally in telemetry.allies {
        if ally.is_down() {
            continue;
        }
        let icon_opt: Option<(&'static str, [f32; 4])> = match ally.state {
            AllyState::Flee | AllyState::Freeze => Some(("F", [0.85, 0.45, 1.0, 0.95])),
            _ if telemetry.in_camp_phase => {
                let col = match ally.camp_activity {
                    CampActivity::Eat => [1.0, 0.65, 0.20, 0.95],
                    CampActivity::Rest => [0.30, 0.70, 1.0, 0.95],
                    CampActivity::Train => [0.35, 0.95, 0.50, 0.95],
                    CampActivity::Spar => [0.95, 0.85, 0.25, 0.95],
                    CampActivity::Quarrel => [1.0, 0.20, 0.20, 0.98],
                    CampActivity::Idle => [0.65, 0.70, 0.75, 0.88],
                };
                Some((ally.camp_activity.icon_letter(), col))
            }
            _ => None,
        };

        if let Some((letter, box_col)) = icon_opt {
            let world_top = ally.pos + Vec3::new(0.0, 2.75, 0.0);
            if let Some(sp) = project_world_to_screen(world_top, telemetry.view_proj, sw, sh) {
                let sz = 14.0;
                let ix = sp.x - sz * 0.5;
                let iy = sp.y - sz;
                push_solid_quad(
                    out,
                    ix - 1.0,
                    iy - 1.0,
                    sz + 2.0,
                    sz + 2.0,
                    [0.02, 0.02, 0.02, 0.9],
                );
                push_solid_quad(out, ix, iy, sz, sz, box_col);
                push_text_line(
                    out,
                    letter,
                    ix + 3.0,
                    iy + 3.0,
                    text_height(1),
                    [0.05, 0.05, 0.08, 1.0],
                );
            }
        }
    }
}

pub fn compute_squad_screen_layout(
    hud_scale: u32,
    sw: f32,
    sh: f32,
    ally_count: usize,
) -> MeasuredPanelLayout {
    let rows = ally_count.max(1);
    let w_1 = 80.0 * BASE_GLYPH_PX + 32.0; // 672px at scale 1
    let row_step_1 = BASE_GLYPH_PX + 4.0;
    // 1 header + per ally (3 text lines + 1 bar line = 4 steps)
    let total_steps = 2 + rows * 4;
    let h_1 = 28.0 + (total_steps as f32) * row_step_1;

    let max_w = (sw * 0.92).max(260.0);
    let max_h = (sh * 0.92).max(240.0);
    let scale = fit_scale(hud_scale, w_1, h_1, max_w, max_h);
    let sf = scale as f32;
    let th = text_height(scale);
    let step = th + 4.0 * sf;
    let pad = (14.0 * sf).max(12.0);

    let pw = (w_1 * sf).min(max_w);
    let inner_w = (pw - pad * 2.0).max(120.0);
    let ph = (pad * 2.0 + (total_steps as f32) * step).min(max_h);
    let px = ((sw - pw) * 0.5).max(4.0);
    let py = ((sh - ph) * 0.5).max(4.0);

    let mut rects = Vec::with_capacity(1 + rows * 3);
    let mut y = py + pad;
    rects.push(UiRect {
        x: px + pad,
        y,
        w: inner_w,
        h: th,
    });
    y += step * 1.5;

    for _ in 0..rows {
        for line_i in 0..3 {
            rects.push(UiRect {
                x: px + pad + 4.0,
                y: y + (line_i as f32) * step,
                w: inner_w - 8.0,
                h: th,
            });
        }
        y += step * 4.0;
    }

    MeasuredPanelLayout {
        scale,
        panel: UiRect {
            x: px,
            y: py,
            w: pw,
            h: ph,
        },
        text_rects: rects,
    }
}

pub fn draw_squad_screen_panel(
    out: &mut Vec<HudVertex>,
    text_scratch: &mut String,
    sw: f32,
    sh: f32,
    hud_scale: u32,
    allies: &[Ally],
    selected_idx: usize,
    food_stock: u32,
) {
    let lay = compute_squad_screen_layout(hud_scale, sw, sh, allies.len());
    let sc = lay.scale;
    let th = text_height(sc);
    let p = lay.panel;

    push_solid_quad(out, 0.0, 0.0, sw, sh, [0.01, 0.02, 0.04, 0.65]);
    push_solid_quad(
        out,
        p.x - 3.0,
        p.y - 3.0,
        p.w + 6.0,
        p.h + 6.0,
        [0.45, 0.80, 0.65, 0.92],
    );
    push_solid_quad(out, p.x, p.y, p.w, p.h, [0.03, 0.05, 0.09, 0.95]);

    let hdr_r = lay.text_rects[0];
    text_scratch.clear();
    let _ = write!(
        text_scratch,
        "SQUAD LIFE & TRAINING [PAUSED] | Food: {}/20 | Up/Down:Select H/Esc:Close",
        food_stock
    );
    let hdr_str = truncate_to_width(text_scratch, sc, hdr_r.w);
    push_text_line(out, &hdr_str, hdr_r.x, hdr_r.y, th, [0.65, 1.0, 0.80, 1.0]);

    if allies.is_empty() {
        let r0 = lay.text_rects[1];
        let msg = truncate_to_width("No allies in squad yet. Summon with G (30 Score).", sc, r0.w);
        push_text_line(out, &msg, r0.x, r0.y, th, [0.75, 0.80, 0.88, 1.0]);
        return;
    }

    for (idx, ally) in allies.iter().enumerate() {
        let base_r = 1 + idx * 3;
        let r1 = lay.text_rects[base_r];
        let r2 = lay.text_rects[base_r + 1];
        let r3 = lay.text_rects[base_r + 2];

        let is_sel = idx == selected_idx % allies.len();
        let bg = if is_sel {
            [0.12, 0.22, 0.34, 0.92]
        } else {
            [0.06, 0.09, 0.14, 0.85]
        };
        let box_h = (r3.y + r3.h - r1.y) + 6.0;
        push_solid_quad(out, r1.x - 4.0, r1.y - 3.0, r1.w + 8.0, box_h, bg);

        let c = ally.rarity.tint();
        let stars_str = match ally.profile.stars {
            1 => "*",
            2 => "**",
            _ => "***",
        };
        let emo = emotion_label(&ally.needs, &ally.profile);

        text_scratch.clear();
        let _ = write!(
            text_scratch,
            "{} ({}) {} Lv{} (XP {}/{}) [{},{}] St:{} Mood:{}",
            ally.name,
            stars_str,
            ally.profile.class.short_label(),
            ally.profile.level,
            ally.profile.xp,
            ally.profile.xp_needed_for_next_level(),
            ally.profile.traits[0].short_tag(),
            ally.profile.traits[1].short_tag(),
            ally.state.hud_label(),
            emo.label()
        );
        let l1 = truncate_to_width(text_scratch, sc, r1.w);
        push_text_line(out, &l1, r1.x, r1.y, th, [c[0], c[1], c[2], 1.0]);

        text_scratch.clear();
        let _ = write!(
            text_scratch,
            "HP:{:.0}/{:.0}  Hunger:{:.0}/100  Fear:{:.0}/100  Fatigue:{:.0}/100  PainPts:{}",
            ally.health,
            ally.max_health,
            ally.needs.hunger,
            ally.needs.fear,
            ally.needs.fatigue,
            ally.profile.passives.pain_points
        );
        let l2 = truncate_to_width(text_scratch, sc, r2.w);
        push_text_line(out, &l2, r2.x, r2.y, th, emo.color());

        text_scratch.clear();
        let _ = write!(text_scratch, "Passives: ");
        let mut any_skill = false;
        for &sk in &ALL_PASSIVE_SKILLS {
            let lv = ally.profile.passives.level(sk);
            let prog = ally.profile.passives.progress[sk.index()];
            if lv > 0 || prog > 0 {
                let _ = write!(text_scratch, "{} Lv{}({}/3) ", sk.short_name(), lv, prog);
                any_skill = true;
            }
        }
        if !any_skill {
            text_scratch.push_str("None yet (trains in Camp)");
        }
        let l3 = truncate_to_width(text_scratch, sc, r3.w);
        push_text_line(out, &l3, r3.x, r3.y, th, [0.85, 0.92, 1.0, 1.0]);
    }
}

pub fn compute_login_report_layout(
    hud_scale: u32,
    sw: f32,
    sh: f32,
    row_count: usize,
) -> MeasuredPanelLayout {
    let rows = row_count.max(1);
    let w_1 = 78.0 * BASE_GLYPH_PX + 32.0; // 656px at scale 1
    let step_1 = BASE_GLYPH_PX + 6.0;
    let total_lines = 3 + rows + 1;
    let h_1 = 28.0 + (total_lines as f32) * step_1;

    let max_w = (sw * 0.92).max(260.0);
    let max_h = (sh * 0.92).max(220.0);
    let scale = fit_scale(hud_scale, w_1, h_1, max_w, max_h);
    let sf = scale as f32;
    let th = text_height(scale);
    let step = th + 6.0 * sf;
    let pad = (14.0 * sf).max(12.0);

    let pw = (w_1 * sf).min(max_w);
    let inner_w = (pw - pad * 2.0).max(120.0);
    let ph = (pad * 2.0 + (total_lines as f32) * step).min(max_h);
    let px = ((sw - pw) * 0.5).max(4.0);
    let py = ((sh - ph) * 0.5).max(4.0);

    let mut rects = Vec::with_capacity(total_lines);
    let mut y = py + pad;
    // Title, summary, then `rows` ally rows, then Continue button
    rects.push(UiRect { x: px + pad, y, w: inner_w, h: th });
    y += step;
    rects.push(UiRect { x: px + pad, y, w: inner_w, h: th });
    y += step * 1.4;
    for _ in 0..rows {
        rects.push(UiRect { x: px + pad, y, w: inner_w, h: th });
        y += step;
    }
    rects.push(UiRect {
        x: px + pad,
        y: (py + ph - pad - th).max(y),
        w: inner_w,
        h: th,
    });

    MeasuredPanelLayout {
        scale,
        panel: UiRect {
            x: px,
            y: py,
            w: pw,
            h: ph,
        },
        text_rects: rects,
    }
}

pub fn draw_login_report_panel(
    out: &mut Vec<HudVertex>,
    text_scratch: &mut String,
    sw: f32,
    sh: f32,
    hud_scale: u32,
    report: &OfflineReport,
) {
    let lay = compute_login_report_layout(hud_scale, sw, sh, report.rows.len());
    let sc = lay.scale;
    let th = text_height(sc);
    let p = lay.panel;

    push_solid_quad(out, 0.0, 0.0, sw, sh, [0.01, 0.02, 0.04, 0.72]);
    push_solid_quad(
        out,
        p.x - 3.0,
        p.y - 3.0,
        p.w + 6.0,
        p.h + 6.0,
        [1.0, 0.85, 0.35, 0.92],
    );
    push_solid_quad(out, p.x, p.y, p.w, p.h, [0.03, 0.05, 0.09, 0.96]);

    let r_title = lay.text_rects[0];
    let title = truncate_to_width("WELCOME BACK - OFFLINE CAMP REPORT", sc, r_title.w);
    push_text_line(out, &title, r_title.x, r_title.y, th, [1.0, 0.92, 0.40, 1.0]);

    let r_sum = lay.text_rects[1];
    let hours = report.away_seconds / 3600;
    let mins = (report.away_seconds % 3600) / 60;
    let sim_h = report.simulated_seconds / 3600;
    let sim_m = (report.simulated_seconds % 3600) / 60;
    text_scratch.clear();
    let _ = write!(
        text_scratch,
        "You were away for {}h {}m (sim {}h {}m, {} ticks | Food: -{}, left: {})",
        hours, mins, sim_h, sim_m, report.ticks, report.food_consumed, report.food_remaining
    );
    let sum_str = truncate_to_width(text_scratch, sc, r_sum.w);
    push_text_line(out, &sum_str, r_sum.x, r_sum.y, th, [0.80, 0.90, 1.0, 1.0]);

    for (idx, row) in report.rows.iter().enumerate() {
        let rr = lay.text_rects[2 + idx];
        text_scratch.clear();
        let color = if row.died {
            let _ = write!(
                text_scratch,
                "{} ({} *{}) - DIED WHILE TRAINING",
                row.name,
                row.class.short_label(),
                row.stars
            );
            [1.0, 0.25, 0.25, 1.0]
        } else if row.did_nothing() {
            let _ = write!(text_scratch, "{} did nothing (Hunger {:.0})", row.name, row.end_hunger);
            if row.end_hunger >= 90.0 {
                [1.0, 0.40, 0.30, 1.0]
            } else {
                [0.60, 0.63, 0.68, 1.0]
            }
        } else {
            let skills_joined = if row.skill_notes.is_empty() {
                String::from("none")
            } else {
                row.skill_notes.join(",")
            };
            let _ = write!(
                text_scratch,
                "{}({} *{}): Lv{}->Lv{}(+{}XP) Trn:{}(0x:{}) Sk:[{}] Hng:{:.0}",
                row.name,
                row.class.short_label(),
                row.stars,
                row.start_level,
                row.end_level,
                row.xp_gained,
                row.sessions_trained,
                row.sessions_nothing,
                skills_joined,
                row.end_hunger
            );
            if row.end_level > row.start_level || !row.skill_notes.is_empty() {
                [0.35, 0.98, 0.50, 1.0]
            } else if row.end_hunger >= 85.0 {
                [1.0, 0.45, 0.25, 1.0]
            } else {
                [0.88, 0.92, 0.98, 1.0]
            }
        };
        let row_str = truncate_to_width(text_scratch, sc, rr.w);
        push_text_line(out, &row_str, rr.x, rr.y, th, color);
    }

    if let Some(&r_btn) = lay.text_rects.last() {
        let btn_txt = truncate_to_width("[Enter / Space / Click] Continue", sc, r_btn.w);
        push_text_line(out, &btn_txt, r_btn.x, r_btn.y, th, [1.0, 0.96, 0.65, 1.0]);
    }
}
