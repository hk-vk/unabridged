//! Raw design tokens, transcribed from the Wonder file "Better Audible".
//!
//! Colours are hex literals exactly as the design names them (`mac/*`, `theme/*`,
//! `book/*`). Nothing outside this module and [`crate::metrics`] should contain
//! a colour or size literal; [`crate::theme`] turns these into semantic roles.

/// Light window palette (`mac/*` tokens).
pub mod light {
    /// `mac/window`, also the sidebar material.
    pub const WINDOW: u32 = 0xFFFFFF;
    /// `mac/ink`: plum-black text.
    pub const INK: u32 = 0x241E2B;
    /// `mac/ink-secondary`.
    pub const INK_SECONDARY: u32 = 0x6B6475;
    /// `mac/ink-top`: top of the dark play button gradient.
    pub const INK_TOP: u32 = 0x443B4F;
    /// `mac/hairline` (RGBA).
    pub const HAIRLINE: u32 = 0x241E2B1A;
    /// `mac/fill` (RGBA): subtle control background.
    pub const FILL: u32 = 0x241E2B0F;
    /// `mac/button-top`.
    pub const BUTTON_TOP: u32 = 0xFFFFFF;
    /// `mac/button-bottom`.
    pub const BUTTON_BOTTOM: u32 = 0xF3F0F5;
    /// Floating player material (RGBA, `#f8f6fac7` in the mock-up).
    pub const FLOATING: u32 = 0xF8F6FAC7;
}

/// Dark window palette. The design only sketches dark mode (Appearance
/// settings preview: ground `#17141b`, text `#f4eff7`); the rest is derived
/// in the same plum family.
pub mod dark {
    /// Window ground, from the settings preview.
    pub const WINDOW: u32 = 0x17141B;
    /// Sidebar: one step above the ground.
    pub const SIDEBAR: u32 = 0x1C1921;
    /// Primary text, from the settings preview.
    pub const INK: u32 = 0xF4EFF7;
    /// Secondary text: keeps 4.5:1 on the ground.
    pub const INK_SECONDARY: u32 = 0xA69EB0;
    /// Top of the inverted (light) play button gradient.
    pub const INK_TOP: u32 = 0xFFFFFF;
    /// Hairline (RGBA).
    pub const HAIRLINE: u32 = 0xF4EFF71F;
    /// Subtle control background (RGBA).
    pub const FILL: u32 = 0xF4EFF714;
    /// Secondary button gradient top.
    pub const BUTTON_TOP: u32 = 0x3A3442;
    /// Secondary button gradient bottom.
    pub const BUTTON_BOTTOM: u32 = 0x2D2834;
    /// Floating player material (RGBA).
    pub const FLOATING: u32 = 0x26222DD9;
}

/// Pure white and black, used for highlights and shadows.
pub mod mono {
    /// White.
    pub const WHITE: u32 = 0xFFFFFF;
    /// Black.
    pub const BLACK: u32 = 0x000000;
}

/// One accent theme as the design specifies it: accent, deep (text on tint) and tint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccentTokens {
    /// The accent itself (`mac/selection` for Ember).
    pub accent: u32,
    /// Darker tone for text on the tint and for button rings.
    pub deep: u32,
    /// Selected-row background.
    pub tint: u32,
    /// Gradient top for primary buttons; derived when `None`.
    pub top: Option<u32>,
    /// Light glow tone (dark-mode accent, button glow); derived when `None`.
    pub glow: Option<u32>,
    /// Whether text on the accent should be dark (Saffron) rather than white.
    pub dark_text_on_accent: bool,
}

/// Ember, the default: `theme/accent*` tokens.
pub const EMBER: AccentTokens = AccentTokens {
    accent: 0xD63F25,
    deep: 0x9E2A14,
    tint: 0xFCEBE6,
    top: Some(0xEC5A3C),
    glow: Some(0xFF8A6B),
    dark_text_on_accent: false,
};

/// Fjord: `theme/fjord*`.
pub const FJORD: AccentTokens = AccentTokens {
    accent: 0x0E7C7B,
    deep: 0x0A4F55,
    tint: 0xDDF1F0,
    top: None,
    glow: None,
    dark_text_on_accent: false,
};

/// Plum: `theme/plum*`.
pub const PLUM: AccentTokens = AccentTokens {
    accent: 0x7B3FC4,
    deep: 0x3E1F73,
    tint: 0xEEE5FA,
    top: None,
    glow: None,
    dark_text_on_accent: false,
};

/// Moss: `theme/moss*`.
pub const MOSS: AccentTokens = AccentTokens {
    accent: 0x3B8A4A,
    deep: 0x1F4D2B,
    tint: 0xE1F2E4,
    top: None,
    glow: None,
    dark_text_on_accent: false,
};

/// Saffron: `theme/saffron*`. Golden yellow with dark text on buttons.
pub const SAFFRON: AccentTokens = AccentTokens {
    accent: 0xF0A216,
    deep: 0x7A4A06,
    tint: 0xFDF0D3,
    top: None,
    glow: None,
    dark_text_on_accent: true,
};

/// Default Now Playing field (`book/*` tokens), used until a cover is analysed.
pub mod book {
    /// `book/field`.
    pub const FIELD: u32 = 0x232A44;
    /// `book/field-deep`.
    pub const FIELD_DEEP: u32 = 0x161B2E;
    /// `book/on-field`.
    pub const ON_FIELD: u32 = 0xF3EEF2;
    /// `book/button-bottom`: bottom of the light play button on the field.
    pub const BUTTON_BOTTOM: u32 = 0xDCD5E3;
    /// Alpha of `book/on-field-muted`.
    pub const MUTED_ALPHA: f32 = 0.68;
    /// Alpha of `book/on-field-track`.
    pub const TRACK_ALPHA: f32 = 0.18;
    /// Opacity of the translucent mini player and popover material.
    pub const MATERIAL_ALPHA: f32 = 0.78;
}

/// macOS traffic light colours, used only for the static window previews.
pub mod traffic {
    /// Close.
    pub const CLOSE: u32 = 0xFF5F57;
    /// Minimise.
    pub const MINIMIZE: u32 = 0xFEBC2E;
    /// Zoom.
    pub const ZOOM: u32 = 0x28C840;
}
