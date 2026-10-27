use crate::ally::{Ally, AllyState};
use crate::camp::CampActivity;
use crate::font::GLYPH_SIZE;
use crate::hud::{project_world_to_screen, push_solid_quad, push_text_line, HudVertex};
use crate::life::emotion_label;
use crate::offline::OfflineReport;
use crate::stats::HudTelemetry;
use crate::traits::ALL_PASSIVE_SKILLS;
use glam::Vec3;
use std::fmt::Write;

pub fn draw_camp_banner_and_ally_icons(
    out: &mut Vec<HudVertex>,
    text_scratch: &mut String,
    sw: f32,
    sh: f32,
    telemetry: &HudTelemetry<'_>,
) {
    let hs = (telemetry.hud_scale.clamp(1, 3)) as f32;
    let glyph_px = (GLYPH_SIZE as f32) * hs;

    if telemetry.in_camp_phase {
        text_scratch.clear();
        let _ = write!(
            text_scratch,
            "CAMP {:2.0}s - N: skip  |  Food: {}/20  |  H: Squad Status",
            telemetry.camp_timer.ceil(),
            telemetry.food_stock
        );
        let bw = (text_scratch.len() as f32) * glyph_px + 20.0;
        let bx = (sw - bw) * 0.5;
        let by = 12.0;
        push_solid_quad(out, bx, by, bw, glyph_px + 12.0, [0.03, 0.06, 0.10, 0.86]);
        push_text_line(out, text_scratch, bx + 10.0, by + 6.0, glyph_px, [0.55, 1.0, 0.72, 1.0]);
    }

    // World-space activity / flee / quarrel icon box above each living ally
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
                let sz = 16.0;
                let ix = sp.x - sz * 0.5;
                let iy = sp.y - sz;
                push_solid_quad(out, ix - 1.0, iy - 1.0, sz + 2.0, sz + 2.0, [0.02, 0.02, 0.02, 0.9]);
                push_solid_quad(out, ix, iy, sz, sz, box_col);
                push_text_line(out, letter, ix + 3.0, iy + 3.0, 10.0, [0.05, 0.05, 0.08, 1.0]);
            }
        }
    }
}

pub fn draw_squad_screen_panel(
    out: &mut Vec<HudVertex>,
    text_scratch: &mut String,
    sw: f32,
    sh: f32,
    allies: &[Ally],
    selected_idx: usize,
    food_stock: u32,
) {
    let pw = 760.0_f32.min((sw - 24.0).max(320.0));
    let ph = 440.0_f32.min((sh - 24.0).max(260.0));
    let px = ((sw - pw) * 0.5).max(12.0);
    let py = ((sh - ph) * 0.5).max(12.0);

    push_solid_quad(out, 0.0, 0.0, sw, sh, [0.01, 0.02, 0.04, 0.65]);
    push_solid_quad(out, px - 3.0, py - 3.0, pw + 6.0, ph + 6.0, [0.45, 0.80, 0.65, 0.92]);
    push_solid_quad(out, px, py, pw, ph, [0.03, 0.05, 0.09, 0.95]);

    text_scratch.clear();
    let _ = write!(
        text_scratch,
        "SQUAD LIFE & TRAINING [PAUSED]  |  Party Food: {}/20  |  Up/Down: Select  H/Esc: Close",
        food_stock
    );
    push_text_line(out, text_scratch, px + 14.0, py + 12.0, 11.5, [0.65, 1.0, 0.80, 1.0]);

    if allies.is_empty() {
        push_text_line(
            out,
            "No allies in squad yet. Summon allies with G (30 Score).",
            px + 20.0,
            py + 60.0,
            12.0,
            [0.75, 0.80, 0.88, 1.0],
        );
        return;
    }

    let mut ry = py + 42.0;
    let row_h = 92.0;

    for (idx, ally) in allies.iter().enumerate() {
        let is_sel = idx == selected_idx % allies.len();
        let bg = if is_sel {
            [0.12, 0.22, 0.34, 0.92]
        } else {
            [0.06, 0.09, 0.14, 0.85]
        };
        push_solid_quad(out, px + 12.0, ry, pw - 24.0, row_h - 6.0, bg);

        let c = ally.rarity.tint();
        let stars_str = match ally.profile.stars {
            1 => "*",
            2 => "**",
            _ => "***",
        };
        let emo = emotion_label(&ally.needs, &ally.profile);

        // Line 1: Name, Stars, Class, Level, XP, Traits, State/Activity
        text_scratch.clear();
        let _ = write!(
            text_scratch,
            "{} ({}) {}  Lv{} (XP {}/{})  Traits:[{},{}]  State:{}  Mood:{}",
            ally.name,
            stars_str,
            ally.profile.class.label(),
            ally.profile.level,
            ally.profile.xp,
            ally.profile.xp_needed_for_next_level(),
            ally.profile.traits[0].short_tag(),
            ally.profile.traits[1].short_tag(),
            ally.state.hud_label(),
            emo.label()
        );
        push_text_line(out, text_scratch, px + 20.0, ry + 6.0, 10.5, [c[0], c[1], c[2], 1.0]);

        // Line 2: HP, Hunger, Fear, Fatigue bars
        text_scratch.clear();
        let _ = write!(
            text_scratch,
            "HP:{:.0}/{:.0}   Hunger:{:.0}/100   Fear:{:.0}/100   Fatigue:{:.0}/100   PainPts:{}",
            ally.health,
            ally.max_health,
            ally.needs.hunger,
            ally.needs.fear,
            ally.needs.fatigue,
            ally.profile.passives.pain_points
        );
        push_text_line(out, text_scratch, px + 20.0, ry + 26.0, 10.0, emo.color());

        // Hunger & Fear mini bars
        let bar_w = 120.0;
        let bar_h = 6.0;
        let h_ratio = (ally.needs.hunger / 100.0).clamp(0.0, 1.0);
        let f_ratio = (ally.needs.fear / 100.0).clamp(0.0, 1.0);
        push_solid_quad(out, px + 20.0, ry + 42.0, bar_w, bar_h, [0.15, 0.15, 0.18, 0.9]);
        push_solid_quad(out, px + 20.0, ry + 42.0, bar_w * h_ratio, bar_h, [1.0, 0.65, 0.20, 0.95]);
        push_solid_quad(out, px + 160.0, ry + 42.0, bar_w, bar_h, [0.15, 0.15, 0.18, 0.9]);
        push_solid_quad(out, px + 160.0, ry + 42.0, bar_w * f_ratio, bar_h, [0.75, 0.40, 1.0, 0.95]);

        // Line 3: Passive skills & levels
        text_scratch.clear();
        let _ = write!(text_scratch, "Passives: ");
        let mut any_skill = false;
        for &sk in &ALL_PASSIVE_SKILLS {
            let lv = ally.profile.passives.level(sk);
            let prog = ally.profile.passives.progress[sk.index()];
            if lv > 0 || prog > 0 {
                let _ = write!(text_scratch, "{} Lv{}({}/3)  ", sk.name(), lv, prog);
                any_skill = true;
            }
        }
        if !any_skill {
            text_scratch.push_str("None yet (trains in Camp)");
        }
        push_text_line(out, text_scratch, px + 20.0, ry + 56.0, 9.5, [0.85, 0.92, 1.0, 1.0]);

        ry += row_h;
    }
}

pub fn draw_login_report_panel(
    out: &mut Vec<HudVertex>,
    text_scratch: &mut String,
    sw: f32,
    sh: f32,
    report: &OfflineReport,
) {
    let pw = 740.0_f32.min((sw - 24.0).max(320.0));
    let ph = 380.0_f32.min((sh - 24.0).max(240.0));
    let px = ((sw - pw) * 0.5).max(12.0);
    let py = ((sh - ph) * 0.5).max(12.0);

    push_solid_quad(out, 0.0, 0.0, sw, sh, [0.01, 0.02, 0.04, 0.72]);
    push_solid_quad(out, px - 3.0, py - 3.0, pw + 6.0, ph + 6.0, [1.0, 0.85, 0.35, 0.92]);
    push_solid_quad(out, px, py, pw, ph, [0.03, 0.05, 0.09, 0.96]);

    push_text_line(out, "WELCOME BACK", px + 20.0, py + 14.0, 18.0, [1.0, 0.92, 0.40, 1.0]);

    let hours = report.away_seconds / 3600;
    let mins = (report.away_seconds % 3600) / 60;
    let sim_hours = report.simulated_seconds / 3600;
    let sim_mins = (report.simulated_seconds % 3600) / 60;
    text_scratch.clear();
    let _ = write!(
        text_scratch,
        "You were away for {}h {}m (Simulated {}h {}m / {} ticks | Food used: {}, Remaining: {})",
        hours, mins, sim_hours, sim_mins, report.ticks, report.food_consumed, report.food_remaining
    );
    push_text_line(out, text_scratch, px + 20.0, py + 40.0, 10.5, [0.80, 0.90, 1.0, 1.0]);

    let mut ry = py + 72.0;
    for row in &report.rows {
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
                row.skill_notes.join(", ")
            };
            let _ = write!(
                text_scratch,
                "#{} {} ({} *{}): Lv{}->Lv{} (+{} XP) | Trained:{} (Nothing:{}) | Skills:[{}] | Hunger:{:.0}",
                row.ally_id,
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

        push_text_line(out, text_scratch, px + 20.0, ry, 10.0, color);
        ry += 28.0;
    }

    let btn_y = py + ph - 48.0;
    push_solid_quad(out, px + 20.0, btn_y, 240.0, 32.0, [0.18, 0.32, 0.50, 0.95]);
    push_text_line(
        out,
        "[Enter / Click] Continue",
        px + 32.0,
        btn_y + 10.0,
        11.0,
        [1.0, 0.96, 0.65, 1.0],
    );
}
