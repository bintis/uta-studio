use bevy::prelude::*;

/// Uta! Studio's native Roon-inspired color system.
#[derive(Clone, Resource)]
pub struct StudioTheme {
    pub dark: bool,
    pub background: Color,
    pub foreground: Color,
    pub card: Color,
    pub muted: Color,
    pub muted_foreground: Color,
    pub primary: Color,
    pub primary_foreground: Color,
    pub border: Color,
    pub sidebar: Color,
    pub sidebar_foreground: Color,
    pub sidebar_accent: Color,
    pub topbar: Color,
    pub destructive: Color,
    pub waveform: Color,
    pub pitch_contour: Color,
    pub note_normal: Color,
    pub editor_selection: Color,
    pub editor_warning: Color,
}

impl StudioTheme {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                dark,
                background: Color::srgb(0.055, 0.058, 0.075),
                foreground: Color::srgb(0.92, 0.925, 0.95),
                card: Color::srgb(0.085, 0.088, 0.11),
                muted: Color::srgb(0.115, 0.118, 0.15),
                muted_foreground: Color::srgb(0.64, 0.645, 0.7),
                primary: Color::srgb(0.62, 0.59, 1.0),
                primary_foreground: Color::srgb(0.065, 0.06, 0.12),
                border: Color::srgb(0.19, 0.195, 0.235),
                sidebar: Color::srgba(0.068, 0.07, 0.09, 0.88),
                sidebar_foreground: Color::srgb(0.9, 0.905, 0.94),
                sidebar_accent: Color::srgb(0.155, 0.15, 0.225),
                topbar: Color::srgba(0.055, 0.058, 0.075, 0.72),
                destructive: Color::srgb(0.82, 0.22, 0.22),
                waveform: Color::srgb(0.48, 0.52, 0.62),
                pitch_contour: Color::srgb(0.72, 0.75, 1.0),
                note_normal: Color::srgb(0.31, 0.39, 0.66),
                editor_selection: Color::srgb(1.0, 0.68, 0.25),
                editor_warning: Color::srgb(0.94, 0.62, 0.18),
            }
        } else {
            Self {
                dark,
                background: Color::srgb(0.965, 0.969, 0.98),
                foreground: Color::srgb(0.15, 0.155, 0.19),
                card: Color::srgb(0.995, 0.996, 1.0),
                muted: Color::srgb(0.935, 0.94, 0.96),
                muted_foreground: Color::srgb(0.46, 0.465, 0.52),
                primary: Color::srgb(0.51, 0.48, 0.96),
                primary_foreground: Color::srgb(1.0, 1.0, 1.0),
                border: Color::srgb(0.865, 0.87, 0.905),
                sidebar: Color::srgba(0.955, 0.959, 0.975, 0.84),
                sidebar_foreground: Color::srgb(0.18, 0.185, 0.22),
                sidebar_accent: Color::srgb(0.9, 0.9, 0.97),
                topbar: Color::srgba(0.965, 0.969, 0.98, 0.72),
                destructive: Color::srgb(0.82, 0.22, 0.22),
                waveform: Color::srgb(0.39, 0.43, 0.51),
                pitch_contour: Color::srgb(0.2, 0.27, 0.55),
                note_normal: Color::srgb(0.3, 0.4, 0.68),
                editor_selection: Color::srgb(0.88, 0.45, 0.08),
                editor_warning: Color::srgb(0.76, 0.39, 0.05),
            }
        }
    }

    /// `opacity_percent` is the Settings "Window opacity" value (30-100,
    /// see `AppConfig::window_opacity_percent`). While `transparent` is on,
    /// it scales `background`, `sidebar`, and `topbar` together by the same
    /// factor: `sidebar`/`topbar` keep their own baseline translucency
    /// (they're meant to read as slightly more solid chrome than the
    /// content canvas even at full opacity) but move with the slider
    /// instead of staying fixed while `background` becomes near-invisible,
    /// which otherwise leaves the sidebar/top bar looking like a mismatched
    /// solid panel over a barely-there window at high transparency.
    pub fn new_with_transparency(dark: bool, transparent: bool, opacity_percent: u32) -> Self {
        let mut theme = Self::new(dark);
        if transparent {
            let factor = opacity_percent.clamp(30, 100) as f32 / 100.0;
            theme.background = theme.background.with_alpha(factor);
            theme.sidebar = theme
                .sidebar
                .with_alpha(theme.sidebar.alpha() * factor);
            theme.topbar = theme.topbar.with_alpha(theme.topbar.alpha() * factor);
        }
        theme
    }
}

pub fn window_clear_color(theme: &StudioTheme, transparent: bool) -> Color {
    if transparent {
        Color::NONE
    } else {
        theme.background
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_srgb(color: Color, expected: [f32; 4]) {
        let actual = color.to_srgba().to_f32_array();
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
        }
    }

    #[test]
    fn native_dark_tokens_match_the_current_ui() {
        let theme = StudioTheme::new(true);
        assert_srgb(theme.background, [0.055, 0.058, 0.075, 1.0]);
        assert_srgb(theme.foreground, [0.92, 0.925, 0.95, 1.0]);
        assert_srgb(theme.primary, [0.62, 0.59, 1.0, 1.0]);
    }

    #[test]
    fn native_light_tokens_match_the_current_ui() {
        let theme = StudioTheme::new(false);
        assert_srgb(theme.background, [0.965, 0.969, 0.98, 1.0]);
        assert_srgb(theme.foreground, [0.15, 0.155, 0.19, 1.0]);
        assert_srgb(theme.primary, [0.51, 0.48, 0.96, 1.0]);
    }

    #[test]
    fn transparent_theme_keeps_content_readable_over_a_clear_surface() {
        let theme = StudioTheme::new_with_transparency(true, true, 86);
        assert_srgb(theme.background, [0.055, 0.058, 0.075, 0.86]);
        assert_srgb(window_clear_color(&theme, true), [0.0, 0.0, 0.0, 0.0]);
        assert_srgb(theme.foreground, [0.92, 0.925, 0.95, 1.0]);
    }

    /// Confirmed against a real report: at high transparency, the sidebar
    /// and top bar stayed at their fixed baseline alpha while `background`
    /// dropped toward the slider's low end, so the chrome panels looked
    /// like solid mismatched slabs over a nearly invisible window. Sidebar
    /// and top bar must move with the same slider, not sit fixed.
    #[test]
    fn sidebar_and_topbar_scale_down_together_with_background_at_high_transparency() {
        let opaque = StudioTheme::new(true);
        let transparent = StudioTheme::new_with_transparency(true, true, 30);
        assert_srgb(transparent.background, [0.055, 0.058, 0.075, 0.30]);
        assert_srgb(
            transparent.sidebar,
            [0.068, 0.07, 0.09, opaque.sidebar.alpha() * 0.30],
        );
        assert_srgb(
            transparent.topbar,
            [0.055, 0.058, 0.075, opaque.topbar.alpha() * 0.30],
        );
        // At full-strength opacity the chrome keeps today's baseline look.
        let full = StudioTheme::new_with_transparency(true, true, 100);
        assert_srgb(full.sidebar, [0.068, 0.07, 0.09, opaque.sidebar.alpha()]);
        assert_srgb(full.topbar, [0.055, 0.058, 0.075, opaque.topbar.alpha()]);
    }

    #[test]
    fn transparency_opacity_is_clamped_and_ignored_when_transparency_is_off() {
        let theme = StudioTheme::new_with_transparency(true, true, 5);
        assert_srgb(theme.background, [0.055, 0.058, 0.075, 0.30]);
        let theme = StudioTheme::new_with_transparency(true, true, 250);
        assert_srgb(theme.background, [0.055, 0.058, 0.075, 1.0]);
        let theme = StudioTheme::new_with_transparency(true, false, 50);
        assert_srgb(theme.background, [0.055, 0.058, 0.075, 1.0]);
    }
}
