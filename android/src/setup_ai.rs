//! The settings screen's AI grammar check section: what the device can run, getting a model file
//! onto the device, a speed test and removal. Copying and testing run on background threads and
//! report through [`STATE`], which is shown whenever `NativeCallback(REFRESH)` runs on the UI thread.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use crate::jni::{jint, jobject, Env, Global, JResult, Local, Value};
use crate::setup::{callback, find_view, with_activity};
use crate::{grammar_worker, log_error, model_path, prefs, prompt_cache_dir, start_check, start_prepare, VM};

const DEVICE_CHECK: i64 = 103;
const DOWNLOAD: i64 = 104;
const IMPORT: i64 = 105;
const TEST: i64 = 106;
const REMOVE: i64 = 107;
const REMOVE_CONFIRMED: i64 = 108;
/// Posted to the UI thread to show [`STATE`].
const REFRESH: i64 = 109;

/// The section's buttons and their callback ids.
pub const BUTTONS: [(&str, i64); 5] =
    [("ai_device_check", DEVICE_CHECK), ("ai_download", DOWNLOAD), ("ai_import", IMPORT), ("ai_test", TEST), ("ai_remove", REMOVE)];

const REQUEST_MODEL_FILE: jint = 41;
const RESULT_OK: jint = -1;
const MIN_CORES: usize = 4;
const MIN_FREE_STORAGE_AFTER_IMPORT: u64 = 500 * 1024 * 1024;
const VISIBLE: i32 = 0;
const GONE: i32 = 8;

#[derive(Default)]
struct State {
    /// A copy is running; the buttons wait.
    busy: bool,
    /// A speed test is running.
    testing: bool,
    /// Copy progress in percent, while a copy with a known size runs.
    progress: Option<i32>,
    /// The latest message; empty shows what is installed.
    status: String,
    /// The device report, once checked.
    report: Option<String>,
}

static STATE: Mutex<State> = Mutex::new(State { busy: false, testing: false, progress: None, status: String::new(), report: None });

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// `NativeCallback(REFRESH)`, made on the UI thread: on threads started from Rust, JNI's
/// `FindClass` only sees system classes, so background threads cannot create it themselves.
static REFRESH_RUNNABLE: Mutex<Option<Global>> = Mutex::new(None);

/// Asks the UI thread to show the current state; callable from any thread.
fn post_refresh() {
    let Some(vm) = VM.get() else { return };
    vm.with_attached(|env| {
        let runnable = REFRESH_RUNNABLE.lock().unwrap_or_else(|e| e.into_inner());
        let Some(runnable) = runnable.as_ref() else { return };
        with_activity(|activity| {
            let _ = env.call(activity, "runOnUiThread", "(Ljava/lang/Runnable;)V", &[Value::Obj(runnable.0)]);
        });
    });
}

fn set_text(env: Env, activity: jobject, view: &str, text: &str) {
    let (Ok(view), Ok(text)) = (find_view(env, activity, view), env.string(text)) else { return };
    let text = Local::new(env, text);
    let _ = env.call(view.obj, "setText", "(Ljava/lang/CharSequence;)V", &[Value::Obj(text.obj)]);
}

fn set_enabled(env: Env, activity: jobject, view: &str, enabled: bool) {
    if let Ok(view) = find_view(env, activity, view) {
        let _ = env.call(view.obj, "setEnabled", "(Z)V", &[Value::Bool(enabled)]);
    }
}

fn abi() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64-v8a",
        "arm" => "armeabi-v7a",
        "x86_64" => "x86_64",
        other => other,
    }
}

fn cores() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

fn free_storage(env: Env, context: jobject) -> u64 {
    let read = || {
        let dir = Local::new(env, env.call(context, "getFilesDir", "()Ljava/io/File;", &[]).ok()?.obj());
        match env.call(dir.obj, "getUsableSpace", "()J", &[]).ok()? {
            Value::Long(bytes) => Some(bytes.max(0) as u64),
            _ => None,
        }
    };
    read().unwrap_or(0)
}

/// Why this device cannot run the check at all, if it cannot.
fn device_problem() -> Option<String> {
    if cfg!(not(target_pointer_width = "64")) {
        return Some(format!("This device runs 32-bit apps ({}); the AI check needs a 64-bit CPU.", abi()));
    }
    let cores = cores();
    (cores < MIN_CORES).then(|| format!("Only {cores} CPU cores; at least {MIN_CORES} are needed for usable speed."))
}

fn recommended() -> Option<&'static grammar::CatalogModel> {
    grammar::recommended_model(grammar::memory_info()?.0)
}

/// Measures the device and says which model fits, or why none does.
fn device_report(env: Env, context: jobject) -> String {
    use grammar::size_text;
    let (total, available) = grammar::memory_info().unwrap_or((0, 0));
    let (power_save, _) = crate::power_state(env, context);
    let storage = free_storage(env, context);
    let mut text = format!(
        "RAM: {} total, {} free now\nCPU: {} cores ({})\nStorage: {} free",
        size_text(total),
        size_text(available),
        cores(),
        abi(),
        size_text(storage)
    );
    if power_save {
        text.push_str("\nBattery saver is on: the AI will use one CPU core.");
    }
    text.push_str("\n\n");
    if let Some(problem) = device_problem() {
        text.push_str(&format!("✗ {problem}"));
        return text;
    }
    let budget = total / 4;
    match recommended() {
        Some(model) => {
            let needed = grammar::estimate_runtime_bytes(model.file_bytes);
            text.push_str(&format!("✓ Suitable model: {} ({} download)\n{}\n", model.name, size_text(model.file_bytes), model.note));
            text.push_str(&format!(
                "• Up to {} of RAM (a quarter of {}) can be used for the AI; it needs about {}.\n",
                size_text(budget),
                size_text(total),
                size_text(needed)
            ));
            text.push_str(&format!("• The AI will use {} of {} cores, at low priority.", grammar::default_threads(), cores()));
            if model.file_bytes + MIN_FREE_STORAGE_AFTER_IMPORT > storage {
                text.push_str(&format!("\n• Free storage is low: the file needs {}.", size_text(model.file_bytes)));
            }
        }
        None => {
            let smallest = grammar::CATALOG.last().map_or(0, |m| grammar::estimate_runtime_bytes(m.file_bytes));
            text.push_str(&format!(
                "✗ No model is suitable: the smallest needs about {} of RAM, but only {} (a quarter of {}) can be used without slowing other apps.",
                size_text(smallest),
                size_text(budget),
                size_text(total)
            ));
        }
    }
    text
}

/// Called when the screen opens: a speed test that never answered (for example because it was
/// cancelled) no longer blocks the buttons.
pub fn on_open(env: Env) {
    let mut runnable = REFRESH_RUNNABLE.lock().unwrap_or_else(|e| e.into_inner());
    if runnable.is_none() {
        *runnable = callback(env, REFRESH).ok().map(|r| Global(env.new_global(r.obj)));
    }
    drop(runnable);
    let mut state = state();
    if state.testing {
        state.testing = false;
        state.status.clear();
    }
}

/// Shows the current state on the screen.
pub fn refresh(env: Env, activity: jobject) {
    let installed = model_path(env, activity).and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len()).filter(|&n| n > 0);
    let state = state();
    let busy = state.busy || state.testing;
    let usable = device_problem().is_none();
    let status = if !state.status.is_empty() {
        state.status.clone()
    } else if let Some(size) = installed {
        let name =
            prefs::get_string(env, activity, prefs::KEY_MODEL_NAME).filter(|n| !n.is_empty()).unwrap_or_else(|| "grammar.gguf".into());
        format!("Installed: {name} ({}).", grammar::size_text(size))
    } else if let Some(model) = recommended().filter(|_| usable) {
        format!("No model installed. Download {} in your browser, then tap \"Install model file (.gguf)…\" and pick the file.", model.name)
    } else {
        String::new()
    };
    set_text(env, activity, "ai_status", &status);
    if let Some(report) = &state.report {
        set_text(env, activity, "device_report", report);
    }
    if let Some(model) = recommended() {
        set_text(env, activity, "ai_download", &format!("Download {}", model.name));
    }
    set_enabled(env, activity, "ai_download", usable && !busy);
    set_enabled(env, activity, "ai_import", usable && !busy);
    set_enabled(env, activity, "ai_test", usable && installed.is_some() && !busy);
    set_enabled(env, activity, "ai_remove", installed.is_some() && !busy);
    if let Ok(progress) = find_view(env, activity, "ai_progress") {
        let _ = env.call(progress.obj, "setVisibility", "(I)V", &[Value::Int(if state.progress.is_some() { VISIBLE } else { GONE })]);
        let _ = env.call(progress.obj, "setProgress", "(I)V", &[Value::Int(state.progress.unwrap_or(0))]);
    }
}

fn start_activity(env: Env, activity: jobject, intent: jobject) -> JResult<Value> {
    env.call(activity, "startActivity", "(Landroid/content/Intent;)V", &[Value::Obj(intent)])
}

/// Opens the recommended model's download page in the browser.
fn open_download(env: Env, activity: jobject) {
    let Some(model) = recommended().or(grammar::CATALOG.last()) else { return };
    let opened = (|| -> JResult<()> {
        let url = Local::new(env, env.string(model.url)?);
        let uri = Local::new(
            env,
            env.call_static("android/net/Uri", "parse", "(Ljava/lang/String;)Landroid/net/Uri;", &[Value::Obj(url.obj)])?.obj(),
        );
        let action = Local::new(env, env.string("android.intent.action.VIEW")?);
        let intent = Local::new(
            env,
            env.new_object(
                "android/content/Intent",
                "(Ljava/lang/String;Landroid/net/Uri;)V",
                &[Value::Obj(action.obj), Value::Obj(uri.obj)],
            )?,
        );
        start_activity(env, activity, intent.obj).map(|_| ())
    })();
    if opened.is_err() {
        state().status = format!("No browser found. Download this file on another device:\n{}", model.url);
    }
}

/// Lets the user pick the downloaded file; the result arrives in [`on_activity_result`].
fn pick_model_file(env: Env, activity: jobject) -> JResult<()> {
    let action = Local::new(env, env.string("android.intent.action.OPEN_DOCUMENT")?);
    let intent = Local::new(env, env.new_object("android/content/Intent", "(Ljava/lang/String;)V", &[Value::Obj(action.obj)])?);
    let category = Local::new(env, env.string("android.intent.category.OPENABLE")?);
    let all = Local::new(env, env.string("*/*")?);
    env.call(intent.obj, "addCategory", "(Ljava/lang/String;)Landroid/content/Intent;", &[Value::Obj(category.obj)])
        .map(|v| env.delete_local(v.obj()))?;
    env.call(intent.obj, "setType", "(Ljava/lang/String;)Landroid/content/Intent;", &[Value::Obj(all.obj)])
        .map(|v| env.delete_local(v.obj()))?;
    env.call(
        activity,
        "startActivityForResult",
        "(Landroid/content/Intent;I)V",
        &[Value::Obj(intent.obj), Value::Int(REQUEST_MODEL_FILE)],
    )?;
    Ok(())
}

pub fn on_activity_result(env: Env, activity: jobject, request: jint, result: jint, data: jobject) {
    if request != REQUEST_MODEL_FILE || result != RESULT_OK || data.is_null() {
        return;
    }
    let Ok(uri) = env.call(data, "getData", "()Landroid/net/Uri;", &[]) else { return };
    let uri = Local::new(env, uri.obj());
    if uri.obj.is_null() {
        return;
    }
    let (Some(target), Ok(context)) =
        (model_path(env, activity), env.call(activity, "getApplicationContext", "()Landroid/content/Context;", &[]))
    else {
        return;
    };
    let context = Local::new(env, context.obj());
    let cache_dir = prompt_cache_dir(env, activity);
    let storage = free_storage(env, activity);
    {
        let mut state = state();
        state.busy = true;
        state.progress = Some(0);
        state.status = "Installing the model…".into();
    }
    let (uri, context) = (Global(env.new_global(uri.obj)), Global(env.new_global(context.obj)));
    let spawned = std::thread::Builder::new().name("min-keyboard-model-import".into()).spawn(move || {
        // Move the whole `Send` wrappers into the thread, not just their pointer fields.
        let (uri, context) = (uri, context);
        let outcome = VM.get().and_then(|vm| {
            vm.with_attached(|env| {
                let outcome = import(env, context.0, uri.0, &target, storage);
                if let Ok(name) = &outcome {
                    prefs::set_string(env, context.0, prefs::KEY_MODEL_NAME, name);
                    if let Some(dir) = &cache_dir {
                        clear_dir(dir);
                    }
                    prepare(env, context.0, name.clone());
                }
                env.delete_global(uri.0);
                env.delete_global(context.0);
                outcome
            })
        });
        let mut state = state();
        state.busy = false;
        state.progress = None;
        match outcome {
            Some(Ok(_)) => {} // `prepare` reports from here on
            Some(Err(message)) => state.status = format!("Install failed: {message}"),
            None => state.status = "Install failed.".into(),
        }
        drop(state);
        post_refresh();
    });
    if let Err(e) = spawned {
        log_error(&format!("cannot start the import thread: {e}"));
        *state() = State::default();
    }
    refresh(env, activity);
}

fn clear_dir(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Copies the picked file next to the final location, checks that it is a usable model and moves
/// it into place. Returns the model's display name.
fn import(env: Env, context: jobject, uri: jobject, target: &Path, storage: u64) -> Result<String, String> {
    let failed = |what: &str| format!("could not {what}.");
    let resolver = Local::new(
        env,
        env.call(context, "getContentResolver", "()Landroid/content/ContentResolver;", &[]).map_err(|_| failed("open the file"))?.obj(),
    );
    let size = file_size(env, resolver.obj, uri);
    if let Some(size) = size {
        if let Some(problem) = grammar::size_problem(size) {
            return Err(problem);
        }
        if size + MIN_FREE_STORAGE_AFTER_IMPORT > storage {
            return Err(format!(
                "Not enough free storage: the file is {} and {} is free.",
                grammar::size_text(size),
                grammar::size_text(storage)
            ));
        }
    }
    let stream = env
        .call(resolver.obj, "openInputStream", "(Landroid/net/Uri;)Ljava/io/InputStream;", &[Value::Obj(uri)])
        .map_err(|_| failed("open the file"))?;
    let stream = Local::new(env, stream.obj());
    let partial = target.with_extension("part");
    let copied = copy_stream(env, stream.obj, &partial, size);
    let _ = env.call(stream.obj, "close", "()V", &[]);
    if let Err(message) = copied {
        let _ = std::fs::remove_file(&partial);
        return Err(message);
    }
    let checked = check_model(&partial);
    if let Err(message) = &checked {
        let _ = std::fs::remove_file(&partial);
        return Err(message.clone());
    }
    // The old model may still be loaded; let it go before replacing the file.
    if let Some(worker) = grammar_worker() {
        worker.release();
    }
    std::fs::rename(&partial, target).map_err(|e| format!("could not save the model: {e}"))?;
    checked
}

/// The size the content provider reports, if it knows it.
fn file_size(env: Env, resolver: jobject, uri: jobject) -> Option<u64> {
    let mode = Local::new(env, env.string("r").ok()?);
    let descriptor = env
        .call(
            resolver,
            "openAssetFileDescriptor",
            "(Landroid/net/Uri;Ljava/lang/String;)Landroid/content/res/AssetFileDescriptor;",
            &[Value::Obj(uri), Value::Obj(mode.obj)],
        )
        .ok()?;
    let descriptor = Local::new(env, descriptor.obj());
    let length = env.call(descriptor.obj, "getLength", "()J", &[]).ok();
    let _ = env.call(descriptor.obj, "close", "()V", &[]);
    match length? {
        Value::Long(n) if n > 0 => Some(n as u64),
        _ => None,
    }
}

fn copy_stream(env: Env, stream: jobject, path: &Path, size: Option<u64>) -> Result<(), String> {
    const CHUNK: usize = 1 << 20;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not create the model folder: {e}"))?;
    }
    let mut file = File::create(path).map_err(|e| format!("could not create the model file: {e}"))?;
    let array = Local::new(env, env.new_byte_array(CHUNK).map_err(|_| "out of memory.".to_string())?);
    let mut buffer = vec![0u8; CHUNK];
    let mut copied = 0u64;
    let mut shown = -1;
    loop {
        let read = env.call(stream, "read", "([B)I", &[Value::Obj(array.obj)]).map_err(|_| "could not read the file.".to_string())?.int();
        if read < 0 {
            break;
        }
        let chunk = &mut buffer[..read as usize];
        env.read_byte_array(array.obj, chunk).map_err(|_| "could not read the file.".to_string())?;
        if copied == 0 && read >= 4 && &chunk[..4] != b"GGUF" {
            return Err("This is not a GGUF model file.".into());
        }
        file.write_all(chunk).map_err(|e| format!("could not write the model: {e}"))?;
        copied += read as u64;
        if let Some(size) = size {
            let percent = (copied * 100 / size.max(1)).min(100) as i32;
            if percent != shown {
                shown = percent;
                state().progress = Some(percent);
                post_refresh();
            }
        }
    }
    file.sync_all().map_err(|e| format!("could not write the model: {e}"))?;
    if copied < 4 {
        return Err("This is not a GGUF model file.".into());
    }
    Ok(())
}

/// Loads the model once to check that the check can use it; returns its display name.
fn check_model(path: &Path) -> Result<String, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if let Some(problem) = grammar::size_problem(size) {
        return Err(problem);
    }
    let model = grammar::model::Model::load(path).map_err(|e| e.to_string())?;
    let known = grammar::CATALOG.iter().find(|m| m.file_bytes == size);
    Ok(match known {
        Some(model) => model.name.to_string(),
        None if model.name.is_empty() => "Your model (not a recommended one)".into(),
        None => format!("{} (not a recommended model)", model.name),
    })
}

/// Processes the fixed part of each language's prompt right after installing, while the user
/// expects to wait, so the first check from the keyboard starts quickly.
fn prepare(env: Env, context: jobject, name: String) {
    {
        let mut state = state();
        state.testing = true;
        state.status = "Preparing the model…".into();
    }
    let on_update = Box::new(move |update: grammar::Update| {
        let mut state = state();
        match update {
            grammar::Update::Status(status) => state.status = status,
            grammar::Update::Partial(_) => {}
            grammar::Update::Done(_) => {
                state.status = format!("Installed: {name}. Turn on the check button above, then select text and tap ✓ Check grammar.");
                state.testing = false;
            }
            grammar::Update::Error(message) => {
                state.status = format!("Installed: {name}, but preparing it failed: {message}");
                state.testing = false;
            }
        }
        drop(state);
        post_refresh();
    });
    start_prepare(env, context, on_update);
}

/// Runs one real check so the user sees how fast the model is on this device.
fn test_speed(env: Env, activity: jobject) {
    const SAMPLE: &str = "she dont like apples and she have two cat";
    {
        let mut state = state();
        state.testing = true;
        state.status = format!("Testing with: \"{SAMPLE}\"");
    }
    let started = Instant::now();
    let on_update = Box::new(move |update: grammar::Update| {
        let mut state = state();
        match update {
            grammar::Update::Status(status) => state.status = status,
            grammar::Update::Partial(text) => state.status = text,
            grammar::Update::Done(corrected) => {
                let seconds = started.elapsed().as_secs_f32();
                let slow = if seconds > 30.0 { "\nThis is slow on this device; a smaller model would answer sooner." } else { "" };
                state.status = format!("Took {seconds:.1} s (including loading the model).\n→ {corrected}{slow}");
                state.testing = false;
            }
            grammar::Update::Error(message) => {
                state.status = message;
                state.testing = false;
            }
        }
        drop(state);
        post_refresh();
    });
    start_check(env, activity, SAMPLE, grammar::Language::English, on_update);
}

fn confirm_remove(env: Env, activity: jobject) -> JResult<()> {
    let size = model_path(env, activity).and_then(|p| std::fs::metadata(p).ok()).map_or(0, |m| m.len());
    let builder =
        Local::new(env, env.new_object("android/app/AlertDialog$Builder", "(Landroid/content/Context;)V", &[Value::Obj(activity)])?);
    let chain =
        |name: &str, signature: &str, args: &[Value]| env.call(builder.obj, name, signature, args).map(|v| env.delete_local(v.obj()));
    let title = Local::new(env, env.string("Remove the AI model?")?);
    let message = Local::new(env, env.string(&format!("This frees {} of storage.", grammar::size_text(size)))?);
    let (remove, cancel) = (Local::new(env, env.string("Remove")?), Local::new(env, env.string("Cancel")?));
    let listener = callback(env, REMOVE_CONFIRMED)?;
    const SET_TEXT: &str = "(Ljava/lang/CharSequence;)Landroid/app/AlertDialog$Builder;";
    const SET_BUTTON: &str = "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;";
    chain("setTitle", SET_TEXT, &[Value::Obj(title.obj)])?;
    chain("setMessage", SET_TEXT, &[Value::Obj(message.obj)])?;
    chain("setPositiveButton", SET_BUTTON, &[Value::Obj(remove.obj), Value::Obj(listener.obj)])?;
    chain("setNegativeButton", SET_BUTTON, &[Value::Obj(cancel.obj), Value::Obj(std::ptr::null_mut())])?;
    chain("show", "()Landroid/app/AlertDialog;", &[])?;
    Ok(())
}

fn remove_model(env: Env, activity: jobject) {
    if let Some(worker) = grammar_worker() {
        worker.release();
    }
    if let Some(path) = model_path(env, activity) {
        let _ = std::fs::remove_file(path);
    }
    if let Some(dir) = prompt_cache_dir(env, activity) {
        clear_dir(&dir);
    }
    prefs::set_string(env, activity, prefs::KEY_MODEL_NAME, "");
    state().status = "The model was removed.".into();
}

pub fn on_click(env: Env, id: i64, activity: jobject) {
    match id {
        DEVICE_CHECK => {
            let report = device_report(env, activity);
            state().report = Some(report);
        }
        DOWNLOAD => open_download(env, activity),
        IMPORT => {
            if pick_model_file(env, activity).is_err() {
                state().status = "No file picker is available on this device.".into();
            }
        }
        TEST => test_speed(env, activity),
        REMOVE => {
            if confirm_remove(env, activity).is_err() {
                log_error("cannot show the remove dialog");
            }
        }
        _ => return,
    }
    refresh(env, activity);
}

pub fn on_run(env: Env, id: i64) {
    if id == REFRESH {
        with_activity(|activity| refresh(env, activity));
    }
}

pub fn on_dialog_click(env: Env, id: i64, which: jint) {
    const BUTTON_POSITIVE: jint = -1;
    if id == REMOVE_CONFIRMED && which == BUTTON_POSITIVE {
        with_activity(|activity| {
            remove_model(env, activity);
            refresh(env, activity);
        });
    }
}
