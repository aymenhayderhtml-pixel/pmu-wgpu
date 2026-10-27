pub const BURN_DPS: f32 = 4.0;
pub const BURN_DURATION: f32 = 3.0;
pub const SLOW_DURATION: f32 = 3.0;
pub const SLOW_MULTIPLIER: f32 = 0.50;

pub const BURN_TINT: [f32; 3] = [1.0, 0.55, 0.12];
pub const SLOW_TINT: [f32; 3] = [0.38, 0.82, 1.00];

#[derive(Copy, Clone, Debug, Default)]
pub struct StatusEffects {
    pub burn_timer: f32,
    pub burn_tick_accum: f32,
    pub slow_timer: f32,
}

impl StatusEffects {
    /// Applies or refreshes Burn duration (does not stack).
    pub fn apply_burn(&mut self, duration: f32) {
        if duration > self.burn_timer {
            self.burn_timer = duration;
        }
    }

    /// Applies or refreshes Slow duration (does not stack).
    pub fn apply_slow(&mut self, duration: f32) {
        if duration > self.slow_timer {
            self.slow_timer = duration;
        }
    }

    pub fn is_burning(&self) -> bool {
        self.burn_timer > 0.0
    }

    pub fn is_slowed(&self) -> bool {
        self.slow_timer > 0.0
    }

    pub fn speed_multiplier(&self) -> f32 {
        if self.slow_timer > 0.0 {
            SLOW_MULTIPLIER
        } else {
            1.0
        }
    }

    /// Ticks status timers for `dt`. Returns `(burn_damage_this_step, optional_1s_popup_number)`.
    pub fn update(&mut self, dt: f32) -> (f32, Option<u32>) {
        let mut dmg = 0.0;
        let mut popup = None;

        if self.burn_timer > 0.0 {
            let active_dt = dt.min(self.burn_timer);
            self.burn_timer = (self.burn_timer - dt).max(0.0);
            dmg = BURN_DPS * active_dt;
            self.burn_tick_accum += active_dt;
            if self.burn_tick_accum >= 0.999 {
                self.burn_tick_accum -= 1.0;
                popup = Some(BURN_DPS.round() as u32);
            }
            if self.burn_timer <= 0.0 {
                self.burn_tick_accum = 0.0;
            }
        }

        if self.slow_timer > 0.0 {
            self.slow_timer = (self.slow_timer - dt).max(0.0);
        }

        (dmg, popup)
    }

    pub fn mix_tint(&self, base: [f32; 3]) -> [f32; 3] {
        let mut c = base;
        if self.is_burning() {
            c = [
                c[0] * 0.45 + BURN_TINT[0] * 0.55,
                c[1] * 0.45 + BURN_TINT[1] * 0.55,
                c[2] * 0.45 + BURN_TINT[2] * 0.55,
            ];
        }
        if self.is_slowed() {
            c = [
                c[0] * 0.45 + SLOW_TINT[0] * 0.55,
                c[1] * 0.45 + SLOW_TINT[1] * 0.55,
                c[2] * 0.45 + SLOW_TINT[2] * 0.55,
            ];
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_burn_and_slow_tick_and_refresh() {
        let mut st = StatusEffects::default();
        assert_eq!(st.speed_multiplier(), 1.0);

        st.apply_burn(BURN_DURATION);
        st.apply_slow(SLOW_DURATION);
        assert!(st.is_burning());
        assert!(st.is_slowed());
        assert!((st.speed_multiplier() - SLOW_MULTIPLIER).abs() < 1e-5);

        // Tick 1.0s: 4.0 burn damage and 1 popup of 4
        let (dmg1, pop1) = st.update(1.0);
        assert!((dmg1 - 4.0).abs() < 1e-4);
        assert_eq!(pop1, Some(4));
        assert!((st.burn_timer - 2.0).abs() < 1e-4);

        // Reapplying refreshes duration to 3.0s (does not stack to 5.0s)
        st.apply_burn(BURN_DURATION);
        assert!((st.burn_timer - 3.0).abs() < 1e-4);

        // Tick 3.0s to expiration
        for _ in 0..3 {
            let _ = st.update(1.0);
        }
        assert!(!st.is_burning());
        assert!(!st.is_slowed());
        assert_eq!(st.speed_multiplier(), 1.0);
    }
}
