//! X keysyms, which both backends use: X11 delivers keycodes that the keyboard mapping turns
//! into keysyms, and Wayland keymaps name the same keysyms.

pub const BACKSPACE: u32 = 0xff08;
pub const TAB: u32 = 0xff09;
pub const RETURN: u32 = 0xff0d;
pub const ESCAPE: u32 = 0xff1b;
pub const HOME: u32 = 0xff50;
pub const LEFT: u32 = 0xff51;
pub const UP: u32 = 0xff52;
pub const RIGHT: u32 = 0xff53;
pub const DOWN: u32 = 0xff54;
pub const PAGE_UP: u32 = 0xff55;
pub const PAGE_DOWN: u32 = 0xff56;
pub const END: u32 = 0xff57;
pub const KP_ENTER: u32 = 0xff8d;
pub const ISO_LEFT_TAB: u32 = 0xfe20;
pub const DELETE: u32 = 0xffff;

/// Modifier masks, the same bits in X11 key events and in Wayland's serialized real modifiers.
pub const SHIFT: u32 = 1;
pub const LOCK: u32 = 2;
pub const CONTROL: u32 = 4;
pub const MOD1: u32 = 8;
pub const MOD2: u32 = 16;
pub const MOD4: u32 = 64;
/// The modifiers that make a key a shortcut rather than typing.
pub const COMMAND_MODIFIERS: u32 = CONTROL | MOD1 | MOD4;

/// Whether the keysym is a modifier key itself (Shift_L … Hyper_R, ISO_Level3_Shift and similar).
pub fn is_modifier(sym: u32) -> bool {
    (0xffe1..=0xffee).contains(&sym) || (0xfe01..=0xfe13).contains(&sym) || sym == 0xff7e || sym == 0xff7f
}

/// The printable character a keysym types, if any: Latin-1 keysyms map directly, Unicode
/// keysyms are 0x1000000 plus the code point.
pub fn to_char(sym: u32) -> Option<char> {
    match sym {
        0x20..=0x7e | 0xa0..=0xff => char::from_u32(sym),
        0x0100_0100..=0x0110_ffff => char::from_u32(sym - 0x0100_0000),
        _ => None,
    }
}

/// The keysym for a character, the reverse of [`to_char`].
pub fn from_char(c: char) -> u32 {
    match c as u32 {
        n @ (0x20..=0x7e | 0xa0..=0xff) => n,
        n => 0x0100_0000 + n,
    }
}

const NAMES: &[(&str, u32)] = &[
    ("space", 0x20),
    ("exclam", 0x21),
    ("quotedbl", 0x22),
    ("numbersign", 0x23),
    ("dollar", 0x24),
    ("percent", 0x25),
    ("ampersand", 0x26),
    ("apostrophe", 0x27),
    ("quoteright", 0x27),
    ("parenleft", 0x28),
    ("parenright", 0x29),
    ("asterisk", 0x2a),
    ("plus", 0x2b),
    ("comma", 0x2c),
    ("minus", 0x2d),
    ("period", 0x2e),
    ("slash", 0x2f),
    ("colon", 0x3a),
    ("semicolon", 0x3b),
    ("less", 0x3c),
    ("equal", 0x3d),
    ("greater", 0x3e),
    ("question", 0x3f),
    ("at", 0x40),
    ("bracketleft", 0x5b),
    ("backslash", 0x5c),
    ("bracketright", 0x5d),
    ("asciicircum", 0x5e),
    ("underscore", 0x5f),
    ("grave", 0x60),
    ("quoteleft", 0x60),
    ("braceleft", 0x7b),
    ("bar", 0x7c),
    ("braceright", 0x7d),
    ("asciitilde", 0x7e),
    ("nobreakspace", 0xa0),
    ("BackSpace", BACKSPACE),
    ("Tab", TAB),
    ("ISO_Left_Tab", ISO_LEFT_TAB),
    ("Return", RETURN),
    ("Escape", ESCAPE),
    ("Delete", DELETE),
    ("Insert", 0xff63),
    ("Menu", 0xff67),
    ("Home", HOME),
    ("Left", LEFT),
    ("Up", UP),
    ("Right", RIGHT),
    ("Down", DOWN),
    ("Prior", PAGE_UP),
    ("Page_Up", PAGE_UP),
    ("Next", PAGE_DOWN),
    ("Page_Down", PAGE_DOWN),
    ("End", END),
    ("KP_Enter", KP_ENTER),
    ("Shift_L", 0xffe1),
    ("Shift_R", 0xffe2),
    ("Control_L", 0xffe3),
    ("Control_R", 0xffe4),
    ("Caps_Lock", 0xffe5),
    ("Shift_Lock", 0xffe6),
    ("Meta_L", 0xffe7),
    ("Meta_R", 0xffe8),
    ("Alt_L", 0xffe9),
    ("Alt_R", 0xffea),
    ("Super_L", 0xffeb),
    ("Super_R", 0xffec),
    ("Hyper_L", 0xffed),
    ("Hyper_R", 0xffee),
    ("ISO_Level3_Shift", 0xfe03),
    ("ISO_Level5_Shift", 0xfe11),
    ("Mode_switch", 0xff7e),
    ("Num_Lock", 0xff7f),
    ("Hangul", 0xff31),
    ("Hangul_Hanja", 0xff34),
];

/// Looks up a keysym name as XKB keymaps and the config file write it: "a", "comma", "F12",
/// "KP_1", "U4F60" or "0x1004f60". Single characters stand for themselves.
pub fn from_name(name: &str) -> Option<u32> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Some(from_char(c));
    }
    if let Some(&(_, sym)) = NAMES.iter().find(|(n, _)| *n == name) {
        return Some(sym);
    }
    if let Some(number) = name.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()).filter(|n| (1..=35).contains(n)) {
        return Some(0xffbe + number - 1);
    }
    if let Some(digit) = name.strip_prefix("KP_").and_then(|n| n.parse::<u32>().ok()).filter(|n| *n <= 9) {
        return Some(0xffb0 + digit);
    }
    if let Some(hex) = name.strip_prefix("0x") {
        return u32::from_str_radix(hex, 16).ok();
    }
    if let Some(hex) = name.strip_prefix('U').filter(|h| h.len() >= 4) {
        return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32).map(from_char);
    }
    None
}

/// Like [`from_name`] but ignores the case of named keys, for hand-written config files.
pub fn from_config_name(name: &str) -> Option<u32> {
    if name.chars().count() == 1 {
        return from_name(&name.to_lowercase());
    }
    from_name(name).or_else(|| {
        let lower = name.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix('f') {
            if let Ok(n) = rest.parse::<u32>() {
                return from_name(&format!("F{n}"));
            }
        }
        NAMES.iter().find(|(n, _)| n.to_ascii_lowercase() == lower).map(|&(_, sym)| sym)
    })
}

/// The lower-case keysym of a letter, so hotkeys match with or without Shift and Caps Lock.
pub fn lowercase(sym: u32) -> u32 {
    match to_char(sym) {
        Some(c) if c.is_uppercase() => c.to_lowercase().next().map(from_char).unwrap_or(sym),
        _ => sym,
    }
}

/// Picks the keysym for a key from its levels (unshifted, shifted), following the core X
/// rules: Shift selects the second level, Caps Lock upper-cases letters, Num Lock swaps the
/// keypad levels.
pub fn choose(levels: &[u32], state: u32) -> u32 {
    let first = levels.first().copied().unwrap_or(0);
    let second = levels.get(1).copied().filter(|&s| s != 0);
    let shift = state & SHIFT != 0;
    if (0xff80..=0xffbd).contains(&second.unwrap_or(0)) && state & MOD2 != 0 {
        return if shift { first } else { second.unwrap_or(first) };
    }
    let is_letter = to_char(first).is_some_and(char::is_lowercase);
    let second_level = shift ^ (state & LOCK != 0 && is_letter);
    if second_level {
        second.unwrap_or_else(|| upper(first))
    } else {
        first
    }
}

fn upper(sym: u32) -> u32 {
    match to_char(sym) {
        Some(c) if c.is_lowercase() => {
            let mut upper = c.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(u), None) => from_char(u),
                _ => sym,
            }
        }
        _ => sym,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(from_name("a"), Some('a' as u32));
        assert_eq!(from_name("comma"), Some(',' as u32));
        assert_eq!(from_name("F12"), Some(0xffc9));
        assert_eq!(from_name("KP_1"), Some(0xffb1));
        assert_eq!(from_name("U4F60"), Some(0x0100_4f60));
        assert_eq!(from_config_name("f12"), Some(0xffc9));
        assert_eq!(from_config_name("SPACE"), Some(0x20));
        assert_eq!(from_config_name("A"), Some('a' as u32));
    }

    #[test]
    fn levels() {
        let a = ['a' as u32, 'A' as u32];
        assert_eq!(choose(&a, 0), 'a' as u32);
        assert_eq!(choose(&a, SHIFT), 'A' as u32);
        assert_eq!(choose(&a, LOCK), 'A' as u32);
        assert_eq!(choose(&a, LOCK | SHIFT), 'a' as u32);
        assert_eq!(choose(&['a' as u32], SHIFT), 'A' as u32);
        assert_eq!(choose(&['1' as u32, '!' as u32], LOCK), '1' as u32);
        assert_eq!(choose(&[0xff9c, 0xffb1], MOD2), 0xffb1);
        assert_eq!(lowercase('Q' as u32), 'q' as u32);
        assert_eq!(to_char(0x0100_4f60), Some('你'));
    }
}
