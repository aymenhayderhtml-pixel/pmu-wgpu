
use crate::font::GLYPH_SIZE;
use crate::game_flow::GameFlow;
use crate::hud::{push_solid_quad, push_text_line, HudVertex};
use crate::hud_life::{draw_login_report_panel, draw_squad_screen_panel};
use crate::menu::{layout, ConfirmKind, MenuScreen, MenuSystem};
use crate::save::SLOT_TITLES;
use crate::stats::HudTelemetry;
use std::fmt::Write;

const CONTROLS_LEFT: [&str; 11] = [
    "WASD           : Move Player",
    "Mouse / Arrows : Orbit Camera",
    "Left Click     : Melee Attack / Menu Click",
    "Space          : Jump (Coyote + Buffer)",
    "Left Shift     : Sprint (8.5 m/s)",
    "C              : Dodge Roll (i-frames)",
    "Q / F / X / V  : Cast Skill Slots 1..4",
    "T              : Quick-Fuse Tier 1 Skills",
    "K              : Open/Close Skill Grimoire",
    "G              : Summon Ally (30 Score)",
    "E (Hold 2.0s)  : Revive Downed Ally",
];

const CONTROLS_RIGHT: [&str; 11] = [
    "1 / 2 / 3      : Squad Mode (Follow/Hold/Aggr)",
    "R              : Restart Game",
    "F1             : Toggle PresentMode (VSync/Fast)",
    "F2             : Cycle Render Scale",
    "F3             : Toggle Top-Left Debug Panel",
    "F4             : Cycle Shadow Quality",
    "F5 / F6        : Cycle Model Yaw / Anim Clip",
    "F7 / F8        : Debug AABBs & Bones / AI Cones",
    "F9 / F10       : Quicksave / Quickload",
    "Delete (Menu)  : Delete Selected Save Slot",
    "Escape         : Pause / Back in Menus",
];

pub fn draw_active_menu_and_fade(
    out: &mut Vec<HudVertex>,
    text_scratch: &mut String,
    sw: f32,
    sh: f32,
    telemetry: &HudTelemetry<'_>,
) {
    let flow = telemetry.game_flow;
    if matches!(flow, GameFlow::MainMenu | GameFlow::PauseMenu | GameFlow::GameOver) {
        draw_menu_screen(out, text_scratch, sw, sh, telemetry.hud_scale, telemetry.menu_sys, telemetry);
    } else if flow == GameFlow::SquadScreen {
        draw_squad_screen_panel(
            out,
            text_scratch,
            sw,
            sh,
            telemetry.allies,
            telemetry.selected_ally_idx,
            telemetry.food_stock,
        );
    } else if flow == GameFlow::LoginReport {
        if let Some(rep) = telemetry.login_report {
            draw_login_report_panel(out, text_scratch, sw, sh, rep);
        }
    }

    if telemetry.fade_alpha > 0.001 {
        let a = telemetry.fade_alpha.clamp(0.0, 1.0);
        push_solid_quad(out, 0.0, 0.0, sw, sh, [0.0, 0.0, 0.0, a]);
    }
}

fn draw_menu_screen(
    out: &mut Vec<HudVertex>,
    text_scratch: &mut String,
    sw: f32,
    sh: f32,
    hud_scale_u: u32,
    menu: &MenuSystem,
    telemetry: &HudTelemetry<'_>,
) {
    // Dark semi-transparent backdrop over the frozen/orbiting 3D scene
    push_solid_quad(out, 0.0, 0.0, sw, sh, [0.01, 0.02, 0.04, 0.62]);

    let scr = menu.current_screen();
    let hs = (hud_scale_u.clamp(1, 3)) as f32;

    if scr == MenuScreen::Controls {
        draw_controls_screen(out, sw, sh, hs, hud_scale_u, menu);
        return;
    }

    let lay = layout(sw as u32, sh as u32, hud_scale_u, menu.item_count);
    let p = lay.panel;

    push_solid_quad(out, p.x - 3.0, p.y - 3.0, p.w + 6.0, p.h + 6.0, [0.42, 0.62, 0.88, 0.92]);
    push_solid_quad(out, p.x, p.y, p.w, p.h, [0.03, 0.05, 0.09, 0.94]);

    let base_glyph = GLYPH_SIZE as f32;
    match scr {
        MenuScreen::Main => {
            let title_px = base_glyph * (hs + 2.0).min(4.5);
            let title = "PICK ME UP";
            let tw = (title.len() as f32) * title_px;
            push_text_line(
                out,
                title,
                p.x + (p.w - tw) * 0.5,
                lay.title_y,
                title_px,
                [1.0, 0.90, 0.35, 1.0],
            );

            let sub_px = base_glyph * (hs * 0.75).max(1.2);
            let sub = "wgpu edition";
            let sub_w = (sub.len() as f32) * sub_px;
            push_text_line(
                out,
                sub,
                p.x + (p.w - sub_w) * 0.5,
                lay.subtitle_y + 4.0,
                sub_px,
                [0.65, 0.88, 1.0, 1.0],
            );
        }
        MenuScreen::GameOver => {
            let title_px = base_glyph * (hs + 1.5).min(4.0);
            let title = "YOU DIED";
            let tw = (title.len() as f32) * title_px;
            push_text_line(
                out,
                title,
                p.x + (p.w - tw) * 0.5,
                lay.title_y,
                title_px,
                [1.0, 0.25, 0.22, 1.0],
            );

            text_scratch.clear();
            let _ = write!(
                text_scratch,
                "Wave: {}   Score: {}   Kills: {}",
                telemetry.wave_number, telemetry.score, telemetry.total_kills
            );
            let sub_px = (base_glyph * hs * 0.75).max(10.0);
            let sub_w = (text_scratch.len() as f32) * sub_px;
            push_text_line(
                out,
                text_scratch,
                p.x + (p.w - sub_w) * 0.5,
                lay.subtitle_y + 2.0,
                sub_px,
                [0.92, 0.94, 0.98, 1.0],
            );
        }
        MenuScreen::Confirm(kind) => {
            let title_px = (base_glyph * (hs + 0.5)).min(24.0);
            text_scratch.clear();
            match kind {
                ConfirmKind::OverwriteSave(slot) => {
                    let name = SLOT_TITLES[slot % SLOT_TITLES.len()];
                    let _ = write!(text_scratch, "Overwrite {}?", name);
                }
                ConfirmKind::DeleteSave(slot) => {
                    let name = SLOT_TITLES[slot % SLOT_TITLES.len()];
                    let _ = write!(text_scratch, "Delete {}?", name);
                }
            }
            let tw = (text_scratch.len() as f32) * title_px;
            push_text_line(
                out,
                text_scratch,
                p.x + (p.w - tw) * 0.5,
                lay.title_y + 8.0,
                title_px,
                [1.0, 0.85, 0.35, 1.0],
            );
        }
        _ => {
            let header = match scr {
                MenuScreen::Pause => "PAUSED",
                MenuScreen::SaveSlots => "SAVE GAME (Enter:Save  Del:Delete  Esc:Back)",
                MenuScreen::LoadSlots => "LOAD GAME (Enter:Load  Del:Delete  Esc:Back)",
                MenuScreen::Settings => "SETTINGS (Left/Right:Adjust  Esc:Back)",
                _ => "MENU",
            };
            let title_px = (base_glyph * (hs * 0.95)).clamp(11.0, 22.0);
            let tw = (header.len() as f32) * title_px;
            push_text_line(
                out,
                header,
                p.x + (p.w - tw).max(12.0) * 0.5,
                lay.title_y + 6.0,
                title_px,
                [1.0, 0.92, 0.45, 1.0],
            );
        }
    }

    for i in 0..lay.row_count {
        let r = lay.rows[i];
        let item = &menu.items[i];
        let is_sel = i == menu.selected;

        let bg_col = if is_sel && item.enabled {
            [0.18, 0.30, 0.48, 0.95]
        } else {
            [0.06, 0.09, 0.15, 0.88]
        };
        let border_col = if is_sel && item.enabled {
            [0.45, 0.90, 1.0, 0.98]
        } else {
            [0.22, 0.28, 0.38, 0.75]
        };

        push_solid_quad(out, r.x - 1.5, r.y - 1.5, r.w + 3.0, r.h + 3.0, border_col);
        push_solid_quad(out, r.x, r.y, r.w, r.h, bg_col);

        let text_col = if item.corrupt {
            [1.0, 0.28, 0.25, 1.0]
        } else if !item.enabled {
            [0.46, 0.48, 0.52, 0.85]
        } else if is_sel {
            [1.0, 0.96, 0.65, 1.0]
        } else {
            [0.90, 0.94, 1.0, 1.0]
        };

        let max_glyph_by_w = ((r.w - 16.0) / (item.label.len().max(1) as f32)).floor();
        let glyph_px = (r.h * 0.48).min(max_glyph_by_w).clamp(8.0, 18.0);
        let ty = r.y + (r.h - glyph_px) * 0.5;
        push_text_line(out, &item.label, r.x + 10.0, ty, glyph_px, text_col);
    }
}

fn draw_controls_screen(
    out: &mut Vec<HudVertex>,
    sw: f32,
    sh: f32,
    hs: f32,
    hud_scale_u: u32,
    menu: &MenuSystem,
) {
    let pw = (760.0 + hs * 40.0).min((sw - 24.0).max(320.0));
    let ph = (390.0 + hs * 24.0).min((sh - 24.0).max(260.0));
    let px = ((sw - pw) * 0.5).max(12.0);
    let py = ((sh - ph) * 0.5).max(12.0);

    push_solid_quad(out, px - 3.0, py - 3.0, pw + 6.0, ph + 6.0, [0.42, 0.62, 0.88, 0.92]);
    push_solid_quad(out, px, py, pw, ph, [0.03, 0.05, 0.09, 0.94]);

    push_text_line(
        out,
        "CONTROLS & HOTKEYS",
        px + 18.0,
        py + 14.0,
        14.0,
        [1.0, 0.92, 0.45, 1.0],
    );

    let col_w = (pw - 40.0) * 0.5;
    let left_x = px + 18.0;
    let right_x = px + 22.0 + col_w;
    let mut row_y = py + 44.0;
    let step_y = ((ph - 110.0) / 11.0).clamp(16.0, 26.0);
    let font_px = (col_w / 46.0).clamp(8.0, 11.0);

    for i in 0..11 {
        push_text_line(out, CONTROLS_LEFT[i], left_x, row_y, font_px, [0.88, 0.94, 1.0, 1.0]);
        push_text_line(out, CONTROLS_RIGHT[i], right_x, row_y, font_px, [0.88, 0.94, 1.0, 1.0]);
        row_y += step_y;
    }

    let lay = layout(sw as u32, sh as u32, hud_scale_u, 1);
    let r = lay.rows[0];
    let back_y = py + ph - r.h - 14.0;
    push_solid_quad(out, r.x - 1.5, back_y - 1.5, r.w + 3.0, r.h + 3.0, [0.45, 0.90, 1.0, 0.98]);
    push_solid_quad(out, r.x, back_y, r.w, r.h, [0.18, 0.30, 0.48, 0.95]);
    let label = &menu.items[0].label;
    push_text_line(out, label, r.x + 12.0, back_y + (r.h - 12.0) * 0.5, 12.0, [1.0, 0.96, 0.65, 1.0]);
}
