//! Colours for the light and dark keyboard, as ARGB.

pub struct Theme {
    pub background: u32,
    pub key: u32,
    pub key_pressed: u32,
    pub function_key: u32,
    pub function_key_pressed: u32,
    pub key_shadow: u32,
    pub text: u32,
    pub text_muted: u32,
    pub accent: u32,
    pub accent_text: u32,
    pub candidate_highlight: u32,
    pub popup: u32,
}

const fn rgb(hex: u32) -> u32 {
    0xFF00_0000 | hex
}

pub const DARK: Theme = Theme {
    background: rgb(0x1B1C1F),
    key: rgb(0x3B3D42),
    key_pressed: rgb(0x55585E),
    function_key: rgb(0x2A2C30),
    function_key_pressed: rgb(0x44474C),
    key_shadow: rgb(0x0E0F11),
    text: rgb(0xE8EAED),
    text_muted: rgb(0x9AA0A6),
    accent: rgb(0x8AB4F8),
    accent_text: rgb(0x0B1D3A),
    candidate_highlight: rgb(0x2F3A4D),
    popup: rgb(0x4A4D53),
};

pub const LIGHT: Theme = Theme {
    background: rgb(0xE6E8EC),
    key: rgb(0xFFFFFF),
    key_pressed: rgb(0xD2D6DC),
    function_key: rgb(0xCDD2D9),
    function_key_pressed: rgb(0xB4BAC3),
    key_shadow: rgb(0xA9AFB8),
    text: rgb(0x1F1F1F),
    text_muted: rgb(0x5F6368),
    accent: rgb(0x1A73E8),
    accent_text: rgb(0xFFFFFF),
    candidate_highlight: rgb(0xD3E3FD),
    popup: rgb(0xFFFFFF),
};

pub fn of(dark: bool) -> &'static Theme {
    if dark {
        &DARK
    } else {
        &LIGHT
    }
}
