use crate::font::GLYPH_SIZE;

pub const BASE_GLYPH_PX: f32 = GLYPH_SIZE as f32; // 8.0

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct UiRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl UiRect {
    #[cfg(test)]
    #[inline]
    pub fn contains_rect(&self, inner: &UiRect) -> bool {
        inner.x >= self.x - 0.5
            && inner.y >= self.y - 0.5
            && inner.x + inner.w <= self.x + self.w + 0.5
            && inner.y + inner.h <= self.y + self.h + 0.5
    }

    #[cfg(test)]
    #[inline]
    pub fn overlaps(&self, other: &UiRect) -> bool {
        let ox = self.x < other.x + other.w - 0.2 && self.x + self.w > other.x + 0.2;
        let oy = self.y < other.y + other.h - 0.2 && self.y + self.h > other.y + 0.2;
        ox && oy
    }
}

/// Returns the rendered pixel width of ASCII string `s` at integer `scale` (`>= 1`).
#[inline]
pub fn text_width(s: &str, scale: u32) -> f32 {
    (s.len() as f32) * BASE_GLYPH_PX * (scale.max(1) as f32)
}

/// Returns the rendered pixel height of one line of text at integer `scale` (`>= 1`).
#[inline]
pub fn text_height(scale: u32) -> f32 {
    BASE_GLYPH_PX * (scale.max(1) as f32)
}

/// Returns the largest integer scale from `max_scale` down to `1` at which `content_w * s <= avail_w`
/// and `content_h * s <= avail_h` (where `content_w` and `content_h` are measured at scale 1).
/// Never returns below `1`.
pub fn fit_scale(
    max_scale: u32,
    content_w_at_1: f32,
    content_h_at_1: f32,
    avail_w: f32,
    avail_h: f32,
) -> u32 {
    let top = max_scale.max(1);
    for s in (1..=top).rev() {
        let sf = s as f32;
        if content_w_at_1 * sf <= avail_w + 0.5 && content_h_at_1 * sf <= avail_h + 0.5 {
            return s;
        }
    }
    1
}

/// Truncates ASCII string `s` with `".."` when `text_width(s, scale) > max_w`.
pub fn truncate_to_width(s: &str, scale: u32, max_w: f32) -> String {
    let gw = text_height(scale);
    if gw <= 0.0 || max_w < gw {
        return String::new();
    }
    let max_chars = (max_w / gw).floor() as usize;
    let bytes = s.as_bytes();
    if bytes.len() <= max_chars {
        return s.to_string();
    }
    if max_chars == 1 {
        return ".".to_string();
    }
    if max_chars == 2 {
        return "..".to_string();
    }
    let keep = max_chars - 2;
    let mut out = String::with_capacity(max_chars);
    for &b in &bytes[..keep] {
        out.push(b as char);
    }
    out.push_str("..");
    out
}

/// Splits `s` into two lines at a space if `s.len() > max_chars`.
pub fn wrap_line(s: &str, max_chars: usize) -> (String, Option<String>) {
    let lim = max_chars.max(1);
    if s.len() <= lim {
        return (s.to_string(), None);
    }
    let bytes = s.as_bytes();
    let mut split_at = None;
    for i in (1..=lim).rev() {
        if bytes[i] == b' ' {
            split_at = Some(i);
            break;
        }
    }
    if split_at.is_none() {
        for (i, &b) in bytes.iter().enumerate().skip(lim + 1) {
            if b == b' ' {
                split_at = Some(i);
                break;
            }
        }
    }
    if let Some(pos) = split_at {
        let left = s[..pos].trim_end().to_string();
        let right = s[pos + 1..].trim_start().to_string();
        if right.is_empty() {
            (left, None)
        } else {
            (left, Some(right))
        }
    } else {
        (s[..lim].to_string(), Some(s[lim..].to_string()))
    }
}

#[derive(Clone, Debug)]
pub struct MeasuredPanelLayout {
    pub scale: u32,
    pub panel: UiRect,
    pub text_rects: Vec<UiRect>,
}

/// Builds the measured F3 debug panel layout from raw lines (wrapping lines > 60 chars, 6px padding).
pub fn build_f3_panel_layout(
    raw_lines: &[String],
    hud_scale: u32,
    sw: f32,
    sh: f32,
) -> (MeasuredPanelLayout, Vec<String>) {
    let mut wrapped = Vec::with_capacity(raw_lines.len() + 4);
    for line in raw_lines {
        let (first, second) = wrap_line(line, 60);
        wrapped.push(first);
        if let Some(sec) = second {
            wrapped.push(sec);
        }
    }

    let max_chars = wrapped.iter().map(|s| s.len()).max().unwrap_or(10);
    let n_lines = wrapped.len().max(1);
    let pad = 6.0;

    let w_at_1 = (max_chars as f32) * BASE_GLYPH_PX;
    let h_at_1 = (n_lines as f32) * (BASE_GLYPH_PX + 4.0) - 4.0;
    let avail_w = (sw - 20.0).max(120.0);
    let avail_h = (sh - 20.0).max(120.0);

    let scale = fit_scale(
        hud_scale,
        w_at_1 + pad * 2.0,
        h_at_1 + pad * 2.0,
        avail_w,
        avail_h,
    );
    let sf = scale as f32;
    let th = text_height(scale);
    let line_step = th + 4.0 * sf;

    let content_w = (w_at_1 * sf).min(avail_w - pad * 2.0);
    let content_h = (n_lines as f32) * line_step - 4.0 * sf;
    let panel = UiRect {
        x: 10.0,
        y: 10.0,
        w: content_w + pad * 2.0,
        h: content_h + pad * 2.0,
    };

    let mut text_rects = Vec::with_capacity(n_lines);
    let mut final_lines = Vec::with_capacity(n_lines);
    for (i, line) in wrapped.into_iter().enumerate() {
        let trunc = truncate_to_width(&line, scale, content_w);
        let tw = text_width(&trunc, scale);
        let ty = panel.y + pad + (i as f32) * line_step;
        text_rects.push(UiRect {
            x: panel.x + pad,
            y: ty,
            w: tw,
            h: th,
        });
        final_lines.push(trunc);
    }

    (
        MeasuredPanelLayout {
            scale,
            panel,
            text_rects,
        },
        final_lines,
    )
}

#[derive(Clone, Debug)]
pub struct SkillMenuLayout {
    pub scale: u32,
    pub panel: UiRect,
    pub pad: f32,
    pub row_step: f32,
    pub inner_w: f32,
    pub left_col_x: f32,
    pub left_col_w: f32,
    pub right_col_x: f32,
    pub right_col_w: f32,
    pub title_y: f32,
    pub section_y: f32,
    pub cols_header_y: f32,
    pub cols_rows_y: f32,
    pub recipes_header_y: f32,
    pub recipes_rows_y: f32,
    pub slots_header_y: f32,
    pub slots_box_y: f32,
    pub slot_w: f32,
    pub slot_h: f32,
    pub slot_gap: f32,
}

/// Computes the Skill Menu layout with explicit measured regions:
/// Title row, Section row, 2 columns (Skills list on left, Details on right, gap >= 24px),
/// full-width Recipes block below the columns, and 4 Slot boxes at the bottom.
pub fn compute_skill_menu_layout(
    hud_scale: u32,
    sw: f32,
    sh: f32,
    widest_left_chars: usize,
    widest_right_chars: usize,
    widest_recipe_chars: usize,
) -> SkillMenuLayout {
    let left_c = widest_left_chars.max(30) as f32;
    let right_c = widest_right_chars.max(34) as f32;
    let rec_c = widest_recipe_chars.max(46) as f32;

    let col_gap_1 = 24.0_f32;
    let pad_1 = 14.0_f32;
    let cols_w_1 = left_c * BASE_GLYPH_PX + col_gap_1 + right_c * BASE_GLYPH_PX;
    let rec_w_1 = rec_c * BASE_GLYPH_PX;
    let slots_w_1 = 4.0 * 128.0 + 3.0 * 8.0;
    let inner_w_1 = cols_w_1.max(rec_w_1).max(slots_w_1);
    let total_w_1 = inner_w_1 + pad_1 * 2.0;

    // Total vertical rows at scale 1:
    // Title(1) + Section(1) + Cols(1 header + 9 skill rows = 10) + Recipes(1 header + 8 lines = 9) + Slots(1 header + 2 box lines = 3) = 24 rows
    let row_step_1 = BASE_GLYPH_PX + 4.0; // 12px
    let total_h_1 = pad_1 * 2.0 + 24.0 * row_step_1 + 28.0;

    let max_panel_w = (sw * 0.92).max(240.0);
    let max_panel_h = (sh * 0.92).max(240.0);
    let scale = fit_scale(hud_scale, total_w_1, total_h_1, max_panel_w, max_panel_h);
    let sf = scale as f32;
    let th = text_height(scale);
    let row_step = th + 4.0 * sf;
    let pad = (12.0 * sf).max(12.0);
    let col_gap = (24.0 * sf).max(24.0);

    let panel_w = (total_w_1 * sf).min(max_panel_w);
    let inner_w = (panel_w - pad * 2.0).max(180.0);
    let left_ratio = left_c / (left_c + right_c);
    let left_col_w = ((inner_w - col_gap) * left_ratio).floor().max(80.0);
    let right_col_w = (inner_w - col_gap - left_col_w).floor().max(80.0);

    let section_gap = 6.0 * sf;
    let slot_h = row_step * 2.0 + 6.0 * sf;
    let panel_h = (pad * 2.0 + 22.0 * row_step + section_gap * 3.0 + slot_h).min(max_panel_h);

    let px = ((sw - panel_w) * 0.5).max(4.0);
    let py = ((sh - panel_h) * 0.5).max(4.0);

    let title_y = py + pad;
    let section_y = title_y + row_step;
    let cols_header_y = section_y + row_step + section_gap;
    let cols_rows_y = cols_header_y + row_step;
    let recipes_header_y = cols_rows_y + 9.0 * row_step + section_gap;
    let recipes_rows_y = recipes_header_y + row_step;
    let slots_header_y = recipes_rows_y + 8.0 * row_step + section_gap;
    let slots_box_y = slots_header_y + row_step;

    let slot_gap = 8.0 * sf;
    let slot_w = ((inner_w - 3.0 * slot_gap) / 4.0).floor().max(32.0);

    SkillMenuLayout {
        scale,
        panel: UiRect {
            x: px,
            y: py,
            w: panel_w,
            h: panel_h,
        },
        pad,
        row_step,
        inner_w,
        left_col_x: px + pad,
        left_col_w,
        right_col_x: px + pad + left_col_w + col_gap,
        right_col_w,
        title_y,
        section_y,
        cols_header_y,
        cols_rows_y,
        recipes_header_y,
        recipes_rows_y,
        slots_header_y,
        slots_box_y,
        slot_w,
        slot_h,
        slot_gap,
    }
}

#[derive(Clone, Debug)]
pub struct TwoColumnControlsLayout {
    pub scale: u32,
    pub panel: UiRect,
    pub pad: f32,
    pub row_step: f32,
    pub title_y: f32,
    pub rows_start_y: f32,
    pub left_x: f32,
    pub left_w: f32,
    pub right_x: f32,
    pub right_w: f32,
    pub back_rect: UiRect,
}

pub fn compute_controls_layout(
    hud_scale: u32,
    sw: f32,
    sh: f32,
    left_lines: &[&str],
    right_lines: &[&str],
) -> TwoColumnControlsLayout {
    let n_rows = left_lines.len().max(right_lines.len()).max(1);
    let left_chars = left_lines.iter().map(|s| s.len()).max().unwrap_or(30) as f32;
    let right_chars = right_lines.iter().map(|s| s.len()).max().unwrap_or(30) as f32;

    let pad_1 = 16.0_f32;
    let gap_1 = 24.0_f32;
    let w_1 = pad_1 * 2.0 + (left_chars + right_chars) * BASE_GLYPH_PX + gap_1;
    let row_step_1 = BASE_GLYPH_PX + 4.0;
    let h_1 = pad_1 * 2.0 + (n_rows as f32 + 3.5) * row_step_1;

    let max_w = (sw * 0.94).max(260.0);
    let max_h = (sh * 0.92).max(240.0);
    let scale = fit_scale(hud_scale, w_1, h_1, max_w, max_h);
    let sf = scale as f32;
    let th = text_height(scale);
    let row_step = th + 4.0 * sf;
    let pad = (16.0 * sf).max(16.0);
    let col_gap = (24.0 * sf).max(24.0);

    let panel_w = (w_1 * sf).min(max_w);
    let inner_w = (panel_w - pad * 2.0).max(160.0);
    let col_w = ((inner_w - col_gap) * 0.5).floor().max(60.0);
    let panel_h = (pad * 2.0 + (n_rows as f32 + 3.2) * row_step).min(max_h);

    let px = ((sw - panel_w) * 0.5).max(4.0);
    let py = ((sh - panel_h) * 0.5).max(4.0);
    let title_y = py + pad;
    let rows_start_y = title_y + row_step * 1.4;
    let back_h = row_step + 4.0 * sf;
    let back_w = (180.0 * sf).min(inner_w);
    let back_y = py + panel_h - pad - back_h;

    TwoColumnControlsLayout {
        scale,
        panel: UiRect {
            x: px,
            y: py,
            w: panel_w,
            h: panel_h,
        },
        pad,
        row_step,
        title_y,
        rows_start_y,
        left_x: px + pad,
        left_w: col_w,
        right_x: px + pad + col_w + col_gap,
        right_w: col_w,
        back_rect: UiRect {
            x: px + (panel_w - back_w) * 0.5,
            y: back_y,
            w: back_w,
            h: back_h,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_valid_panel_and_rects(
        screen_name: &str,
        sw: f32,
        sh: f32,
        panel: &UiRect,
        rects: &[UiRect],
    ) {
        let screen_rect = UiRect {
            x: 0.0,
            y: 0.0,
            w: sw,
            h: sh,
        };
        assert!(
            screen_rect.contains_rect(panel),
            "{screen_name}: panel {:?} outside screen {sw}x{sh}",
            panel
        );
        for (i, r) in rects.iter().enumerate() {
            assert!(
                panel.contains_rect(r),
                "{screen_name}: text rect #{i} {:?} outside panel {:?} at {sw}x{sh}",
                r,
                panel
            );
            for (j, r2) in rects.iter().enumerate().skip(i + 1) {
                assert!(
                    !r.overlaps(r2),
                    "{screen_name}: text rect #{i} {:?} overlaps #{j} {:?} at {sw}x{sh}",
                    r,
                    r2
                );
            }
        }
    }

    #[test]
    fn test_text_width_truncate_fit_scale_and_wrap_line() {
        assert_eq!(text_width("ABCD", 1), 32.0);
        assert_eq!(text_width("ABCD", 2), 64.0);
        assert_eq!(text_height(3), 24.0);

        // truncate_to_width cuts with ".." when exceeding max_w
        assert_eq!(truncate_to_width("HelloWorld", 1, 80.0), "HelloWorld");
        assert_eq!(truncate_to_width("HelloWorld", 1, 48.0), "Hell..");
        assert_eq!(truncate_to_width("HelloWorld", 1, 16.0), "..");

        // fit_scale returns 1 at worst and never a scale that overflows when scale 1 fits
        assert_eq!(fit_scale(3, 300.0, 200.0, 800.0, 600.0), 2);
        assert_eq!(fit_scale(3, 500.0, 400.0, 800.0, 600.0), 1);
        assert_eq!(fit_scale(3, 1200.0, 900.0, 800.0, 600.0), 1);

        // wrap_line splits lines longer than 60 characters at a space
        let long = "St:Playing AlCast:12 Prj:8 Fx:14 Orb:4 Inv[55555|3333] Camp:ON(42s) Food:18";
        let (l1, l2) = wrap_line(long, 60);
        assert!(l1.len() <= 60);
        assert!(l2.is_some());
    }

    #[test]
    fn test_all_screens_layout_bounds_and_no_overlap_across_resolutions_and_scales() {
        let resolutions = [(800.0, 600.0), (1280.0, 720.0), (1600.0, 900.0), (1920.0, 1080.0)];
        let scales = [1u32, 2, 3];

        let f3_sample = vec![
            "FPS: 144.0   CPU:  2.15 ms   GPU:  1.80 ms (Shd: 0.35 ms)".to_string(),
            "Pos: (-24.5,  2.0,  18.3)   Spd: 8.5 m/s   Grounded: YES".to_string(),
            "Draws: 28  Tris: 24500 (Skn: 18200)  Inst: 240/412 (Shd: 310)".to_string(),
            "ShdQ: 2048  Mode: Mailbox  Scale: 0.75x  Yaw: 180d  Flow: Playing".to_string(),
            "Anim: skeletal(auto) 'Survey_Long_Clip' t:12.34s  Jnt: 24".to_string(),
            "Enemy[W:4 C:5 A:2 R:1 D:0]   A*: 180/s  Avg: 42.5 us".to_string(),
            "Squad(AGGRESSIVE)[F:1 E:2 A:1 R:0 G:0 D:0]   AlCast: 19".to_string(),
            "MP: 100  Prj: 12  Fx: 24  Orb: 8  Inv[99999|5555]".to_string(),
            "Camp: ON(45s)  Food: 20  Act[E:1 R:1 T:1 S:1 Q:0 Z:0]  Fairy: Quarrel".to_string(),
        ];

        let ctrl_l = crate::hud_menu::CONTROLS_LEFT;
        let ctrl_r = crate::hud_menu::CONTROLS_RIGHT;

        for &(sw, sh) in &resolutions {
            for &hs in &scales {
                // 1. F3 Panel
                let (f3_lay, _) = build_f3_panel_layout(&f3_sample, hs, sw, sh);
                assert_valid_panel_and_rects("F3Panel", sw, sh, &f3_lay.panel, &f3_lay.text_rects);

                // 2. Skill Menu
                let sk_lay = compute_skill_menu_layout(hs, sw, sh, 32, 38, 48);
                let th = text_height(sk_lay.scale);
                let mut sk_rects = Vec::new();
                sk_rects.push(UiRect { x: sk_lay.left_col_x, y: sk_lay.title_y, w: sk_lay.inner_w, h: th });
                sk_rects.push(UiRect { x: sk_lay.left_col_x, y: sk_lay.section_y, w: sk_lay.inner_w, h: th });
                sk_rects.push(UiRect { x: sk_lay.left_col_x, y: sk_lay.cols_header_y, w: sk_lay.left_col_w, h: th });
                sk_rects.push(UiRect { x: sk_lay.right_col_x, y: sk_lay.cols_header_y, w: sk_lay.right_col_w, h: th });
                for i in 0..9 {
                    sk_rects.push(UiRect {
                        x: sk_lay.left_col_x,
                        y: sk_lay.cols_rows_y + (i as f32) * sk_lay.row_step,
                        w: sk_lay.left_col_w,
                        h: th,
                    });
                }
                for i in 0..4 {
                    sk_rects.push(UiRect {
                        x: sk_lay.right_col_x,
                        y: sk_lay.cols_rows_y + (i as f32) * sk_lay.row_step,
                        w: sk_lay.right_col_w,
                        h: th,
                    });
                }
                sk_rects.push(UiRect { x: sk_lay.left_col_x, y: sk_lay.recipes_header_y, w: sk_lay.inner_w, h: th });
                for i in 0..8 {
                    sk_rects.push(UiRect {
                        x: sk_lay.left_col_x,
                        y: sk_lay.recipes_rows_y + (i as f32) * sk_lay.row_step,
                        w: sk_lay.inner_w,
                        h: th,
                    });
                }
                sk_rects.push(UiRect { x: sk_lay.left_col_x, y: sk_lay.slots_header_y, w: sk_lay.inner_w, h: th });
                for s_i in 0..4 {
                    let sx = sk_lay.left_col_x + (s_i as f32) * (sk_lay.slot_w + sk_lay.slot_gap);
                    sk_rects.push(UiRect { x: sx + 2.0, y: sk_lay.slots_box_y + 2.0, w: sk_lay.slot_w - 4.0, h: th });
                    sk_rects.push(UiRect { x: sx + 2.0, y: sk_lay.slots_box_y + 2.0 + sk_lay.row_step, w: sk_lay.slot_w - 4.0, h: th });
                }
                assert_valid_panel_and_rects("SkillMenu", sw, sh, &sk_lay.panel, &sk_rects);

                // 3. Controls Screen
                let c_lay = compute_controls_layout(hs, sw, sh, &ctrl_l, &ctrl_r);
                let c_th = text_height(c_lay.scale);
                let mut c_rects = Vec::new();
                c_rects.push(UiRect { x: c_lay.left_x, y: c_lay.title_y, w: c_lay.left_w, h: c_th });
                for i in 0..ctrl_l.len() {
                    let ry = c_lay.rows_start_y + (i as f32) * c_lay.row_step;
                    c_rects.push(UiRect { x: c_lay.left_x, y: ry, w: c_lay.left_w, h: c_th });
                    c_rects.push(UiRect { x: c_lay.right_x, y: ry, w: c_lay.right_w, h: c_th });
                }
                c_rects.push(c_lay.back_rect);
                assert_valid_panel_and_rects("ControlsScreen", sw, sh, &c_lay.panel, &c_rects);

                // 4. Squad Screen, Login Report, Camp Banner, Left Squad Panel, and Toast
                let sq_lay = crate::hud_life::compute_squad_screen_layout(hs, sw, sh, 4);
                assert_valid_panel_and_rects("SquadScreen", sw, sh, &sq_lay.panel, &sq_lay.text_rects);

                let lr_lay = crate::hud_life::compute_login_report_layout(hs, sw, sh, 4);
                assert_valid_panel_and_rects("LoginReport", sw, sh, &lr_lay.panel, &lr_lay.text_rects);

                let cb_lay = crate::hud_life::compute_camp_banner_layout(
                    "CAMP 45s - N: skip  |  Food: 20/20  |  H: Squad Status",
                    hs,
                    sw,
                    sh,
                );
                assert_valid_panel_and_rects("CampBanner", sw, sh, &cb_lay.panel, &cb_lay.text_rects);
            }
        }
    }
}
