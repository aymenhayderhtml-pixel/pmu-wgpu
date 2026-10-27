#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GameFlow {
    MainMenu,
    Playing,
    PauseMenu,
    SkillMenu,
    GameOver,
}

impl GameFlow {
    #[inline]
    pub fn is_paused(self) -> bool {
        self != Self::Playing
    }

    #[inline]
    pub fn blocks_world_update(self) -> bool {
        self != Self::Playing
    }

    #[inline]
    pub fn mouse_should_be_captured(self) -> bool {
        self == Self::Playing
    }

    #[inline]
    pub fn label(self) -> &'static str {
        match self {
            Self::MainMenu => "MainMenu",
            Self::Playing => "Playing",
            Self::PauseMenu => "PauseMenu",
            Self::SkillMenu => "SkillMenu",
            Self::GameOver => "GameOver",
        }
    }

    #[inline]
    pub fn on_start_or_resume(self) -> Self {
        Self::Playing
    }

    /// Esc while Playing opens PauseMenu; Esc while SkillMenu or PauseMenu returns to Playing
    /// (unless a sub-menu screen is on the stack, handled before calling this).
    /// Esc on MainMenu or GameOver never quits the app.
    #[inline]
    pub fn on_escape(self) -> Self {
        match self {
            Self::Playing => Self::PauseMenu,
            Self::PauseMenu | Self::SkillMenu => Self::Playing,
            Self::MainMenu => Self::MainMenu,
            Self::GameOver => Self::GameOver,
        }
    }

    #[inline]
    pub fn on_toggle_skill_menu(self) -> Self {
        match self {
            Self::Playing => Self::SkillMenu,
            Self::SkillMenu => Self::Playing,
            other => other,
        }
    }

    #[inline]
    pub fn on_focus_lost(self) -> Self {
        match self {
            Self::Playing | Self::SkillMenu => Self::PauseMenu,
            other => other,
        }
    }

    #[inline]
    pub fn on_player_death(self) -> Self {
        match self {
            Self::Playing | Self::SkillMenu | Self::PauseMenu => Self::GameOver,
            other => other,
        }
    }

    #[inline]
    pub fn on_retry(self) -> Self {
        Self::Playing
    }

    #[inline]
    pub fn on_back_to_main(self) -> Self {
        Self::MainMenu
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_game_flow_transitions_and_pause_rules() {
        let mut flow = GameFlow::MainMenu;
        assert!(flow.is_paused());
        assert!(flow.blocks_world_update());
        assert!(!flow.mouse_should_be_captured());

        // Esc on MainMenu never quits or changes state
        assert_eq!(flow.on_escape(), GameFlow::MainMenu);

        // MainMenu -> Playing
        flow = flow.on_start_or_resume();
        assert_eq!(flow, GameFlow::Playing);
        assert!(!flow.is_paused());
        assert!(!flow.blocks_world_update());
        assert!(flow.mouse_should_be_captured());

        // Playing -> PauseMenu -> Playing
        flow = flow.on_escape();
        assert_eq!(flow, GameFlow::PauseMenu);
        assert!(flow.is_paused());
        flow = flow.on_escape();
        assert_eq!(flow, GameFlow::Playing);

        // Playing -> SkillMenu -> Playing
        flow = flow.on_toggle_skill_menu();
        assert_eq!(flow, GameFlow::SkillMenu);
        assert!(flow.blocks_world_update());
        flow = flow.on_toggle_skill_menu();
        assert_eq!(flow, GameFlow::Playing);

        // Focus loss while Playing opens PauseMenu
        flow = flow.on_focus_lost();
        assert_eq!(flow, GameFlow::PauseMenu);

        // Player death -> GameOver -> Retry -> Playing
        flow = flow.on_start_or_resume().on_player_death();
        assert_eq!(flow, GameFlow::GameOver);
        assert_eq!(flow.on_escape(), GameFlow::GameOver);
        flow = flow.on_retry();
        assert_eq!(flow, GameFlow::Playing);
        flow = flow.on_back_to_main();
        assert_eq!(flow, GameFlow::MainMenu);
    }
}
