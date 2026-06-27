use ratatui::style::Color;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    pub bg_base: Color,
    pub bg_sunken: Color,
    pub bg_surface: Color,
    pub bg_surface_alt: Color,
    pub border_subtle: Color,
    pub border_strong: Color,
    pub text_title: Color,
    pub text_primary: Color,
    pub text_body: Color,
    pub text_secondary: Color,
    pub text_muted: Color,
    pub accent_primary: Color,
    pub success_fg: Color,
    pub warning_fg: Color,
    pub warning_bg: Color,
    pub error_fg: Color,
    pub error_bg: Color,
    pub error_border: Color,
    pub info_fg: Color,
}

impl Theme {
    pub fn midnight() -> Self {
        Self {
            bg_base: rgb(0x14131c),
            bg_sunken: rgb(0x0e0d15),
            bg_surface: rgb(0x1a1925),
            bg_surface_alt: rgb(0x1e1c2c),
            border_subtle: rgb(0x2c2a3d),
            border_strong: rgb(0x3a3850),
            text_title: rgb(0xf3f1fa),
            text_primary: rgb(0xe8e6f0),
            text_body: rgb(0xd7d4e4),
            text_secondary: rgb(0x9a97b0),
            text_muted: rgb(0x6f6c86),
            accent_primary: rgb(0xb18cf0),
            success_fg: rgb(0x7ddca4),
            warning_fg: rgb(0xf0c674),
            warning_bg: rgb(0x3a3327),
            error_fg: rgb(0xf08a8a),
            error_bg: rgb(0x241a1a),
            error_border: rgb(0x5c2b2e),
            info_fg: rgb(0x6fd3e0),
        }
    }

    pub fn daylight() -> Self {
        Self {
            bg_base: rgb(0xf4f1ea),
            bg_sunken: rgb(0xefebe2),
            bg_surface: rgb(0xfffdf8),
            bg_surface_alt: rgb(0xfaf8f3),
            border_subtle: rgb(0xe0d9cb),
            border_strong: rgb(0xd6cdbb),
            text_title: rgb(0x2c2a33),
            text_primary: rgb(0x2c2a33),
            text_body: rgb(0x5a5566),
            text_secondary: rgb(0x6b6675),
            text_muted: rgb(0x9a93a6),
            accent_primary: rgb(0x6d44c0),
            success_fg: rgb(0x2f8f5b),
            warning_fg: rgb(0x9a6b12),
            warning_bg: rgb(0xfbf3e0),
            error_fg: rgb(0xb8453f),
            error_bg: rgb(0xfdf1f0),
            error_border: rgb(0xf0d4d2),
            info_fg: rgb(0x1f7a8c),
        }
    }

    pub fn token_name(&self, color: Color) -> Option<&'static str> {
        self.tokens()
            .into_iter()
            .find_map(|(name, token_color)| (token_color == color).then_some(name))
    }

    fn tokens(&self) -> [(&'static str, Color); 19] {
        [
            ("bg.base", self.bg_base),
            ("bg.sunken", self.bg_sunken),
            ("bg.surface", self.bg_surface),
            ("bg.surface_alt", self.bg_surface_alt),
            ("border.subtle", self.border_subtle),
            ("border.strong", self.border_strong),
            ("text.title", self.text_title),
            ("text.primary", self.text_primary),
            ("text.body", self.text_body),
            ("text.secondary", self.text_secondary),
            ("text.muted", self.text_muted),
            ("accent.primary", self.accent_primary),
            ("success.fg", self.success_fg),
            ("warning.fg", self.warning_fg),
            ("warning.bg", self.warning_bg),
            ("error.fg", self.error_fg),
            ("error.bg", self.error_bg),
            ("error.border", self.error_border),
            ("info.fg", self.info_fg),
        ]
    }
}

fn rgb(value: u32) -> Color {
    Color::Rgb(
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::Theme;
    use ratatui::style::Color;

    fn rgb(value: u32) -> Color {
        Color::Rgb(
            ((value >> 16) & 0xff) as u8,
            ((value >> 8) & 0xff) as u8,
            (value & 0xff) as u8,
        )
    }

    fn assert_tokens(theme: &Theme, expected: &[Color]) {
        let actual = [
            theme.bg_base,
            theme.bg_sunken,
            theme.bg_surface,
            theme.bg_surface_alt,
            theme.border_subtle,
            theme.border_strong,
            theme.text_title,
            theme.text_primary,
            theme.text_body,
            theme.text_secondary,
            theme.text_muted,
            theme.accent_primary,
            theme.success_fg,
            theme.warning_fg,
            theme.warning_bg,
            theme.error_fg,
            theme.error_bg,
            theme.error_border,
            theme.info_fg,
        ];

        for (actual, expected) in actual.into_iter().zip(expected.iter()) {
            assert_eq!(actual, *expected);
        }
    }

    #[test]
    fn midnight_tokens_match_design_spec_01() {
        let theme = Theme::midnight();

        assert_tokens(
            &theme,
            &[
                rgb(0x14131c),
                rgb(0x0e0d15),
                rgb(0x1a1925),
                rgb(0x1e1c2c),
                rgb(0x2c2a3d),
                rgb(0x3a3850),
                rgb(0xf3f1fa),
                rgb(0xe8e6f0),
                rgb(0xd7d4e4),
                rgb(0x9a97b0),
                rgb(0x6f6c86),
                rgb(0xb18cf0),
                rgb(0x7ddca4),
                rgb(0xf0c674),
                rgb(0x3a3327),
                rgb(0xf08a8a),
                rgb(0x241a1a),
                rgb(0x5c2b2e),
                rgb(0x6fd3e0),
            ],
        );
    }

    #[test]
    fn daylight_tokens_match_design_spec_01() {
        let theme = Theme::daylight();

        assert_tokens(
            &theme,
            &[
                rgb(0xf4f1ea),
                rgb(0xefebe2),
                rgb(0xfffdf8),
                rgb(0xfaf8f3),
                rgb(0xe0d9cb),
                rgb(0xd6cdbb),
                rgb(0x2c2a33),
                rgb(0x2c2a33),
                rgb(0x5a5566),
                rgb(0x6b6675),
                rgb(0x9a93a6),
                rgb(0x6d44c0),
                rgb(0x2f8f5b),
                rgb(0x9a6b12),
                rgb(0xfbf3e0),
                rgb(0xb8453f),
                rgb(0xfdf1f0),
                rgb(0xf0d4d2),
                rgb(0x1f7a8c),
            ],
        );
    }
}
