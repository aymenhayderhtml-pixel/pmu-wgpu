use crate::skill_inv::{SkillInventory, FUSION_RECIPES, SLOT_COUNT};
use crate::skills::{level_multiplier, SkillId, SkillKind, SKILL_COUNT, SKILL_TABLE};
use std::fmt::Write;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GameState {
    Playing,
    Paused,
}

impl GameState {
    #[inline]
    pub fn is_paused(self) -> bool {
        self == Self::Paused
    }

    #[inline]
    pub fn label(self) -> &'static str {
        match self {
            Self::Playing => "Playing",
            Self::Paused => "Paused",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MenuSection {
    Skills,
    Slots,
    Recipes,
}

impl MenuSection {
    pub fn next(self) -> Self {
        match self {
            Self::Skills => Self::Slots,
            Self::Slots => Self::Recipes,
            Self::Recipes => Self::Skills,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Skills => Self::Recipes,
            Self::Slots => Self::Skills,
            Self::Recipes => Self::Slots,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EscapeOutcome {
    ClosedMenu,
    ReleaseMouse,
    ExitApp,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RecipeStatus {
    pub src_a: SkillId,
    pub src_b: SkillId,
    pub result: SkillId,
    pub craftable: bool,
    pub missing_a: u8,
    pub missing_b: u8,
}

#[derive(Clone, Debug)]
pub struct MenuState {
    pub open: bool,
    pub section: MenuSection,
    pub selected_skill: usize,
    pub selected_slot: usize,
    pub selected_recipe: usize,
}

impl Default for MenuState {
    fn default() -> Self {
        Self::new()
    }
}

impl MenuState {
    pub fn new() -> Self {
        Self {
            open: false,
            section: MenuSection::Skills,
            selected_skill: 0,
            selected_slot: 0,
            selected_recipe: 0,
        }
    }

    pub fn toggle(&mut self, game_state: &mut GameState) {
        self.open = !self.open;
        *game_state = if self.open {
            GameState::Paused
        } else {
            GameState::Playing
        };
    }

    pub fn close(&mut self, game_state: &mut GameState) {
        self.open = false;
        *game_state = GameState::Playing;
    }

    pub fn handle_escape(
        &mut self,
        game_state: &mut GameState,
        mouse_captured: bool,
    ) -> EscapeOutcome {
        if self.open {
            self.close(game_state);
            EscapeOutcome::ClosedMenu
        } else if mouse_captured {
            EscapeOutcome::ReleaseMouse
        } else {
            EscapeOutcome::ExitApp
        }
    }

    pub fn next_section(&mut self) {
        self.section = self.section.next();
    }

    pub fn prev_section(&mut self) {
        self.section = self.section.prev();
    }

    pub fn move_up(&mut self) {
        match self.section {
            MenuSection::Skills => {
                self.selected_skill = (self.selected_skill + SKILL_COUNT - 1) % SKILL_COUNT;
            }
            MenuSection::Slots => {
                self.selected_slot = (self.selected_slot + SLOT_COUNT - 1) % SLOT_COUNT;
            }
            MenuSection::Recipes => {
                let n = FUSION_RECIPES.len();
                self.selected_recipe = (self.selected_recipe + n - 1) % n;
            }
        }
    }

    pub fn move_down(&mut self) {
        match self.section {
            MenuSection::Skills => {
                self.selected_skill = (self.selected_skill + 1) % SKILL_COUNT;
            }
            MenuSection::Slots => {
                self.selected_slot = (self.selected_slot + 1) % SLOT_COUNT;
            }
            MenuSection::Recipes => {
                let n = FUSION_RECIPES.len();
                self.selected_recipe = (self.selected_recipe + 1) % n;
            }
        }
    }

    pub fn selected_skill_id(&self) -> SkillId {
        SKILL_TABLE[self.selected_skill % SKILL_COUNT].id
    }

    /// Assigns the currently selected skill to `target_slot` (`0..4`).
    /// Locked skills (`count == 0`) cannot be assigned. If the skill is already in another slot,
    /// the two slots are swapped so a skill is never duplicated across two slots.
    pub fn assign_selected_to_slot(&mut self, inv: &mut SkillInventory, target_slot: usize) -> bool {
        if target_slot >= SLOT_COUNT {
            return false;
        }
        let skill = self.selected_skill_id();
        if inv.count(skill) == 0 {
            return false;
        }

        if let Some(existing_slot) = inv.slots.iter().position(|&s| s == Some(skill)) {
            if existing_slot != target_slot {
                inv.slots.swap(existing_slot, target_slot);
                inv.cooldowns.swap(existing_slot, target_slot);
            }
        } else {
            inv.slots[target_slot] = Some(skill);
        }
        self.selected_slot = target_slot;
        true
    }

    /// Empties `selected_slot` (or if in Skills section, removes `selected_skill` from any slot).
    pub fn clear_slot(&self, inv: &mut SkillInventory) -> bool {
        match self.section {
            MenuSection::Slots => {
                if inv.slots[self.selected_slot].is_some() {
                    inv.slots[self.selected_slot] = None;
                    return true;
                }
                false
            }
            _ => {
                let sk = self.selected_skill_id();
                if let Some(pos) = inv.slots.iter().position(|&s| s == Some(sk)) {
                    inv.slots[pos] = None;
                    return true;
                }
                if inv.slots[self.selected_slot].is_some() {
                    inv.slots[self.selected_slot] = None;
                    return true;
                }
                false
            }
        }
    }

    /// Evaluates craftability and missing copy counts for all 4 fusion recipes.
    pub fn recipe_statuses(inv: &SkillInventory) -> [RecipeStatus; 4] {
        let mut out = [RecipeStatus {
            src_a: SkillId::Fireball,
            src_b: SkillId::IceShard,
            result: SkillId::SteamBurst,
            craftable: false,
            missing_a: 0,
            missing_b: 0,
        }; 4];

        for (i, &(src_a, src_b, result)) in FUSION_RECIPES.iter().enumerate() {
            let ca = inv.count(src_a);
            let cb = inv.count(src_b);
            let missing_a = 2u8.saturating_sub(ca);
            let missing_b = 2u8.saturating_sub(cb);
            out[i] = RecipeStatus {
                src_a,
                src_b,
                result,
                craftable: missing_a == 0 && missing_b == 0,
                missing_a,
                missing_b,
            };
        }
        out
    }

    /// Pressing Enter in the Recipes section fuses `selected_recipe` if craftable.
    pub fn try_fuse_selected_recipe(
        &self,
        inv: &mut SkillInventory,
    ) -> Result<(SkillId, SkillId, SkillId), &'static str> {
        if self.section != MenuSection::Recipes {
            return Err("Select Recipes section to fuse with Enter");
        }
        let idx = self.selected_recipe % FUSION_RECIPES.len();
        let (a, b, result) = FUSION_RECIPES[idx];
        if inv.count(a) < 2 || inv.count(b) < 2 {
            return Err("Need 2 copies of each skill to fuse");
        }
        inv.counts[a.index()] -= 1;
        inv.counts[b.index()] -= 1;
        let _ = inv.add_skill_copy(result);
        Ok((a, b, result))
    }
}

/// Formats scaled skill details for the right-hand pane of the skill menu.
pub fn format_skill_details(skill: SkillId, level: u8, out_lines: &mut [String; 4]) {
    for line in out_lines.iter_mut() {
        line.clear();
    }
    let def = skill.def();
    let eff_lvl = level.max(1);
    let mult = level_multiplier(eff_lvl);
    let scaled_power = def.power * mult;

    let _ = write!(
        &mut out_lines[0],
        "{} (Tier {} | Level {})",
        def.name, def.tier, eff_lvl
    );
    let _ = write!(
        &mut out_lines[1],
        "Mana: {:.0}   Cooldown: {:.1}s",
        def.mana_cost, def.cooldown
    );

    match def.kind {
        SkillKind::Projectile {
            pierce,
            explosion_radius,
            applies_burn,
            applies_slow,
        } => {
            let _ = write!(
                &mut out_lines[2],
                "Dmg: {:.0}  Spd: {:.0}  AoE: {:.1}m  Pierce: {}",
                scaled_power, def.speed, explosion_radius, pierce
            );
            let status_str = match (applies_burn, applies_slow) {
                (true, _) => "Status: Burn (4 DPS for 3s)",
                (_, true) => "Status: Slow (0.5x speed for 3s)",
                _ => "Status: None",
            };
            out_lines[3].push_str(status_str);
        }
        SkillKind::HealPulse => {
            let _ = write!(
                &mut out_lines[2],
                "Heal: {:.0} HP  Radius: {:.1}m",
                scaled_power, def.radius
            );
            out_lines[3].push_str("Instant squad heal (living allies)");
        }
        SkillKind::Shockwave { knockback } => {
            let _ = write!(
                &mut out_lines[2],
                "AoE Dmg: {:.0}  Radius: {:.1}m",
                scaled_power, def.radius
            );
            let _ = write!(&mut out_lines[3], "Instant blast (Knockback {:.0})", knockback);
        }
        SkillKind::Dash { leaves_slow_zone } => {
            let _ = write!(
                &mut out_lines[2],
                "Dash Dist: {:.1}m in {:.2}s (Invulnerable)",
                def.radius, def.duration
            );
            if leaves_slow_zone {
                out_lines[3].push_str("Leaves 2.0s Frost Slow trail (r=2.5m)");
            } else {
                out_lines[3].push_str("Rapid collision-safe forward dash");
            }
        }
        SkillKind::Meteor {
            mark_dist,
            delay,
            knockback,
        } => {
            let _ = write!(
                &mut out_lines[2],
                "Blast Dmg: {:.0}  Radius: {:.1}m  Range: {:.0}m",
                scaled_power, def.radius, mark_dist
            );
            let _ = write!(
                &mut out_lines[3],
                "Delay {:.1}s | Knockback {:.0} + Burn",
                delay, knockback
            );
        }
        SkillKind::HolyNova {
            heal_radius,
            enemy_damage,
            enemy_radius,
            knockback,
        } => {
            let _ = write!(
                &mut out_lines[2],
                "Heal: {:.0} (r={:.0}m) | Dmg: {:.0} (r={:.0}m)",
                scaled_power,
                heal_radius,
                enemy_damage * mult,
                enemy_radius
            );
            let _ = write!(
                &mut out_lines[3],
                "Radiant pulse (Knockback {:.0})",
                knockback
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_menu_navigation_and_section_switching() {
        let mut menu = MenuState::new();
        let mut gs = GameState::Playing;

        menu.toggle(&mut gs);
        assert!(menu.open);
        assert!(gs.is_paused());

        assert_eq!(menu.section, MenuSection::Skills);
        menu.move_down();
        assert_eq!(menu.selected_skill, 1);
        menu.move_up();
        assert_eq!(menu.selected_skill, 0);
        menu.move_up();
        assert_eq!(menu.selected_skill, SKILL_COUNT - 1);

        menu.next_section();
        assert_eq!(menu.section, MenuSection::Slots);
        menu.move_down();
        assert_eq!(menu.selected_slot, 1);

        menu.next_section();
        assert_eq!(menu.section, MenuSection::Recipes);
        menu.move_down();
        assert_eq!(menu.selected_recipe, 1);

        menu.prev_section();
        assert_eq!(menu.section, MenuSection::Slots);
    }

    #[test]
    fn test_slot_assignment_swap_empty_and_locked_rule() {
        let mut inv = SkillInventory::new();
        let mut menu = MenuState::new();

        // At start: slot 0 = Fireball, slot 1 = HealPulse, IceShard (index 1) is locked (count 0)
        menu.selected_skill = SkillId::IceShard.index();
        assert!(!menu.assign_selected_to_slot(&mut inv, 2));
        assert_eq!(inv.slots[2], None);

        // Assign Fireball (currently in slot 0) to slot 1 (currently HealPulse) -> swaps slots 0 and 1!
        menu.selected_skill = SkillId::Fireball.index();
        assert!(menu.assign_selected_to_slot(&mut inv, 1));
        assert_eq!(inv.slots[0], Some(SkillId::HealPulse));
        assert_eq!(inv.slots[1], Some(SkillId::Fireball));

        // Verify no duplicate skill across slots
        let fb_count = inv
            .slots
            .iter()
            .filter(|&&s| s == Some(SkillId::Fireball))
            .count();
        assert_eq!(fb_count, 1);

        // Backspace in Slots section empties the selected slot
        menu.section = MenuSection::Slots;
        menu.selected_slot = 1;
        assert!(menu.clear_slot(&mut inv));
        assert_eq!(inv.slots[1], None);
    }

    #[test]
    fn test_recipe_craftability_and_enter_to_fuse() {
        let mut inv = SkillInventory::new();
        let mut menu = MenuState::new();
        menu.section = MenuSection::Recipes;
        menu.selected_recipe = 0; // Fireball + IceShard = SteamBurst

        let statuses_before = MenuState::recipe_statuses(&inv);
        assert!(!statuses_before[0].craftable);
        assert_eq!(statuses_before[0].missing_a, 1);
        assert_eq!(statuses_before[0].missing_b, 2);
        assert!(menu.try_fuse_selected_recipe(&mut inv).is_err());

        inv.counts[SkillId::Fireball.index()] = 2;
        inv.counts[SkillId::IceShard.index()] = 2;
        let statuses_ready = MenuState::recipe_statuses(&inv);
        assert!(statuses_ready[0].craftable);

        let (_, _, res) = menu
            .try_fuse_selected_recipe(&mut inv)
            .expect("Enter in Recipes should fuse");
        assert_eq!(res, SkillId::SteamBurst);
        assert_eq!(inv.count(SkillId::SteamBurst), 1);
        assert_eq!(inv.count(SkillId::Fireball), 1);
        assert_eq!(inv.count(SkillId::IceShard), 1);
    }

    #[test]
    fn test_escape_behavior_in_menu_vs_game() {
        let mut menu = MenuState::new();
        let mut gs = GameState::Playing;

        menu.toggle(&mut gs);
        assert!(menu.open);

        // Esc while menu is open ONLY closes the menu (does not release mouse or exit)
        let out1 = menu.handle_escape(&mut gs, true);
        assert_eq!(out1, EscapeOutcome::ClosedMenu);
        assert!(!menu.open);
        assert_eq!(gs, GameState::Playing);

        // Esc while menu is closed and mouse is captured releases mouse
        let out2 = menu.handle_escape(&mut gs, true);
        assert_eq!(out2, EscapeOutcome::ReleaseMouse);

        // Esc while menu is closed and mouse is not captured exits app
        let out3 = menu.handle_escape(&mut gs, false);
        assert_eq!(out3, EscapeOutcome::ExitApp);
    }
}
