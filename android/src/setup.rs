//! The settings screen. Its layout is `res/layout/activity_setup.xml`; this module binds the
//! buttons and switches to the preferences the keyboard reads. Languages and the AI grammar
//! check have pages of their own, shown in place of the main page.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

use crate::jni::{jint, jobject, Env, Global, JResult, Local, RawEnv, Value};
use crate::keyboard::MODES;
use crate::{guard, log_error, prefs, setup_ai, CALLBACK_CLASS};

const OPEN_INPUT_SETTINGS: i64 = 100;
const OPEN_INPUT_PICKER: i64 = 101;
const THEME_GROUP: i64 = 102;
const OPEN_LANGUAGES: i64 = 103;
const OPEN_AI: i64 = 104;
const BACK: i64 = 105;
const OPEN_ACCESSIBILITY: i64 = 106;
/// Radio groups use `CHOICE_BASE + index` into [`CHOICES`].
const CHOICE_BASE: i64 = 300;
/// Switches use `SWITCH_BASE + index` into [`switches`].
const SWITCH_BASE: i64 = 200;

/// The settings activity currently shown, for `recreate()` after a theme change and for updates
/// from the AI model's background work.
pub static ACTIVITY: Mutex<Option<Global>> = Mutex::new(None);

/// Runs `f` with the settings activity currently shown, if there is one.
pub fn with_activity<R>(f: impl FnOnce(jobject) -> R) -> Option<R> {
    ACTIVITY.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|activity| f(activity.0))
}

/// The buttons of this module: (view id, callback id).
const BUTTONS: [(&str, i64); 8] = [
    ("open_input_settings", OPEN_INPUT_SETTINGS),
    ("open_input_picker", OPEN_INPUT_PICKER),
    ("open_languages", OPEN_LANGUAGES),
    ("open_ai", OPEN_AI),
    ("back_from_languages", BACK),
    ("back_from_ai", BACK),
    ("open_accessibility", OPEN_ACCESSIBILITY),
    ("back_from_accessibility", BACK),
];

/// Settings chosen with radio buttons: (preference key, option names, default option). The group's
/// view is `<key>_group` and each button's is `<key>_<option>`.
fn choices() -> [(&'static str, Vec<&'static str>, &'static str); 2] {
    [
        (prefs::KEY_KEYBOARD_SIZE, prefs::KEYBOARD_SIZES.iter().map(|(name, _)| *name).collect(), "medium"),
        (prefs::KEY_LONG_PRESS, prefs::LONG_PRESS_DELAYS.iter().map(|(name, _)| *name).collect(), "normal"),
    ]
}

/// The pages of the screen, by the id of their container.
const PAGES: [&str; 4] = ["page_main", "page_languages", "page_ai", "page_accessibility"];
/// The page shown, as an index into [`PAGES`]; kept when the activity is recreated.
static PAGE: AtomicU8 = AtomicU8::new(0);

/// Every switch on the screen: (preference key, default). Its view is `<key>_toggle`.
fn switches() -> Vec<(String, bool)> {
    let mut all: Vec<(String, bool)> = prefs::SWITCHES.iter().map(|&(key, default)| (key.to_string(), default)).collect();
    all.extend(MODES.map(|mode| (prefs::keyboard_key(mode), mode.on_by_default())));
    all
}

/// Shows one page and hides the others, scrolled to the top.
fn show_page(env: Env, activity: jobject, page: u8) {
    PAGE.store(page, Ordering::SeqCst);
    const VISIBLE: i32 = 0;
    const GONE: i32 = 8;
    for (i, name) in PAGES.iter().enumerate() {
        if let Ok(view) = find_view(env, activity, name) {
            let visibility = if i == page as usize { VISIBLE } else { GONE };
            let _ = env.call(view.obj, "setVisibility", "(I)V", &[Value::Int(visibility)]);
        }
    }
    if let Ok(scroll) = find_view(env, activity, "settings_scroll") {
        let _ = env.call(scroll.obj, "scrollTo", "(II)V", &[Value::Int(0), Value::Int(0)]);
    }
}

fn resource_id(env: Env, context: jobject, name: &str, kind: &str) -> JResult<i32> {
    let resources = Local::new(env, env.call(context, "getResources", "()Landroid/content/res/Resources;", &[])?.obj());
    let package = Local::new(env, env.call(context, "getPackageName", "()Ljava/lang/String;", &[])?.obj());
    let (name, kind) = (Local::new(env, env.string(name)?), Local::new(env, env.string(kind)?));
    let args = [Value::Obj(name.obj), Value::Obj(kind.obj), Value::Obj(package.obj)];
    Ok(env.call(resources.obj, "getIdentifier", "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I", &args)?.int())
}

pub fn find_view(env: Env, activity: jobject, name: &str) -> JResult<Local> {
    let id = resource_id(env, activity, name, "id")?;
    let view = env.call(activity, "findViewById", "(I)Landroid/view/View;", &[Value::Int(id)])?.obj();
    if view.is_null() {
        log_error(&format!("view {name} not found"));
        return Err(crate::jni::JavaException);
    }
    Ok(Local::new(env, view))
}

pub fn callback(env: Env, id: i64) -> JResult<Local> {
    Ok(Local::new(env, env.new_object(CALLBACK_CLASS, "(J)V", &[Value::Long(id)])?))
}

fn bind(env: Env, activity: jobject) -> JResult<()> {
    for (name, id) in BUTTONS.into_iter().chain(setup_ai::BUTTONS) {
        let button = find_view(env, activity, name)?;
        let listener = callback(env, id)?;
        env.call(button.obj, "setOnClickListener", "(Landroid/view/View$OnClickListener;)V", &[Value::Obj(listener.obj)])?;
    }

    let group = find_view(env, activity, "theme_group")?;
    let selected = match prefs::theme(env, activity).as_str() {
        prefs::THEME_DARK => "theme_dark",
        prefs::THEME_LIGHT => "theme_light",
        _ => "theme_system",
    };
    env.call(group.obj, "check", "(I)V", &[Value::Int(resource_id(env, activity, selected, "id")?)])?;
    let listener = callback(env, THEME_GROUP)?;
    env.call(
        group.obj,
        "setOnCheckedChangeListener",
        "(Landroid/widget/RadioGroup$OnCheckedChangeListener;)V",
        &[Value::Obj(listener.obj)],
    )?;

    for (index, (key, options, default)) in choices().into_iter().enumerate() {
        let group = find_view(env, activity, &format!("{key}_group"))?;
        let value = prefs::get_string(env, activity, key).filter(|v| options.contains(&v.as_str()));
        let button = format!("{key}_{}", value.as_deref().unwrap_or(default));
        env.call(group.obj, "check", "(I)V", &[Value::Int(resource_id(env, activity, &button, "id")?)])?;
        let listener = callback(env, CHOICE_BASE + index as i64)?;
        env.call(
            group.obj,
            "setOnCheckedChangeListener",
            "(Landroid/widget/RadioGroup$OnCheckedChangeListener;)V",
            &[Value::Obj(listener.obj)],
        )?;
    }
    for (index, (key, default)) in switches().iter().enumerate() {
        let switch = find_view(env, activity, &format!("{key}_toggle"))?;
        env.call(switch.obj, "setChecked", "(Z)V", &[Value::Bool(prefs::get_switch(env, activity, key, *default))])?;
        let listener = callback(env, SWITCH_BASE + index as i64)?;
        env.call(
            switch.obj,
            "setOnCheckedChangeListener",
            "(Landroid/widget/CompoundButton$OnCheckedChangeListener;)V",
            &[Value::Obj(listener.obj)],
        )?;
    }
    setup_ai::on_open(env);
    setup_ai::refresh(env, activity);
    Ok(())
}

pub extern "system" fn on_create(raw: RawEnv, this: jobject, saved_state: jobject) {
    guard((), || {
        let env = unsafe { Env::from_raw(raw) };
        let theme =
            if prefs::is_dark_mode(env, this) { "Theme_DeviceDefault_NoActionBar" } else { "Theme_DeviceDefault_Light_NoActionBar" };
        if let Ok(Value::Int(theme)) = env.static_field("android/R$style", theme, "I") {
            let _ = env.call(this, "setTheme", "(I)V", &[Value::Int(theme)]);
        }
        let _ = env.call_super(this, "android/app/Activity", "onCreate", "(Landroid/os/Bundle;)V", &[Value::Obj(saved_state)]);
        if let Some(old) = ACTIVITY.lock().unwrap_or_else(|e| e.into_inner()).replace(Global(env.new_global(this))) {
            env.delete_global(old.0);
        }
        let Ok(layout) = resource_id(env, this, "activity_setup", "layout") else { return };
        if env.call(this, "setContentView", "(I)V", &[Value::Int(layout)]).is_ok() && bind(env, this).is_err() {
            log_error("cannot set up the settings screen");
        }
        // A fresh start opens the main page; a recreated screen (after a theme change) stays put.
        show_page(env, this, if saved_state.is_null() { 0 } else { PAGE.load(Ordering::SeqCst) });
    })
}

/// Back returns from a page to the main page, and from there leaves the screen.
pub extern "system" fn on_back_pressed(raw: RawEnv, this: jobject) {
    guard((), || {
        let env = unsafe { Env::from_raw(raw) };
        if PAGE.load(Ordering::SeqCst) != 0 {
            show_page(env, this, 0);
        } else {
            let _ = env.call_super(this, "android/app/Activity", "onBackPressed", "()V", &[]);
        }
    })
}

fn context_of(env: Env, view: jobject) -> Option<Local> {
    let context = env.call(view, "getContext", "()Landroid/content/Context;", &[]).ok()?.obj();
    (!context.is_null()).then(|| Local::new(env, context))
}

pub fn on_click(env: Env, id: i64, view: jobject) {
    let Some(context) = context_of(env, view) else { return };
    match id {
        OPEN_INPUT_SETTINGS => {
            let Ok(action) = env.string("android.settings.INPUT_METHOD_SETTINGS") else { return };
            let action = Local::new(env, action);
            if let Ok(intent) = env.new_object("android/content/Intent", "(Ljava/lang/String;)V", &[Value::Obj(action.obj)]) {
                let intent = Local::new(env, intent);
                let _ = env.call(context.obj, "startActivity", "(Landroid/content/Intent;)V", &[Value::Obj(intent.obj)]);
            }
        }
        OPEN_INPUT_PICKER => {
            let Ok(name) = env.string("input_method") else { return };
            let name = Local::new(env, name);
            if let Ok(manager) =
                env.call(context.obj, "getSystemService", "(Ljava/lang/String;)Ljava/lang/Object;", &[Value::Obj(name.obj)])
            {
                let manager = Local::new(env, manager.obj());
                let _ = env.call(manager.obj, "showInputMethodPicker", "()V", &[]);
            }
        }
        OPEN_LANGUAGES | OPEN_AI | OPEN_ACCESSIBILITY | BACK => {
            let page = match id {
                OPEN_LANGUAGES => 1,
                OPEN_AI => 2,
                OPEN_ACCESSIBILITY => 3,
                _ => 0,
            };
            with_activity(|activity| show_page(env, activity, page));
        }
        id => setup_ai::on_click(env, id, context.obj),
    }
}

pub fn on_switch(env: Env, id: i64, button: jobject, checked: bool) {
    let Some(context) = context_of(env, button) else { return };
    if let Some((key, _)) = usize::try_from(id - SWITCH_BASE).ok().and_then(|i| switches().into_iter().nth(i)) {
        prefs::set_bool(env, context.obj, &key, checked);
    }
}

pub fn on_radio(env: Env, id: i64, group: jobject, checked_id: jint) {
    let Some(context) = context_of(env, group) else { return };
    if let Some((key, options, _)) = usize::try_from(id - CHOICE_BASE).ok().and_then(|i| choices().into_iter().nth(i)) {
        let checked = options
            .into_iter()
            .find(|option| resource_id(env, context.obj, &format!("{key}_{option}"), "id").is_ok_and(|r| r == checked_id));
        if let Some(option) = checked {
            prefs::set_string(env, context.obj, key, option);
        }
        return;
    }
    if id != THEME_GROUP {
        return;
    }
    let is = |name: &str| resource_id(env, context.obj, name, "id").map(|r| r == checked_id).unwrap_or(false);
    let theme = if is("theme_dark") {
        prefs::THEME_DARK
    } else if is("theme_light") {
        prefs::THEME_LIGHT
    } else {
        prefs::THEME_SYSTEM
    };
    prefs::set_string(env, context.obj, prefs::KEY_THEME, theme);
    with_activity(|activity| {
        let _ = env.call(activity, "recreate", "()V", &[]);
    });
}

/// `NativeCallback.run()` with an id from this screen.
pub fn on_run(env: Env, id: i64) {
    setup_ai::on_run(env, id);
}

pub fn on_dialog_click(env: Env, id: i64, which: jint) {
    setup_ai::on_dialog_click(env, id, which);
}

pub extern "system" fn on_activity_result(raw: RawEnv, this: jobject, request: jint, result: jint, data: jobject) {
    guard((), || {
        let env = unsafe { Env::from_raw(raw) };
        let args = [Value::Int(request), Value::Int(result), Value::Obj(data)];
        let _ = env.call_super(this, "android/app/Activity", "onActivityResult", "(IILandroid/content/Intent;)V", &args);
        setup_ai::on_activity_result(env, this, request, result, data);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The view ids in the settings layout.
    fn layout_ids() -> Vec<&'static str> {
        include_str!("../res/layout/activity_setup.xml").split("@+id/").skip(1).filter_map(|rest| rest.split('"').next()).collect()
    }

    #[test]
    fn every_view_the_code_uses_is_in_the_layout() {
        let ids = layout_ids();
        let switches = switches().into_iter().map(|(key, _)| format!("{key}_toggle"));
        let buttons = BUTTONS.iter().chain(&setup_ai::BUTTONS).map(|(name, _)| name.to_string());
        let choices = choices()
            .into_iter()
            .flat_map(|(key, options, _)| options.into_iter().map(move |option| format!("{key}_{option}")).chain([format!("{key}_group")]));
        let others = PAGES.iter().chain(&["settings_scroll", "theme_group", "theme_system", "theme_light", "theme_dark"]);
        let others = others.chain(&["device_report", "ai_status", "ai_progress"]).map(|name| name.to_string());
        for name in switches.chain(buttons).chain(choices).chain(others) {
            assert!(ids.contains(&name.as_str()), "{name} is not in activity_setup.xml");
        }
    }

    #[test]
    fn every_switch_in_the_layout_is_a_setting() {
        let keys: Vec<String> = switches().into_iter().map(|(key, _)| format!("{key}_toggle")).collect();
        for id in layout_ids().into_iter().filter(|id| id.ends_with("_toggle")) {
            assert!(keys.iter().any(|key| key == id), "{id} has no setting");
        }
        assert_eq!(keys.len(), prefs::SWITCHES.len() + MODES.len());
    }
}
