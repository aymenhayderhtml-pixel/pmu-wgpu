use crate::save::{format_playtime_hms, format_utc_datetime, most_recent_save_slot, SlotStatus, SLOT_TITLES};
use crate::settings::Settings;
use std::fmt::Write;

pub const MAX_MENU_ITEMS: usize = 12;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ConfirmKind {
    OverwriteSave(usize),
    DeleteSave(usize),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MenuScreen {
    Main,
    Pause,
    SaveSlots,
    LoadSlots,
    Settings,
    Controls,
    GameOver,
    Confirm(ConfirmKind),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SettingId {
    MouseSensitivity,
    FovDegrees,
    Shadows,
    RenderScale,
    PresentMode,
    HudScale,
    TitleStats,
    MasterVolume,
    MusicVolume,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MenuAction {
    None,
    Continue,
    NewGame,
    OpenLoad,
    OpenSave,
    OpenSettings,
    OpenControls,
    ResumeGame,
    SaveToSlot(usize),
    LoadSlot(usize),
    DeleteSlot(usize),
    AdjustSetting(SettingId, i32),
    ResetSettings,
    BackToMainMenu,
    Quit,
    Retry,
    LoadLast,
    Back,
}

#[derive(Clone, Debug)]
pub struct MenuItem {
    pub label: String,
    pub enabled: bool,
    pub corrupt: bool,
    pub action: MenuAction,
}

impl Default for MenuItem {
    fn default() -> Self {
        Self {
            label: String::new(),
            enabled: false,
            corrupt: false,
            action: MenuAction::None,
        }
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct MenuRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl MenuRect {
    #[inline]
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

#[derive(Copy, Clone, Debug)]
pub struct MenuLayout {
    pub panel: MenuRect,
    pub title_y: f32,
    pub subtitle_y: f32,
    pub rows: [MenuRect; MAX_MENU_ITEMS],
    pub row_count: usize,
}

/// Pure, allocation-free layout function computing centered menu panel and item rectangles.
pub fn layout(screen_w: u32, screen_h: u32, hud_scale: u32, item_count: usize) -> MenuLayout {
    let sw = (screen_w.max(1)) as f32;
    let sh = (screen_h.max(1)) as f32;
    let count = item_count.min(MAX_MENU_ITEMS);
    let hs = (hud_scale.clamp(1, 3)) as f32;

    let max_avail_h = (sh - 24.0).max(200.0);
    let header_space = (64.0 + hs * 16.0).min(max_avail_h * 0.26);
    let avail_rows_h = (max_avail_h - header_space - 20.0).max(100.0);

    let ideal_row_h = 22.0 + hs * 6.0;
    let ideal_gap = 4.0 + hs * 1.5;
    let total_ideal = (count.max(1) as f32) * (ideal_row_h + ideal_gap);

    let fit_scale = if total_ideal > avail_rows_h {
        (avail_rows_h / total_ideal).clamp(0.45, 1.0)
    } else {
        1.0
    };

    let row_h = (ideal_row_h * fit_scale).max(16.0);
    let gap = (ideal_gap * fit_scale).max(2.0);
    let rows_total_h = (count as f32) * row_h + (count.saturating_sub(1) as f32) * gap;

    let panel_w = (460.0 + hs * 80.0).min((sw - 24.0).max(240.0));
    let panel_h = (header_space + rows_total_h + 24.0).min(max_avail_h);
    let panel_x = ((sw - panel_w) * 0.5).max(12.0);
    let panel_y = ((sh - panel_h) * 0.5).max(12.0);

    let title_y = panel_y + 12.0;
    let subtitle_y = title_y + 22.0 + hs * 6.0;
    let start_row_y = panel_y + header_space;

    let mut rows = [MenuRect::default(); MAX_MENU_ITEMS];
    let row_w = (panel_w - 32.0).max(100.0);
    let row_x = panel_x + 16.0;

    for i in 0..count {
        let ry = start_row_y + (i as f32) * (row_h + gap);
        rows[i] = MenuRect {
            x: row_x,
            y: ry,
            w: row_w,
            h: row_h,
        };
    }

    MenuLayout {
        panel: MenuRect {
            x: panel_x,
            y: panel_y,
            w: panel_w,
            h: panel_h,
        },
        title_y,
        subtitle_y,
        rows,
        row_count: count,
    }
}

pub struct MenuSystem {
    stack: [MenuScreen; 8],
    depth: usize,
    pub selected: usize,
    pub items: [MenuItem; MAX_MENU_ITEMS],
    pub item_count: usize,
    pub cursor_pos: (f32, f32),
}

impl Default for MenuSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl MenuSystem {
    pub fn new() -> Self {
        let mut sys = Self {
            stack: [MenuScreen::Main; 8],
            depth: 1,
            selected: 0,
            items: std::array::from_fn(|_| MenuItem::default()),
            item_count: 0,
            cursor_pos: (0.0, 0.0),
        };
        let empty_saves = [
            SlotStatus::Empty,
            SlotStatus::Empty,
            SlotStatus::Empty,
            SlotStatus::Empty,
            SlotStatus::Empty,
        ];
        sys.rebuild_items(&Settings::default(), &empty_saves);
        sys
    }

    pub fn current_screen(&self) -> MenuScreen {
        self.stack[self.depth.saturating_sub(1)]
    }

    pub fn open_root(
        &mut self,
        screen: MenuScreen,
        settings: &Settings,
        saves: &[SlotStatus; 5],
    ) {
        self.stack[0] = screen;
        self.depth = 1;
        self.selected = 0;
        self.rebuild_items(settings, saves);
        self.ensure_valid_selection();
    }

    pub fn push_screen(
        &mut self,
        screen: MenuScreen,
        settings: &Settings,
        saves: &[SlotStatus; 5],
    ) {
        if self.depth < self.stack.len() {
            self.stack[self.depth] = screen;
            self.depth += 1;
            self.selected = 0;
            self.rebuild_items(settings, saves);
            self.ensure_valid_selection();
        }
    }

    /// Pops the current sub-screen; returns `true` if a sub-screen was popped, or `false` if already at root.
    pub fn pop_screen(&mut self, settings: &Settings, saves: &[SlotStatus; 5]) -> bool {
        if self.depth > 1 {
            self.depth -= 1;
            self.selected = 0;
            self.rebuild_items(settings, saves);
            self.ensure_valid_selection();
            true
        } else {
            false
        }
    }

    pub fn move_up(&mut self) {
        if self.item_count == 0 {
            return;
        }
        for _ in 0..self.item_count {
            self.selected = (self.selected + self.item_count - 1) % self.item_count;
            if self.items[self.selected].enabled {
                break;
            }
        }
    }

    pub fn move_down(&mut self) {
        if self.item_count == 0 {
            return;
        }
        for _ in 0..self.item_count {
            self.selected = (self.selected + 1) % self.item_count;
            if self.items[self.selected].enabled {
                break;
            }
        }
    }

    pub fn activate_selected(
        &mut self,
        settings: &Settings,
        saves: &[SlotStatus; 5],
    ) -> MenuAction {
        if self.selected >= self.item_count || !self.items[self.selected].enabled {
            return MenuAction::None;
        }
        let act = self.items[self.selected].action;

        // Saving over an existing non-empty slot asks Confirm("Overwrite slot N?")
        if let MenuAction::SaveToSlot(slot_idx) = act {
            if self.current_screen() == MenuScreen::SaveSlots
                && slot_idx < saves.len()
                && saves[slot_idx] != SlotStatus::Empty
            {
                self.push_screen(
                    MenuScreen::Confirm(ConfirmKind::OverwriteSave(slot_idx)),
                    settings,
                    saves,
                );
                return MenuAction::None;
            }
        }
        act
    }

    pub fn request_delete_selected(
        &mut self,
        settings: &Settings,
        saves: &[SlotStatus; 5],
    ) -> bool {
        let scr = self.current_screen();
        if !matches!(scr, MenuScreen::SaveSlots | MenuScreen::LoadSlots) {
            return false;
        }
        if self.selected < saves.len() && saves[self.selected] != SlotStatus::Empty {
            self.push_screen(
                MenuScreen::Confirm(ConfirmKind::DeleteSave(self.selected)),
                settings,
                saves,
            );
            return true;
        }
        false
    }

    pub fn adjust_selected(&self, delta: i32) -> MenuAction {
        if self.current_screen() != MenuScreen::Settings || self.selected >= self.item_count {
            return MenuAction::None;
        }
        if let MenuAction::AdjustSetting(id, _) = self.items[self.selected].action {
            MenuAction::AdjustSetting(id, delta)
        } else {
            MenuAction::None
        }
    }

    pub fn on_mouse_move(&mut self, x: f32, y: f32, menu_layout: &MenuLayout) -> bool {
        self.cursor_pos = (x, y);
        for i in 0..menu_layout.row_count.min(self.item_count) {
            if menu_layout.rows[i].contains(x, y) && self.items[i].enabled {
                self.selected = i;
                return true;
            }
        }
        false
    }

    pub fn on_mouse_click(
        &mut self,
        x: f32,
        y: f32,
        menu_layout: &MenuLayout,
        settings: &Settings,
        saves: &[SlotStatus; 5],
    ) -> MenuAction {
        for i in 0..menu_layout.row_count.min(self.item_count) {
            if menu_layout.rows[i].contains(x, y) && self.items[i].enabled {
                self.selected = i;
                if let MenuAction::AdjustSetting(id, _) = self.items[i].action {
                    let mid_x = menu_layout.rows[i].x + menu_layout.rows[i].w * 0.5;
                    let delta = if x < mid_x { -1 } else { 1 };
                    return MenuAction::AdjustSetting(id, delta);
                }
                return self.activate_selected(settings, saves);
            }
        }
        MenuAction::None
    }

    fn ensure_valid_selection(&mut self) {
        if self.item_count == 0 {
            self.selected = 0;
            return;
        }
        if self.selected >= self.item_count {
            self.selected = 0;
        }
        if !self.items[self.selected].enabled {
            for i in 0..self.item_count {
                if self.items[i].enabled {
                    self.selected = i;
                    break;
                }
            }
        }
    }

    fn set_item(&mut self, idx: usize, label: &str, enabled: bool, corrupt: bool, action: MenuAction) {
        let item = &mut self.items[idx];
        item.label.clear();
        item.label.push_str(label);
        item.enabled = enabled;
        item.corrupt = corrupt;
        item.action = action;
    }

    pub fn rebuild_items(&mut self, settings: &Settings, saves: &[SlotStatus; 5]) {
        let has_any_valid_save = most_recent_save_slot(saves).is_some();

        match self.current_screen() {
            MenuScreen::Main => {
                self.item_count = 6;
                self.set_item(0, "Continue", has_any_valid_save, false, MenuAction::Continue);
                self.set_item(1, "New Game", true, false, MenuAction::NewGame);
                self.set_item(2, "Load Game", true, false, MenuAction::OpenLoad);
                self.set_item(3, "Settings", true, false, MenuAction::OpenSettings);
                self.set_item(4, "Controls", true, false, MenuAction::OpenControls);
                self.set_item(5, "Quit", true, false, MenuAction::Quit);
            }
            MenuScreen::Pause => {
                self.item_count = 7;
                self.set_item(0, "Resume", true, false, MenuAction::ResumeGame);
                self.set_item(1, "Save Game", true, false, MenuAction::OpenSave);
                self.set_item(2, "Load Game", true, false, MenuAction::OpenLoad);
                self.set_item(3, "Settings", true, false, MenuAction::OpenSettings);
                self.set_item(4, "Controls", true, false, MenuAction::OpenControls);
                self.set_item(5, "Main Menu", true, false, MenuAction::BackToMainMenu);
                self.set_item(6, "Quit", true, false, MenuAction::Quit);
            }
            MenuScreen::SaveSlots => {
                self.item_count = 4;
                let mut buf = String::with_capacity(80);
                for i in 0..3 {
                    format_slot_row(i, &saves[i], &mut buf);
                    self.set_item(i, &buf, true, saves[i] == SlotStatus::Corrupt, MenuAction::SaveToSlot(i));
                }
                self.set_item(3, "Back", true, false, MenuAction::Back);
            }
            MenuScreen::LoadSlots => {
                self.item_count = 6;
                let mut buf = String::with_capacity(80);
                for i in 0..5 {
                    format_slot_row(i, &saves[i], &mut buf);
                    let valid = matches!(saves[i], SlotStatus::Valid(_));
                    let corrupt = saves[i] == SlotStatus::Corrupt;
                    self.set_item(i, &buf, valid, corrupt, MenuAction::LoadSlot(i));
                }
                self.set_item(5, "Back", true, false, MenuAction::Back);
            }
            MenuScreen::Settings => {
                self.item_count = 11;
                let mut buf = String::with_capacity(64);

                buf.clear();
                let _ = write!(&mut buf, "Mouse Sensitivity       [{:.1}]", settings.mouse_sensitivity);
                self.set_item(0, &buf, true, false, MenuAction::AdjustSetting(SettingId::MouseSensitivity, 1));

                buf.clear();
                let _ = write!(&mut buf, "Field of View           [{:.0} deg]", settings.fov_degrees);
                self.set_item(1, &buf, true, false, MenuAction::AdjustSetting(SettingId::FovDegrees, 1));

                buf.clear();
                let _ = write!(&mut buf, "Shadows                 [{}]", settings.shadow_quality.label());
                self.set_item(2, &buf, true, false, MenuAction::AdjustSetting(SettingId::Shadows, 1));

                buf.clear();
                let _ = write!(&mut buf, "Render Scale            [{:.2}x]", settings.render_scale);
                self.set_item(3, &buf, true, false, MenuAction::AdjustSetting(SettingId::RenderScale, 1));

                buf.clear();
                let _ = write!(&mut buf, "Present Mode            [{}]", settings.present_mode.label());
                self.set_item(4, &buf, true, false, MenuAction::AdjustSetting(SettingId::PresentMode, 1));

                buf.clear();
                let _ = write!(&mut buf, "HUD Scale               [{}x]", settings.hud_scale);
                self.set_item(5, &buf, true, false, MenuAction::AdjustSetting(SettingId::HudScale, 1));

                buf.clear();
                let _ = write!(&mut buf, "Title Stats             [{}]", if settings.show_title_stats { "On" } else { "Off" });
                self.set_item(6, &buf, true, false, MenuAction::AdjustSetting(SettingId::TitleStats, 1));

                buf.clear();
                let _ = write!(&mut buf, "Master Volume           [{:.1}] (no audio yet)", settings.master_volume);
                self.set_item(7, &buf, true, false, MenuAction::AdjustSetting(SettingId::MasterVolume, 1));

                buf.clear();
                let _ = write!(&mut buf, "Music Volume            [{:.1}] (no audio yet)", settings.music_volume);
                self.set_item(8, &buf, true, false, MenuAction::AdjustSetting(SettingId::MusicVolume, 1));

                self.set_item(9, "Reset to Defaults", true, false, MenuAction::ResetSettings);
                self.set_item(10, "Back", true, false, MenuAction::Back);
            }
            MenuScreen::Controls => {
                self.item_count = 1;
                self.set_item(0, "Back", true, false, MenuAction::Back);
            }
            MenuScreen::GameOver => {
                self.item_count = 3;
                self.set_item(0, "Retry (New Game)", true, false, MenuAction::Retry);
                self.set_item(1, "Load Last Save", has_any_valid_save, false, MenuAction::LoadLast);
                self.set_item(2, "Main Menu", true, false, MenuAction::BackToMainMenu);
            }
            MenuScreen::Confirm(kind) => {
                self.item_count = 2;
                let yes_act = match kind {
                    ConfirmKind::OverwriteSave(s) => MenuAction::SaveToSlot(s),
                    ConfirmKind::DeleteSave(s) => MenuAction::DeleteSlot(s),
                };
                self.set_item(0, "Yes, Confirm", true, false, yes_act);
                self.set_item(1, "No, Cancel", true, false, MenuAction::Back);
            }
        }
        self.ensure_valid_selection();
    }
}

fn format_slot_row(slot_idx: usize, status: &SlotStatus, out: &mut String) {
    out.clear();
    let title = SLOT_TITLES[slot_idx % SLOT_TITLES.len()];
    match status {
        SlotStatus::Empty => {
            let _ = write!(out, "{:<10} | EMPTY", title);
        }
        SlotStatus::Corrupt => {
            let _ = write!(out, "{:<10} | CORRUPT (Del to remove)", title);
        }
        SlotStatus::Valid(info) => {
            let dt_str = format_utc_datetime(info.created_unix);
            let pt_str = format_playtime_hms(info.playtime_seconds);
            let _ = write!(
                out,
                "{:<8} | {} | Wv:{} Scr:{} | {} | Al:{}(Lv{})",
                title,
                dt_str,
                info.wave,
                info.score,
                pt_str,
                info.ally_count,
                info.avg_ally_level
            );
        }
    }
}

pub fn apply_setting_adjustment(settings: &mut Settings, id: SettingId, delta: i32) {
    let dir = if delta >= 0 { 1.0 } else { -1.0 };
    match id {
        SettingId::MouseSensitivity => settings.mouse_sensitivity += 0.1 * dir,
        SettingId::FovDegrees => settings.fov_degrees += 5.0 * dir,
        SettingId::Shadows => {
            settings.shadow_quality = if delta >= 0 {
                settings.shadow_quality.cycle()
            } else {
                settings.shadow_quality.cycle().cycle()
            };
        }
        SettingId::RenderScale => settings.render_scale += 0.05 * dir,
        SettingId::PresentMode => settings.present_mode = settings.present_mode.toggle(),
        SettingId::HudScale => {
            let next = (settings.hud_scale as i32 + delta.signum()).clamp(1, 3);
            settings.hud_scale = next as u32;
        }
        SettingId::TitleStats => settings.show_title_stats = !settings.show_title_stats,
        SettingId::MasterVolume => settings.master_volume += 0.1 * dir,
        SettingId::MusicVolume => settings.music_volume += 0.1 * dir,
    }
    settings.clamp_all();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save::SlotInfo;

    #[test]
    fn test_menu_navigation_wrap_skip_disabled_and_continue_state() {
        let cfg = Settings::default();
        let empty = [
            SlotStatus::Empty,
            SlotStatus::Empty,
            SlotStatus::Empty,
            SlotStatus::Empty,
            SlotStatus::Empty,
        ];
        let mut menu = MenuSystem::new();
        menu.open_root(MenuScreen::Main, &cfg, &empty);

        // With no valid saves, Continue (index 0) is disabled and selection starts at New Game (index 1)
        assert!(!menu.items[0].enabled);
        assert_eq!(menu.selected, 1);

        // Moving up from index 1 wraps to Quit (index 5) and skips disabled index 0!
        menu.move_up();
        assert_eq!(menu.selected, 5);
        menu.move_down();
        assert_eq!(menu.selected, 1);

        // With a valid save, Continue is enabled and selected by default
        let mut with_save = empty.clone();
        with_save[0] = SlotStatus::Valid(SlotInfo {
            created_unix: 100,
            wave: 2,
            score: 40,
            playtime_seconds: 90.0,
            ally_count: 1,
            avg_ally_level: 2,
        });
        menu.open_root(MenuScreen::Main, &cfg, &with_save);
        assert!(menu.items[0].enabled);
        assert_eq!(menu.selected, 0);
        // Esc on Main pops nothing
        assert!(!menu.pop_screen(&cfg, &with_save));
    }

    #[test]
    fn test_confirm_dialog_flows_overwrite_and_delete() {
        let cfg = Settings::default();
        let mut saves = [
            SlotStatus::Empty,
            SlotStatus::Empty,
            SlotStatus::Empty,
            SlotStatus::Empty,
            SlotStatus::Empty,
        ];
        saves[0] = SlotStatus::Valid(SlotInfo {
            created_unix: 100,
            wave: 1,
            score: 10,
            playtime_seconds: 15.0,
            ally_count: 0,
            avg_ally_level: 0,
        });

        let mut menu = MenuSystem::new();
        menu.open_root(MenuScreen::Pause, &cfg, &saves);
        menu.push_screen(MenuScreen::SaveSlots, &cfg, &saves);
        assert_eq!(menu.current_screen(), MenuScreen::SaveSlots);

        // Activating existing slot 0 pushes Confirm(OverwriteSave(0))
        menu.selected = 0;
        assert_eq!(menu.activate_selected(&cfg, &saves), MenuAction::None);
        assert_eq!(
            menu.current_screen(),
            MenuScreen::Confirm(ConfirmKind::OverwriteSave(0))
        );

        // Selecting "No, Cancel" returns Back
        menu.selected = 1;
        assert_eq!(menu.activate_selected(&cfg, &saves), MenuAction::Back);
        assert!(menu.pop_screen(&cfg, &saves));
        assert_eq!(menu.current_screen(), MenuScreen::SaveSlots);

        // Pressing Delete on slot 0 pushes Confirm(DeleteSave(0)), and "Yes, Confirm" returns DeleteSlot(0)
        menu.selected = 0;
        assert!(menu.request_delete_selected(&cfg, &saves));
        assert_eq!(
            menu.current_screen(),
            MenuScreen::Confirm(ConfirmKind::DeleteSave(0))
        );
        menu.selected = 0;
        assert_eq!(
            menu.activate_selected(&cfg, &saves),
            MenuAction::DeleteSlot(0)
        );
    }

    #[test]
    fn test_settings_adjustment_and_mouse_hit_test() {
        let mut cfg = Settings::default();
        let empty = [
            SlotStatus::Empty,
            SlotStatus::Empty,
            SlotStatus::Empty,
            SlotStatus::Empty,
            SlotStatus::Empty,
        ];
        let mut menu = MenuSystem::new();
        menu.open_root(MenuScreen::Settings, &cfg, &empty);

        for i in 0..9 {
            menu.selected = i;
            let act = menu.adjust_selected(1);
            if let MenuAction::AdjustSetting(id, d) = act {
                apply_setting_adjustment(&mut cfg, id, d);
            } else {
                panic!("expected AdjustSetting for row {i}");
            }
        }
        assert!((cfg.mouse_sensitivity - 1.1).abs() < 1e-4);
        assert!((cfg.fov_degrees - 75.0).abs() < 1e-4);

        let lay = layout(1280, 720, 2, menu.item_count);
        let r3 = lay.rows[3];
        assert!(menu.on_mouse_move(r3.x + 10.0, r3.y + 5.0, &lay));
        assert_eq!(menu.selected, 3);
    }

    #[test]
    fn test_layout_inside_screen_and_non_overlapping() {
        for &(w, h) in &[(800u32, 600u32), (1280, 720), (1920, 1080)] {
            for scale in 1..=3 {
                let lay = layout(w, h, scale, 11);
                assert!(lay.panel.x >= 0.0 && lay.panel.y >= 0.0);
                assert!(lay.panel.x + lay.panel.w <= w as f32 + 1e-3);
                assert!(lay.panel.y + lay.panel.h <= h as f32 + 1e-3);

                for i in 0..lay.row_count {
                    let r = lay.rows[i];
                    assert!(r.x >= lay.panel.x && r.x + r.w <= lay.panel.x + lay.panel.w + 1e-3);
                    assert!(r.y >= lay.panel.y && r.y + r.h <= lay.panel.y + lay.panel.h + 1e-3);
                    if i + 1 < lay.row_count {
                        let next_r = lay.rows[i + 1];
                        assert!(
                            r.y + r.h <= next_r.y + 1e-3,
                            "rows {i} and {} overlapped at {}x{} scale {scale}",
                            i + 1,
                            w,
                            h
                        );
                    }
                }
            }
        }
    }
}
