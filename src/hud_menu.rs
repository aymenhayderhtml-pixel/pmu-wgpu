use crate::game_flow::GameFlow;
use crate::hud::{push_solid_quad, push_text_line, HudVertex};
use crate::hud_life::{draw_login_report_panel, draw_squad_screen_panel};
use crate::hud_squad::draw_skill_menu_panel;
use crate::menu::{layout, ConfirmKind, MenuScreen, MenuSystem};
use crate::save::SLOT_TITLES;
use crate::stats::HudTelemetry;
use crate::ui_text::{
    compute_controls_layout, fit_scale, text_height, text_width, truncate_to_width, BASE_GLYPH_PX,
};
use std::fmt::Write;

pub const CONTROLS_LEFT: [&str; 13] = [
    "WASD            : Move Player",
    "Mouse / Arrows  : Orbit Camera",
    "Left Click      : Melee Attack / Menu Click",
    "Space           : Jump (Coyote + Buffer)",
    "Left Shift      : Sprint (8.5 m/s)",
    "C               : Dodge Roll (i-frames)",
    "Q / F / X / V   : Cast Skill Slots 1..4",
    "T               : Quick-Fuse Tier 1 Skills",
    "K               : Open/Close Skill Menu",
    "H               : Open/Close Squad Screen",
    "N               : Skip 45s Camp Phase",
    "G               : Summon Ally (30 Score)",
    "E (Hold 2.0s)   : Revive Downed Ally",
];

pub const CONTROLS_RIGHT: [&str; 13] = [
    "1 / 2 / 3 (Game): Squad Mode (Fol/Hld/Aggr)",
    "R               : Restart Game",
    "Up/Down (Menu)  : Select Row / Skill / Ally",
    "Left/Right/Tab  : Adjust Setting / Section",
    "Enter           : Activate / Fuse Recipe",
    "1..4 / Backspace: Equip Slot / Clear Slot",
    "Delete (Saves)  : Delete Selected Save",
    "F1 / F2         : Present Mode / Render Scale",
    "F3 / F4         : Debug Panel / Shadow Qual",
    "F5 / F6         : Model Yaw / Anim Clip",
    "F7 / F8         : Debug AABBs / AI Cones",
    "F9 / F10        : Quicksave / Quickload",
    "Escape          : Pause / Back / Close Menu",
];

/// Draws any active modal menu LAST over the entire screen (including its own dark backdrop),
/// followed by the screen-transition fade overlay.
pub fn draw_active_menu_and_fade(
    out: &mut Vec<HudVertex>,
    text_scratch: &mut String,
    sw: f32,
    sh: f32,
    telemetry: &HudTelemetry<'_>,
) {
    let flow = telemetry.game_flow;
    match flow {
        GameFlow::MainMenu | GameFlow::PauseMenu | GameFlow::GameOver => {
            draw_menu_screen(
                out,
                text_scratch,
                sw,
                sh,
                telemetry.hud_scale,
                telemetry.menu_sys,
                telemetry,
            );
        }
        GameFlow::SkillMenu => {
            draw_skill_menu_panel(out, text_scratch, sw, sh, telemetry);
        }
        GameFlow::SquadScreen => {
            draw_squad_screen_panel(
                out,
                text_scratch,
                sw,
                sh,
                telemetry.hud_scale,
                telemetry.allies,
                telemetry.selected_ally_idx,
                telemetry.food_stock,
            );
        }
        GameFlow::LoginReport => {
            if let Some(rep) = telemetry.login_report {
                draw_login_report_panel(out, text_scratch, sw, sh, telemetry.hud_scale, rep);
            }
        }
        GameFlow::Playing => {}
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
    push_solid_quad(out, 0.0, 0.0, sw, sh, [0.01, 0.02, 0.04, 0.64]);

    let scr = menu.current_screen();
    if scr == MenuScreen::Controls {
        draw_controls_screen(out, sw, sh, hud_scale_u, menu);
        return;
    }

    let lay = layout(sw as u32, sh as u32, hud_scale_u, menu.item_count);
    let p = lay.panel;

    push_solid_quad(
        out,
        p.x - 3.0,
        p.y - 3.0,
        p.w + 6.0,
        p.h + 6.0,
        [0.42, 0.62, 0.88, 0.92],
    );
    push_solid_quad(out, p.x, p.y, p.w, p.h, [0.03, 0.05, 0.09, 0.95]);

    let inner_w = (p.w - 32.0).max(80.0);
    let title_band_h = (lay.subtitle_y - lay.title_y).max(10.0);
    let row0_top = if lay.row_count > 0 {
        lay.rows[0].y
    } else {
        p.y + p.h
    };
    let sub_band_h = (row0_top - lay.subtitle_y - 4.0).max(8.0);

    match scr {
        MenuScreen::Main => {
            let title = "PICK ME UP";
            let t_sc = fit_scale(
                hud_scale_u + 1,
                (title.len() as f32) * BASE_GLYPH_PX,
                BASE_GLYPH_PX,
                inner_w,
                title_band_h,
            );
            let t_th = text_height(t_sc);
            let t_str = truncate_to_width(title, t_sc, inner_w);
            let tw = text_width(&t_str, t_sc);
            push_text_line(
                out,
                &t_str,
                p.x + (p.w - tw) * 0.5,
                lay.title_y,
                t_th,
                [1.0, 0.90, 0.35, 1.0],
            );

            let sub = "wgpu edition";
            let s_sc = fit_scale(
                hud_scale_u,
                (sub.len() as f32) * BASE_GLYPH_PX,
                BASE_GLYPH_PX,
                inner_w,
                sub_band_h,
            );
            let s_th = text_height(s_sc);
            let s_str = truncate_to_width(sub, s_sc, inner_w);
            let sw_px = text_width(&s_str, s_sc);
            push_text_line(
                out,
                &s_str,
                p.x + (p.w - sw_px) * 0.5,
                lay.subtitle_y,
                s_th,
                [0.65, 0.88, 1.0, 1.0],
            );
        }
        MenuScreen::GameOver => {
            let title = "YOU DIED";
            let t_sc = fit_scale(
                hud_scale_u + 1,
                (title.len() as f32) * BASE_GLYPH_PX,
                BASE_GLYPH_PX,
                inner_w,
                title_band_h,
            );
            let t_th = text_height(t_sc);
            let tw = text_width(title, t_sc);
            push_text_line(
                out,
                title,
                p.x + (p.w - tw) * 0.5,
                lay.title_y,
                t_th,
                [1.0, 0.25, 0.22, 1.0],
            );

            text_scratch.clear();
            let _ = write!(
                text_scratch,
                "Wave: {}   Score: {}   Kills: {}",
                telemetry.wave_number, telemetry.score, telemetry.total_kills
            );
            let s_sc = fit_scale(
                hud_scale_u,
                (text_scratch.len() as f32) * BASE_GLYPH_PX,
                BASE_GLYPH_PX,
                inner_w,
                sub_band_h,
            );
            let s_th = text_height(s_sc);
            let s_str = truncate_to_width(text_scratch, s_sc, inner_w);
            let sw_px = text_width(&s_str, s_sc);
            push_text_line(
                out,
                &s_str,
                p.x + (p.w - sw_px) * 0.5,
                lay.subtitle_y,
                s_th,
                [0.92, 0.94, 0.98, 1.0],
            );
        }
        MenuScreen::Confirm(kind) => {
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
            let h_sc = fit_scale(
                hud_scale_u,
                (text_scratch.len() as f32) * BASE_GLYPH_PX,
                BASE_GLYPH_PX,
                inner_w,
                title_band_h + sub_band_h,
            );
            let h_th = text_height(h_sc);
            let h_str = truncate_to_width(text_scratch, h_sc, inner_w);
            let tw = text_width(&h_str, h_sc);
            push_text_line(
                out,
                &h_str,
                p.x + (p.w - tw) * 0.5,
                lay.title_y + 4.0,
                h_th,
                [1.0, 0.85, 0.35, 1.0],
            );
        }
        _ => {
            let header = match scr {
                MenuScreen::Pause => "PAUSED",
                MenuScreen::SaveSlots => "SAVE GAME (Enter:Save Del:Delete Esc:Back)",
                MenuScreen::LoadSlots => "LOAD GAME (Enter:Load Del:Delete Esc:Back)",
                MenuScreen::Settings => "SETTINGS (Left/Right:Adjust Esc:Back)",
                _ => "MENU",
            };
            let h_sc = fit_scale(
                hud_scale_u,
                (header.len() as f32) * BASE_GLYPH_PX,
                BASE_GLYPH_PX,
                inner_w,
                title_band_h + sub_band_h,
            );
            let h_th = text_height(h_sc);
            let h_str = truncate_to_width(header, h_sc, inner_w);
            let tw = text_width(&h_str, h_sc);
            push_text_line(
                out,
                &h_str,
                p.x + (p.w - tw) * 0.5,
                lay.title_y + 4.0,
                h_th,
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

        let row_inner_w = (r.w - 20.0).max(40.0);
        let row_sc = fit_scale(
            hud_scale_u,
            (item.label.len() as f32) * BASE_GLYPH_PX,
            BASE_GLYPH_PX,
            row_inner_w,
            (r.h - 4.0).max(8.0),
        );
        let r_th = text_height(row_sc);
        let r_str = truncate_to_width(&item.label, row_sc, row_inner_w);
        let ty = r.y + (r.h - r_th) * 0.5;
        push_text_line(out, &r_str, r.x + 10.0, ty, r_th, text_col);
    }
}

fn draw_controls_screen(
    out: &mut Vec<HudVertex>,
    sw: f32,
    sh: f32,
    hud_scale_u: u32,
    menu: &MenuSystem,
) {
    let c_lay = compute_controls_layout(hud_scale_u, sw, sh, &CONTROLS_LEFT, &CONTROLS_RIGHT);
    let sc = c_lay.scale;
    let th = text_height(sc);
    let p = c_lay.panel;

    push_solid_quad(
        out,
        p.x - 3.0,
        p.y - 3.0,
        p.w + 6.0,
        p.h + 6.0,
        [0.42, 0.62, 0.88, 0.92],
    );
    push_solid_quad(out, p.x, p.y, p.w, p.h, [0.03, 0.05, 0.09, 0.95]);

    let hdr = truncate_to_width("CONTROLS & HOTKEYS", sc, p.w - c_lay.pad * 2.0);
    push_text_line(
        out,
        &hdr,
        c_lay.left_x,
        c_lay.title_y,
        th,
        [1.0, 0.92, 0.45, 1.0],
    );

    for i in 0..CONTROLS_LEFT.len() {
        let ry = c_lay.rows_start_y + (i as f32) * c_lay.row_step;
        let l_str = truncate_to_width(CONTROLS_LEFT[i], sc, c_lay.left_w);
        let r_str = truncate_to_width(CONTROLS_RIGHT[i], sc, c_lay.right_w);
        push_text_line(out, &l_str, c_lay.left_x, ry, th, [0.88, 0.94, 1.0, 1.0]);
        push_text_line(out, &r_str, c_lay.right_x, ry, th, [0.88, 0.94, 1.0, 1.0]);
    }

    let br = c_lay.back_rect;
    push_solid_quad(
        out,
        br.x - 1.5,
        br.y - 1.5,
        br.w + 3.0,
        br.h + 3.0,
        [0.45, 0.90, 1.0, 0.98],
    );
    push_solid_quad(out, br.x, br.y, br.w, br.h, [0.18, 0.30, 0.48, 0.95]);
    let label = truncate_to_width(&menu.items[0].label, sc, (br.w - 16.0).max(24.0));
    let lw = text_width(&label, sc);
    push_text_line(
        out,
        &label,
        br.x + (br.w - lw) * 0.5,
        br.y + (br.h - th) * 0.5,
        th,
        [1.0, 0.96, 0.65, 1.0],
    );
}
