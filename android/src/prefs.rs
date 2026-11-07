//! Settings, stored in the same `SharedPreferences` file the Java app used, so they survive the upgrade.

use crate::jni::{jobject, Env, Local, Value};
use crate::keyboard::{Mode, Settings, MODES};

const PREFS_NAME: &str = "convoy_pinyin_prefs";
const KEY_DARK_MODE: &str = "dark_mode"; // before the theme setting existed
pub const KEY_THEME: &str = "theme";
pub const KEY_LANGUAGE_PREFIX: &str = "language_";

pub const THEME_SYSTEM: &str = "system";
pub const THEME_LIGHT: &str = "light";
pub const THEME_DARK: &str = "dark";

/// The on/off settings shown as switches, besides the keyboards: (preference key, default).
/// Each switch's view is named after its key: `auto_space_toggle`.
pub const SWITCHES: [(&str, bool); 11] = [
    ("key_vibration", true),
    ("key_popup", true),
    ("number_row_hints", true),
    ("auto_space", false),
    // Off until the user turns it on; it also needs a model installed.
    ("grammar_button", false),
    ("romanize_selection", true),
    ("auto_correct_english", true),
    ("auto_correct_malay", true),
    ("normal_space_chinese", false),
    ("normal_space_japanese", false),
    ("chinese_traditional", false),
];

/// Settings that replaced an older, shared one start from its value: (new key, old key).
const REPLACED: [(&str, &str); 6] = [
    ("auto_correct_english", "auto_correct"),
    ("auto_correct_malay", "auto_correct"),
    ("normal_space_chinese", "normal_space"),
    ("normal_space_japanese", "normal_space"),
    ("language_japanese_romaji", "romaji_keys"),
    ("language_korean_romaji", "romaji_keys"),
];

/// A switch's value: its own preference, else the one it replaced, else `default`.
pub fn get_switch(env: Env, context: jobject, key: &str, default: bool) -> bool {
    // Chinese used to be two keyboards, Simplified and Traditional.
    let simplified = || get_bool(env, context, "language_simplified", true);
    let traditional = || get_bool(env, context, "language_traditional", true);
    let default = match key {
        "language_chinese" => simplified() || traditional(),
        "chinese_traditional" => !simplified() && traditional(),
        _ => match REPLACED.iter().find(|(new, _)| *new == key) {
            Some((_, old)) => get_bool(env, context, old, default),
            None => default,
        },
    };
    get_bool(env, context, key, default)
}

/// The preference key that turns a keyboard on, such as `language_japanese_romaji`.
pub fn keyboard_key(mode: Mode) -> String {
    format!("{KEY_LANGUAGE_PREFIX}{}", mode.language_key())
}
/// The display name of the installed AI model.
pub const KEY_MODEL_NAME: &str = "grammar_model_name";

fn prefs(env: Env, context: jobject) -> Option<Local> {
    let name = Local::new(env, env.string(PREFS_NAME).ok()?);
    let prefs = env
        .call(
            context,
            "getSharedPreferences",
            "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
            &[Value::Obj(name.obj), Value::Int(0)],
        )
        .ok()?
        .obj();
    (!prefs.is_null()).then(|| Local::new(env, prefs))
}

pub fn get_bool(env: Env, context: jobject, key: &str, default: bool) -> bool {
    let read = || {
        let prefs = prefs(env, context)?;
        let key = Local::new(env, env.string(key).ok()?);
        env.call(prefs.obj, "getBoolean", "(Ljava/lang/String;Z)Z", &[Value::Obj(key.obj), Value::Bool(default)]).ok().map(Value::bool)
    };
    read().unwrap_or(default)
}

fn contains(env: Env, context: jobject, key: &str) -> bool {
    let read = || {
        let prefs = prefs(env, context)?;
        let key = Local::new(env, env.string(key).ok()?);
        env.call(prefs.obj, "contains", "(Ljava/lang/String;)Z", &[Value::Obj(key.obj)]).ok().map(Value::bool)
    };
    read().unwrap_or(false)
}

pub fn get_string(env: Env, context: jobject, key: &str) -> Option<String> {
    let prefs = prefs(env, context)?;
    let key = Local::new(env, env.string(key).ok()?);
    let value = env
        .call(
            prefs.obj,
            "getString",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[Value::Obj(key.obj), Value::Obj(std::ptr::null_mut())],
        )
        .ok()?;
    let value = Local::new(env, value.obj());
    (!value.obj.is_null()).then(|| env.read_string(value.obj))
}

/// Opens an editor, lets `put` change it, and applies the change in the background.
fn edit(env: Env, context: jobject, put: impl FnOnce(jobject)) {
    let Some(prefs) = prefs(env, context) else { return };
    let Ok(editor) = env.call(prefs.obj, "edit", "()Landroid/content/SharedPreferences$Editor;", &[]) else { return };
    let editor = Local::new(env, editor.obj());
    put(editor.obj);
    let _ = env.call(editor.obj, "apply", "()V", &[]);
}

pub fn set_bool(env: Env, context: jobject, key: &str, value: bool) {
    edit(env, context, |editor| {
        if let Ok(key) = env.string(key) {
            let key = Local::new(env, key);
            let _ = env
                .call(
                    editor,
                    "putBoolean",
                    "(Ljava/lang/String;Z)Landroid/content/SharedPreferences$Editor;",
                    &[Value::Obj(key.obj), Value::Bool(value)],
                )
                .map(|v| env.delete_local(v.obj()));
        }
    });
}

pub fn set_string(env: Env, context: jobject, key: &str, value: &str) {
    edit(env, context, |editor| {
        if let (Ok(key), Ok(value)) = (env.string(key), env.string(value)) {
            let (key, value) = (Local::new(env, key), Local::new(env, value));
            let _ = env
                .call(
                    editor,
                    "putString",
                    "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
                    &[Value::Obj(key.obj), Value::Obj(value.obj)],
                )
                .map(|v| env.delete_local(v.obj()));
        }
    });
}

/// "system", "light" or "dark". Users of the old dark-mode switch keep their choice.
pub fn theme(env: Env, context: jobject) -> String {
    if contains(env, context, KEY_THEME) {
        return get_string(env, context, KEY_THEME).unwrap_or_else(|| THEME_SYSTEM.into());
    }
    if contains(env, context, KEY_DARK_MODE) {
        return if get_bool(env, context, KEY_DARK_MODE, false) { THEME_DARK } else { THEME_LIGHT }.into();
    }
    THEME_SYSTEM.into()
}

pub fn is_dark_mode(env: Env, context: jobject) -> bool {
    match theme(env, context).as_str() {
        THEME_DARK => true,
        THEME_LIGHT => false,
        _ => {
            const UI_MODE_NIGHT_MASK: i32 = 0x30;
            const UI_MODE_NIGHT_YES: i32 = 0x20;
            let read = || {
                let resources = Local::new(env, env.call(context, "getResources", "()Landroid/content/res/Resources;", &[]).ok()?.obj());
                let config =
                    Local::new(env, env.call(resources.obj, "getConfiguration", "()Landroid/content/res/Configuration;", &[]).ok()?.obj());
                env.field(config.obj, "uiMode", "I").ok().map(Value::int)
            };
            read().is_some_and(|mode| mode & UI_MODE_NIGHT_MASK == UI_MODE_NIGHT_YES)
        }
    }
}

pub fn load(env: Env, context: jobject) -> Settings {
    let switch = |key: &str| {
        let default = SWITCHES.iter().find(|(k, _)| *k == key).is_some_and(|(_, d)| *d);
        get_switch(env, context, key, default)
    };
    let mut languages = [true; MODES.len()];
    for (enabled, mode) in languages.iter_mut().zip(MODES) {
        *enabled = get_switch(env, context, &keyboard_key(mode), mode.on_by_default());
    }
    Settings {
        dark: is_dark_mode(env, context),
        vibration: switch("key_vibration"),
        key_popup: switch("key_popup"),
        number_hints: switch("number_row_hints"),
        auto_space: switch("auto_space"),
        grammar_button: switch("grammar_button"),
        romanize_selection: switch("romanize_selection"),
        auto_correct_english: switch("auto_correct_english"),
        auto_correct_malay: switch("auto_correct_malay"),
        normal_space_chinese: switch("normal_space_chinese"),
        normal_space_japanese: switch("normal_space_japanese"),
        chinese_traditional: switch("chinese_traditional"),
        languages,
    }
}
