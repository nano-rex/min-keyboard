//! The settings screen. Its layout is `res/layout/activity_setup.xml`; this module binds the
//! buttons and switches to the preferences the keyboard reads.

use std::sync::Mutex;

use crate::jni::{jint, jobject, Env, Global, JResult, Local, RawEnv, Value};
use crate::keyboard::MODES;
use crate::{guard, log_error, prefs, setup_ai, CALLBACK_CLASS};

const OPEN_INPUT_SETTINGS: i64 = 100;
const OPEN_INPUT_PICKER: i64 = 101;
const THEME_GROUP: i64 = 102;
/// Switches use `SWITCH_BASE + index` into [`switches`].
const SWITCH_BASE: i64 = 200;

/// The settings activity currently shown, for `recreate()` after a theme change and for updates
/// from the AI model's background work.
pub static ACTIVITY: Mutex<Option<Global>> = Mutex::new(None);

/// Runs `f` with the settings activity currently shown, if there is one.
pub fn with_activity<R>(f: impl FnOnce(jobject) -> R) -> Option<R> {
    ACTIVITY.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|activity| f(activity.0))
}

/// Every switch on the screen: (view id, preference key, default).
fn switches() -> Vec<(String, String, bool)> {
    let mut all: Vec<(String, String, bool)> = [
        "vibration_toggle",
        "popup_toggle",
        "number_hints_toggle",
        "auto_correct_toggle",
        "auto_space_toggle",
        "grammar_button_toggle",
        "normal_space_toggle",
        "romaji_keys_toggle",
    ]
    .iter()
    .zip(prefs::SWITCHES)
    .map(|(id, (key, default))| (id.to_string(), key.to_string(), default))
    .collect();
    for mode in MODES {
        let language = mode.language_key();
        all.push((format!("language_{language}_toggle"), format!("{}{language}", prefs::KEY_LANGUAGE_PREFIX), true));
    }
    all
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
    let buttons = [("open_input_settings", OPEN_INPUT_SETTINGS), ("open_input_picker", OPEN_INPUT_PICKER)];
    for (name, id) in buttons.into_iter().chain(setup_ai::BUTTONS) {
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

    for (index, (name, key, default)) in switches().iter().enumerate() {
        let switch = find_view(env, activity, name)?;
        env.call(switch.obj, "setChecked", "(Z)V", &[Value::Bool(prefs::get_bool(env, activity, key, *default))])?;
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
        id => setup_ai::on_click(env, id, context.obj),
    }
}

pub fn on_switch(env: Env, id: i64, button: jobject, checked: bool) {
    let Some(context) = context_of(env, button) else { return };
    if let Some((_, key, _)) = usize::try_from(id - SWITCH_BASE).ok().and_then(|i| switches().into_iter().nth(i)) {
        prefs::set_bool(env, context.obj, &key, checked);
    }
}

pub fn on_radio(env: Env, id: i64, group: jobject, checked_id: jint) {
    if id != THEME_GROUP {
        return;
    }
    let Some(context) = context_of(env, group) else { return };
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
