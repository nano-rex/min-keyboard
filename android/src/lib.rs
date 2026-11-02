//! Min Keyboard for Android, in Rust. The APK's `classes.dex` only declares the classes Android
//! needs (`ConvoyPinyinImeService`, `SetupActivity`, `KeyboardView`, `NativeCallback`) with
//! `native` methods; this library implements them and registers them in `JNI_OnLoad`.

pub mod jni;
pub mod keyboard;
mod platform;
mod prefs;
mod setup;
mod setup_ai;
pub mod theme;

use std::cell::RefCell;
use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use jni::{jboolean, jint, jobject, Env, Global, JResult, Local, RawEnv, RawVm, Value, Vm, JNI_VERSION_1_6};
use keyboard::{Canvas, Editor, Engines, Host, Keyboard, Proofreader, Rect, TextStyle, Touch};

pub use platform::log_error;

const SERVICE_CLASS: &str = "org/convoy/pinyinime/ConvoyPinyinImeService";
const SETUP_CLASS: &str = "org/convoy/pinyinime/SetupActivity";
const VIEW_CLASS: &str = "org/convoy/pinyinime/KeyboardView";
const CALLBACK_CLASS: &str = "org/convoy/pinyinime/NativeCallback";
const SERVICE_SUPER: &str = "android/inputmethodservice/InputMethodService";

/// `NativeCallback.id` values. The settings screen uses its own range (see `setup`).
const CALLBACK_TIMER: i64 = 1;

static VM: OnceLock<Vm> = OnceLock::new();
/// The keyboard view, for background threads that need it redrawn.
static VIEW: Mutex<Option<Global>> = Mutex::new(None);

/// Replaces the view shared with background threads, releasing the previous global reference.
fn set_shared_view(env: Env, view: Option<jobject>) {
    let mut shared = VIEW.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(old) = shared.take() {
        env.delete_global(old.0);
    }
    *shared = view.map(|v| Global(env.new_global(v)));
}

/// The AI grammar check's background thread, shared by the keyboard and the settings screen so
/// they use one loaded model.
fn grammar_worker() -> Option<&'static grammar::Worker> {
    static WORKER: OnceLock<Option<grammar::Worker>> = OnceLock::new();
    WORKER.get_or_init(|| grammar::Worker::start().map_err(|e| log_error(&format!("cannot start the AI check thread: {e}"))).ok()).as_ref()
}

/// A `java.io.File` method returning a path, such as `getFilesDir`, as a Rust path.
fn context_dir(env: Env, context: jobject, method: &str) -> Option<PathBuf> {
    let dir = Local::new(env, env.call(context, method, "()Ljava/io/File;", &[]).ok()?.obj());
    let path = Local::new(env, env.call(dir.obj, "getPath", "()Ljava/lang/String;", &[]).ok()?.obj());
    Some(PathBuf::from(env.read_string(path.obj)))
}

/// Where the AI model is installed (the same place as in version 0.2, so it carries over).
pub fn model_path(env: Env, context: jobject) -> Option<PathBuf> {
    Some(context_dir(env, context, "getFilesDir")?.join("models").join("grammar.gguf"))
}

/// Where the processed fixed part of the AI prompt is kept; Android may clear it at any time.
pub fn prompt_cache_dir(env: Env, context: jobject) -> Option<PathBuf> {
    let dir = context_dir(env, context, "getCacheDir")?.join("grammar");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Battery saver and overheating, which limit or block the AI check: (power save, too hot).
pub fn power_state(env: Env, context: jobject) -> (bool, bool) {
    let read = || {
        let name = Local::new(env, env.string("power").ok()?);
        let power = Local::new(
            env,
            env.call(context, "getSystemService", "(Ljava/lang/String;)Ljava/lang/Object;", &[Value::Obj(name.obj)]).ok()?.obj(),
        );
        let saving = env.call(power.obj, "isPowerSaveMode", "()Z", &[]).map(Value::bool).unwrap_or(false);
        // THERMAL_STATUS_SEVERE and above; the call exists from Android 10.
        const THERMAL_STATUS_SEVERE: i32 = 3;
        let hot = env.call(power.obj, "getCurrentThermalStatus", "()I", &[]).map(Value::int).unwrap_or(0) >= THERMAL_STATUS_SEVERE;
        Some((saving, hot))
    };
    read().unwrap_or((false, false))
}

/// Starts a check on the shared worker, after the checks that need the Android context.
pub fn start_check(env: Env, context: jobject, text: &str, language: grammar::Language, on_update: Box<dyn Fn(grammar::Update) + Send>) {
    if cfg!(not(target_pointer_width = "64")) {
        return on_update(grammar::Update::Error("The AI check needs a 64-bit CPU.".into()));
    }
    let (power_save, too_hot) = power_state(env, context);
    if too_hot {
        return on_update(grammar::Update::Error("The device is too hot. Try again when it has cooled down.".into()));
    }
    let (Some(worker), Some(model)) = (grammar_worker(), model_path(env, context)) else {
        return on_update(grammar::Update::Error("The AI check could not start.".into()));
    };
    worker.check(grammar::Request {
        model,
        cache_dir: prompt_cache_dir(env, context),
        text: text.to_string(),
        language,
        threads: if power_save { 1 } else { grammar::default_threads() },
        on_update,
    });
}

/// The keyboard's [`Proofreader`]: checks run on the shared worker and redraw the keyboard.
struct AndroidProofreader {
    context: Global,
}

impl Proofreader for AndroidProofreader {
    fn check(&mut self, text: &str, language: grammar::Language, on_update: Box<dyn Fn(grammar::Update) + Send>) {
        let Some(env) = VM.get().and_then(Vm::env) else { return };
        let on_update = Box::new(move |update| {
            on_update(update);
            redraw_from_any_thread();
        });
        start_check(env, self.context.0, text, language, on_update);
    }

    fn cancel(&mut self) {
        if let Some(worker) = grammar_worker() {
            worker.cancel();
        }
    }

    fn release(&mut self) {
        if let Some(worker) = grammar_worker() {
            worker.release();
        }
    }
}

impl Drop for AndroidProofreader {
    fn drop(&mut self) {
        if let Some(env) = VM.get().and_then(Vm::env) {
            env.delete_global(self.context.0);
        }
    }
}

/// Runs a native method body, turning a panic into a log line instead of undefined behaviour.
fn guard<R>(default: R, body: impl FnOnce() -> R) -> R {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or(default)
}

// ----------------------------------------------------------------- service ----

struct Service {
    service: Global,
    keyboard: Keyboard,
    view: Option<Global>,
    timer: Option<Global>,
    paint: Option<Paint>,
}

thread_local! {
    /// The input method service. Android calls it on the main thread only.
    static SERVICE: RefCell<Option<Service>> = const { RefCell::new(None) };
}

fn with_service<R>(f: impl FnOnce(&mut Service) -> R) -> Option<R> {
    SERVICE.with(|cell| match cell.try_borrow_mut() {
        Ok(mut service) => service.as_mut().map(f),
        Err(_) => {
            log_error("re-entrant keyboard call ignored");
            None
        }
    })
}

fn display_metrics(env: Env, context: jobject) -> JResult<(f32, f32, i32)> {
    let resources = Local::new(env, env.call(context, "getResources", "()Landroid/content/res/Resources;", &[])?.obj());
    let metrics = Local::new(env, env.call(resources.obj, "getDisplayMetrics", "()Landroid/util/DisplayMetrics;", &[])?.obj());
    let density = env.field(metrics.obj, "density", "F")?.float();
    let scaled = env.field(metrics.obj, "scaledDensity", "F")?.float();
    let width = env.field(metrics.obj, "widthPixels", "I")?.int();
    Ok((density, scaled, width))
}

extern "system" fn service_on_create(raw: RawEnv, this: jobject) {
    guard((), || {
        let env = unsafe { Env::from_raw(raw) };
        let _ = env.call_super(this, SERVICE_SUPER, "onCreate", "()V", &[]);
        let Ok(assets) = env.call(this, "getAssets", "()Landroid/content/res/AssetManager;", &[]) else { return };
        let Some(assets) = platform::ApkAssets::new(env, assets.obj()) else { return log_error("no asset manager") };
        let engines = Engines::new(Arc::new(assets), Box::new(redraw_from_any_thread));
        let (density, scaled, _) = display_metrics(env, this).unwrap_or((1.0, 1.0, 0));
        let mut keyboard = Keyboard::new(engines, prefs::load(env, this), density, scaled);
        keyboard.set_proofreader(Some(Box::new(AndroidProofreader { context: Global(env.new_global(this)) })));
        let service = Service { service: Global(env.new_global(this)), keyboard, view: None, timer: None, paint: None };
        SERVICE.with(|cell| *cell.borrow_mut() = Some(service));
    })
}

extern "system" fn service_on_destroy(raw: RawEnv, this: jobject) {
    guard((), || {
        let env = unsafe { Env::from_raw(raw) };
        set_shared_view(env, None);
        if let Some(service) = SERVICE.with(|cell| cell.borrow_mut().take()) {
            for global in [Some(service.service), service.view, service.timer].into_iter().flatten() {
                env.delete_global(global.0);
            }
            if let Some(paint) = service.paint {
                paint.release(env);
            }
        }
        let _ = env.call_super(this, SERVICE_SUPER, "onDestroy", "()V", &[]);
    })
}

extern "system" fn service_on_create_input_view(raw: RawEnv, this: jobject) -> jobject {
    guard(std::ptr::null_mut(), || {
        let env = unsafe { Env::from_raw(raw) };
        let Ok(view) = env.new_object(VIEW_CLASS, "(Landroid/content/Context;)V", &[Value::Obj(this)]) else {
            return std::ptr::null_mut();
        };
        let timer = env.new_object(CALLBACK_CLASS, "(J)V", &[Value::Long(CALLBACK_TIMER)]).ok();
        let settings = prefs::load(env, this);
        let paint = Paint::new(env);
        with_service(|service| {
            for old in [service.view.take(), service.timer.take()].into_iter().flatten() {
                env.delete_global(old.0);
            }
            if let Some(old) = service.paint.take() {
                old.release(env);
            }
            service.view = Some(Global(env.new_global(view)));
            service.timer = timer.map(|t| {
                let global = Global(env.new_global(t));
                env.delete_local(t);
                global
            });
            service.paint = paint;
            service.keyboard.apply_settings(settings);
        });
        set_shared_view(env, Some(view));
        view
    })
}

extern "system" fn service_on_start_input(raw: RawEnv, this: jobject, info: jobject, restarting: jboolean) {
    guard((), || {
        let env = unsafe { Env::from_raw(raw) };
        let _ = env.call_super(
            this,
            SERVICE_SUPER,
            "onStartInput",
            "(Landroid/view/inputmethod/EditorInfo;Z)V",
            &[Value::Obj(info), Value::Bool(restarting != 0)],
        );
        let field = |name: &str| env.field(info, name, "I").map(Value::int).unwrap_or(-1);
        let has_selection = !info.is_null() && field("initialSelStart") != field("initialSelEnd");
        with_service(|service| {
            let mut host = AndroidHost::new(env, service);
            service.keyboard.on_start_input(&mut host, has_selection);
        });
    })
}

extern "system" fn service_on_trim_memory(raw: RawEnv, this: jobject, level: jint) {
    guard((), || {
        let env = unsafe { Env::from_raw(raw) };
        let _ = env.call_super(this, SERVICE_SUPER, "onTrimMemory", "(I)V", &[Value::Int(level)]);
        // Give the AI model's memory back as soon as the system asks for it.
        const TRIM_MEMORY_RUNNING_MODERATE: i32 = 5;
        if level >= TRIM_MEMORY_RUNNING_MODERATE {
            with_service(|service| service.keyboard.release_proofreader());
        }
    })
}

extern "system" fn service_on_low_memory(raw: RawEnv, this: jobject) {
    guard((), || {
        let env = unsafe { Env::from_raw(raw) };
        let _ = env.call_super(this, SERVICE_SUPER, "onLowMemory", "()V", &[]);
        with_service(|service| service.keyboard.release_proofreader());
    })
}

extern "system" fn service_on_start_input_view(raw: RawEnv, this: jobject, info: jobject, restarting: jboolean) {
    guard((), || {
        let env = unsafe { Env::from_raw(raw) };
        let _ = env.call_super(
            this,
            SERVICE_SUPER,
            "onStartInputView",
            "(Landroid/view/inputmethod/EditorInfo;Z)V",
            &[Value::Obj(info), Value::Bool(restarting != 0)],
        );
        let settings = prefs::load(env, this);
        with_service(|service| {
            service.keyboard.apply_settings(settings);
            if let Some(view) = &service.view {
                let _ = env.call(view.0, "requestLayout", "()V", &[]);
                let _ = env.call(view.0, "invalidate", "()V", &[]);
            }
        });
    })
}

extern "system" fn service_on_finish_input_view(raw: RawEnv, this: jobject, finishing: jboolean) {
    guard((), || {
        let env = unsafe { Env::from_raw(raw) };
        let _ = env.call_super(this, SERVICE_SUPER, "onFinishInputView", "(Z)V", &[Value::Bool(finishing != 0)]);
        with_service(|service| {
            let mut host = AndroidHost::new(env, service);
            service.keyboard.on_finish_input_view(&mut host);
        });
    })
}

#[allow(clippy::too_many_arguments)]
extern "system" fn service_on_update_selection(
    raw: RawEnv,
    this: jobject,
    old_start: jint,
    old_end: jint,
    start: jint,
    end: jint,
    cand_start: jint,
    cand_end: jint,
) {
    guard((), || {
        let env = unsafe { Env::from_raw(raw) };
        let args = [old_start, old_end, start, end, cand_start, cand_end].map(Value::Int);
        let _ = env.call_super(this, SERVICE_SUPER, "onUpdateSelection", "(IIIIII)V", &args);
        with_service(|service| {
            let mut editor = AndroidEditor::new(env, service.service.0);
            let mut host = AndroidHost::new(env, service);
            service.keyboard.on_update_selection(&mut editor, &mut host, start, end, cand_end);
        });
    })
}

/// Asks the keyboard view to redraw; safe to call from the dictionary loading thread.
fn redraw_from_any_thread() {
    let Some(vm) = VM.get() else { return };
    vm.with_attached(|env| {
        if let Some(view) = &*VIEW.lock().unwrap_or_else(|e| e.into_inner()) {
            let _ = env.call(view.0, "postInvalidate", "()V", &[]);
        }
    });
}

// -------------------------------------------------------------------- view ----

extern "system" fn view_on_draw(raw: RawEnv, _this: jobject, canvas: jobject) {
    guard((), || {
        let env = unsafe { Env::from_raw(raw) };
        with_service(|service| {
            let Some(paint) = &mut service.paint else { return };
            let mut canvas = AndroidCanvas { env, canvas, paint, clips: 0 };
            service.keyboard.draw(&mut canvas);
        });
    })
}

extern "system" fn view_on_measure(raw: RawEnv, this: jobject, width_spec: jint, _height_spec: jint) {
    guard((), || {
        let env = unsafe { Env::from_raw(raw) };
        const MEASURE_SPEC_MODE_MASK: i32 = 0b11 << 30;
        let mut width = width_spec & !MEASURE_SPEC_MODE_MASK;
        if width_spec & MEASURE_SPEC_MODE_MASK == 0 {
            // UNSPECIFIED: take the screen width.
            width = display_metrics(env, this).map(|m| m.2).unwrap_or(width);
        }
        let height = with_service(|service| {
            service.keyboard.set_width(width as f32);
            service.keyboard.height().round() as i32
        })
        .unwrap_or(0);
        let _ = env.call(this, "setMeasuredDimension", "(II)V", &[Value::Int(width), Value::Int(height)]);
    })
}

extern "system" fn view_on_touch_event(raw: RawEnv, _this: jobject, event: jobject) -> jboolean {
    guard(0, || {
        let env = unsafe { Env::from_raw(raw) };
        let int = |name: &str, sig: &str, args: &[Value]| env.call(event, name, sig, args).map(Value::int).unwrap_or(0);
        let point = |index: i32| {
            let id = int("getPointerId", "(I)I", &[Value::Int(index)]);
            let x = env.call(event, "getX", "(I)F", &[Value::Int(index)]).map(Value::float).unwrap_or(0.0);
            let y = env.call(event, "getY", "(I)F", &[Value::Int(index)]).map(Value::float).unwrap_or(0.0);
            (id, x, y)
        };
        let action = int("getActionMasked", "()I", &[]);
        let touches: Vec<Touch> = match action {
            0 | 5 => {
                let (id, x, y) = point(int("getActionIndex", "()I", &[]));
                vec![Touch::Down { id, x, y }]
            }
            1 | 6 => {
                let (id, x, y) = point(int("getActionIndex", "()I", &[]));
                vec![Touch::Up { id, x, y }]
            }
            2 => (0..int("getPointerCount", "()I", &[])).map(point).map(|(id, x, y)| Touch::Move { id, x, y }).collect(),
            3 => vec![Touch::Cancel],
            _ => vec![],
        };
        with_service(|service| {
            for touch in touches {
                let mut editor = AndroidEditor::new(env, service.service.0);
                let mut host = AndroidHost::new(env, service);
                let Some(paint) = &mut service.paint else { return };
                let mut canvas = AndroidCanvas { env, canvas: std::ptr::null_mut(), paint, clips: 0 };
                service.keyboard.on_touch(touch, &mut editor, &mut host, &mut canvas);
            }
        });
        1
    })
}

// --------------------------------------------------------------- callbacks ----

extern "system" fn callback_run(raw: RawEnv, this: jobject) {
    guard((), || {
        let env = unsafe { Env::from_raw(raw) };
        match callback_id(env, this) {
            CALLBACK_TIMER => {
                with_service(|service| {
                    let mut editor = AndroidEditor::new(env, service.service.0);
                    let mut host = AndroidHost::new(env, service);
                    service.keyboard.on_timer(&mut editor, &mut host);
                });
            }
            id => setup::on_run(env, id),
        }
    })
}

fn callback_id(env: Env, this: jobject) -> i64 {
    match env.field(this, "id", "J") {
        Ok(Value::Long(id)) => id,
        _ => 0,
    }
}

extern "system" fn callback_on_click(raw: RawEnv, this: jobject, view: jobject) {
    guard((), || {
        let env = unsafe { Env::from_raw(raw) };
        setup::on_click(env, callback_id(env, this), view);
    })
}

extern "system" fn callback_on_dialog_click(raw: RawEnv, this: jobject, _dialog: jobject, which: jint) {
    guard((), || {
        let env = unsafe { Env::from_raw(raw) };
        setup::on_dialog_click(env, callback_id(env, this), which);
    })
}

extern "system" fn callback_on_switch(raw: RawEnv, this: jobject, button: jobject, checked: jboolean) {
    guard((), || {
        let env = unsafe { Env::from_raw(raw) };
        setup::on_switch(env, callback_id(env, this), button, checked != 0);
    })
}

extern "system" fn callback_on_radio(raw: RawEnv, this: jobject, group: jobject, checked_id: jint) {
    guard((), || {
        let env = unsafe { Env::from_raw(raw) };
        setup::on_radio(env, callback_id(env, this), group, checked_id);
    })
}

// ------------------------------------------------------------------ editor ----

/// The current `InputConnection`, looked up once per event.
struct AndroidEditor {
    env: Env,
    service: jobject,
    connection: Option<Local>,
}

impl AndroidEditor {
    fn new(env: Env, service: jobject) -> Self {
        let connection = env
            .call(service, "getCurrentInputConnection", "()Landroid/view/inputmethod/InputConnection;", &[])
            .ok()
            .map(Value::obj)
            .filter(|c| !c.is_null())
            .map(|c| Local::new(env, c));
        AndroidEditor { env, service, connection }
    }

    fn call(&self, name: &str, signature: &str, args: &[Value]) -> Option<Value> {
        self.env.call(self.connection.as_ref()?.obj, name, signature, args).ok()
    }

    fn with_text(&self, text: &str, name: &str) {
        let Ok(s) = self.env.string(text) else { return };
        let s = Local::new(self.env, s);
        self.call(name, "(Ljava/lang/CharSequence;I)Z", &[Value::Obj(s.obj), Value::Int(1)]);
    }
}

impl Editor for AndroidEditor {
    fn connected(&self) -> bool {
        self.connection.is_some()
    }

    fn commit_text(&mut self, text: &str) {
        self.with_text(text, "commitText");
    }

    fn set_composing_text(&mut self, text: &str) {
        self.with_text(text, "setComposingText");
    }

    fn finish_composing_text(&mut self) {
        self.call("finishComposingText", "()Z", &[]);
    }

    fn delete_before(&mut self, count: i32) {
        self.call("deleteSurroundingTextInCodePoints", "(II)Z", &[Value::Int(count), Value::Int(0)]);
    }

    fn selected_text(&mut self) -> Option<String> {
        let text = Local::new(self.env, self.call("getSelectedText", "(I)Ljava/lang/CharSequence;", &[Value::Int(0)])?.obj());
        self.env.read_char_sequence(text.obj)
    }

    fn editor_info(&mut self) -> Option<(i32, i32)> {
        let info = self.env.call(self.service, "getCurrentInputEditorInfo", "()Landroid/view/inputmethod/EditorInfo;", &[]).ok()?.obj();
        if info.is_null() {
            return None;
        }
        let info = Local::new(self.env, info);
        Some((self.env.field(info.obj, "imeOptions", "I").ok()?.int(), self.env.field(info.obj, "inputType", "I").ok()?.int()))
    }

    fn perform_editor_action(&mut self, action: i32) {
        self.call("performEditorAction", "(I)Z", &[Value::Int(action)]);
    }

    fn send_enter_key(&mut self) {
        const KEYCODE_ENTER: i32 = 66;
        for action in [0, 1] {
            if let Ok(event) = self.env.new_object("android/view/KeyEvent", "(II)V", &[Value::Int(action), Value::Int(KEYCODE_ENTER)]) {
                let event = Local::new(self.env, event);
                self.call("sendKeyEvent", "(Landroid/view/KeyEvent;)Z", &[Value::Obj(event.obj)]);
            }
        }
    }
}

// -------------------------------------------------------------------- host ----

struct AndroidHost {
    env: Env,
    view: jobject,
    timer: jobject,
}

impl AndroidHost {
    fn new(env: Env, service: &Service) -> Self {
        let raw = |g: &Option<Global>| g.as_ref().map(|g| g.0).unwrap_or(std::ptr::null_mut());
        AndroidHost { env, view: raw(&service.view), timer: raw(&service.timer) }
    }
}

impl Host for AndroidHost {
    fn haptic(&mut self) {
        const KEYBOARD_TAP: i32 = 3;
        let _ = self.env.call(self.view, "performHapticFeedback", "(I)Z", &[Value::Int(KEYBOARD_TAP)]);
    }

    fn schedule_timer(&mut self, delay_ms: i64) {
        self.cancel_timer();
        let _ = self.env.call(self.view, "postDelayed", "(Ljava/lang/Runnable;J)Z", &[Value::Obj(self.timer), Value::Long(delay_ms)]);
    }

    fn cancel_timer(&mut self) {
        let _ = self.env.call(self.view, "removeCallbacks", "(Ljava/lang/Runnable;)Z", &[Value::Obj(self.timer)]);
    }

    fn invalidate(&mut self) {
        let _ = self.env.call(self.view, "invalidate", "()V", &[]);
    }
}

// ------------------------------------------------------------------ canvas ----

/// A `Paint` and the typefaces and alignments it switches between, with its current state cached.
struct Paint {
    paint: jobject,
    regular: jobject,
    bold: jobject,
    aligns: [jobject; 3],
    state: Option<(f32, bool, keyboard::Align)>,
    color: Option<u32>,
}

impl Paint {
    fn new(env: Env) -> Option<Self> {
        const ANTI_ALIAS_FLAG: i32 = 1;
        let global = |obj: jobject| {
            let g = env.new_global(obj);
            env.delete_local(obj);
            g
        };
        let paint = global(env.new_object("android/graphics/Paint", "(I)V", &[Value::Int(ANTI_ALIAS_FLAG)]).ok()?);
        let typeface =
            |name: &str| env.static_field("android/graphics/Typeface", name, "Landroid/graphics/Typeface;").ok().map(|v| global(v.obj()));
        let align = |name: &str| {
            env.static_field("android/graphics/Paint$Align", name, "Landroid/graphics/Paint$Align;").ok().map(|v| global(v.obj()))
        };
        Some(Paint {
            paint,
            regular: typeface("DEFAULT")?,
            bold: typeface("DEFAULT_BOLD")?,
            aligns: [align("LEFT")?, align("CENTER")?, align("RIGHT")?],
            state: None,
            color: None,
        })
    }

    fn release(self, env: Env) {
        for obj in [self.paint, self.regular, self.bold].into_iter().chain(self.aligns) {
            env.delete_global(obj);
        }
    }

    fn set_color(&mut self, env: Env, color: u32) {
        if self.color != Some(color) {
            let _ = env.call(self.paint, "setColor", "(I)V", &[Value::Int(color as i32)]);
            self.color = Some(color);
        }
    }

    fn set_font(&mut self, env: Env, size: f32, bold: bool, align: keyboard::Align) {
        let previous = self.state;
        if previous.map(|p| p.0) != Some(size) {
            let _ = env.call(self.paint, "setTextSize", "(F)V", &[Value::Float(size)]);
        }
        if previous.map(|p| p.1) != Some(bold) {
            let typeface = if bold { self.bold } else { self.regular };
            let _ = env
                .call(self.paint, "setTypeface", "(Landroid/graphics/Typeface;)Landroid/graphics/Typeface;", &[Value::Obj(typeface)])
                .map(|v| env.delete_local(v.obj()));
        }
        if previous.map(|p| p.2) != Some(align) {
            let index = match align {
                keyboard::Align::Left => 0,
                keyboard::Align::Center => 1,
                keyboard::Align::Right => 2,
            };
            let _ = env.call(self.paint, "setTextAlign", "(Landroid/graphics/Paint$Align;)V", &[Value::Obj(self.aligns[index])]);
        }
        self.state = Some((size, bold, align));
    }
}

/// Draws on an `android.graphics.Canvas`; with a null canvas it only measures text.
struct AndroidCanvas<'a> {
    env: Env,
    canvas: jobject,
    paint: &'a mut Paint,
    clips: u32,
}

impl Canvas for AndroidCanvas<'_> {
    fn fill(&mut self, color: u32) {
        if !self.canvas.is_null() {
            let _ = self.env.call(self.canvas, "drawColor", "(I)V", &[Value::Int(color as i32)]);
        }
    }

    fn round_rect(&mut self, r: Rect, radius: f32, color: u32) {
        if self.canvas.is_null() {
            return;
        }
        self.paint.set_color(self.env, color);
        let args = [r.left, r.top, r.right, r.bottom, radius, radius].map(Value::Float);
        let mut args = args.to_vec();
        args.push(Value::Obj(self.paint.paint));
        let _ = self.env.call(self.canvas, "drawRoundRect", "(FFFFFFLandroid/graphics/Paint;)V", &args);
    }

    fn text(&mut self, text: &str, x: f32, baseline: f32, style: TextStyle) {
        if self.canvas.is_null() || text.is_empty() {
            return;
        }
        self.paint.set_color(self.env, style.color);
        self.paint.set_font(self.env, style.size, style.bold, style.align);
        let Ok(s) = self.env.string(text) else { return };
        let s = Local::new(self.env, s);
        let args = [Value::Obj(s.obj), Value::Float(x), Value::Float(baseline), Value::Obj(self.paint.paint)];
        let _ = self.env.call(self.canvas, "drawText", "(Ljava/lang/String;FFLandroid/graphics/Paint;)V", &args);
    }

    fn measure(&mut self, text: &str, size: f32, bold: bool) -> f32 {
        let align = self.paint.state.map(|s| s.2).unwrap_or(keyboard::Align::Left);
        self.paint.set_font(self.env, size, bold, align);
        let Ok(s) = self.env.string(text) else { return 0.0 };
        let s = Local::new(self.env, s);
        self.env.call(self.paint.paint, "measureText", "(Ljava/lang/String;)F", &[Value::Obj(s.obj)]).map(Value::float).unwrap_or(0.0)
    }

    fn metrics(&mut self, size: f32, bold: bool) -> (f32, f32) {
        let align = self.paint.state.map(|s| s.2).unwrap_or(keyboard::Align::Left);
        self.paint.set_font(self.env, size, bold, align);
        let get = |name: &str| self.env.call(self.paint.paint, name, "()F", &[]).map(Value::float).unwrap_or(0.0);
        (get("ascent"), get("descent"))
    }

    fn clip(&mut self, r: Rect) {
        if self.canvas.is_null() {
            return;
        }
        let _ = self.env.call(self.canvas, "save", "()I", &[]);
        let _ = self.env.call(self.canvas, "clipRect", "(FFFF)Z", &[r.left, r.top, r.right, r.bottom].map(Value::Float));
        self.clips += 1;
    }

    fn unclip(&mut self) {
        if self.canvas.is_null() || self.clips == 0 {
            return;
        }
        let _ = self.env.call(self.canvas, "restore", "()V", &[]);
        self.clips -= 1;
    }
}

// ------------------------------------------------------------ registration ----

/// Registers the native methods of all four classes when the library is loaded.
#[no_mangle]
pub extern "system" fn JNI_OnLoad(vm: RawVm, _reserved: *mut c_void) -> jint {
    std::panic::set_hook(Box::new(|info| log_error(&format!("panic: {info}"))));
    let vm = Vm(vm);
    let _ = VM.set(vm);
    let Some(env) = vm.env() else { return -1 };
    let f = |p: *const ()| p as *mut c_void;
    type Natives = Vec<(&'static str, &'static str, *mut c_void)>;
    let classes: [(&str, Natives); 4] = [
        (
            SERVICE_CLASS,
            vec![
                ("onCreate", "()V", f(service_on_create as *const ())),
                ("onDestroy", "()V", f(service_on_destroy as *const ())),
                ("onCreateInputView", "()Landroid/view/View;", f(service_on_create_input_view as *const ())),
                ("onStartInput", "(Landroid/view/inputmethod/EditorInfo;Z)V", f(service_on_start_input as *const ())),
                ("onStartInputView", "(Landroid/view/inputmethod/EditorInfo;Z)V", f(service_on_start_input_view as *const ())),
                ("onFinishInputView", "(Z)V", f(service_on_finish_input_view as *const ())),
                ("onUpdateSelection", "(IIIIII)V", f(service_on_update_selection as *const ())),
                ("onTrimMemory", "(I)V", f(service_on_trim_memory as *const ())),
                ("onLowMemory", "()V", f(service_on_low_memory as *const ())),
            ],
        ),
        (
            SETUP_CLASS,
            vec![
                ("onCreate", "(Landroid/os/Bundle;)V", f(setup::on_create as *const ())),
                ("onActivityResult", "(IILandroid/content/Intent;)V", f(setup::on_activity_result as *const ())),
            ],
        ),
        (
            VIEW_CLASS,
            vec![
                ("onDraw", "(Landroid/graphics/Canvas;)V", f(view_on_draw as *const ())),
                ("onMeasure", "(II)V", f(view_on_measure as *const ())),
                ("onTouchEvent", "(Landroid/view/MotionEvent;)Z", f(view_on_touch_event as *const ())),
            ],
        ),
        (
            CALLBACK_CLASS,
            vec![
                ("run", "()V", f(callback_run as *const ())),
                ("onClick", "(Landroid/view/View;)V", f(callback_on_click as *const ())),
                ("onClick", "(Landroid/content/DialogInterface;I)V", f(callback_on_dialog_click as *const ())),
                ("onCheckedChanged", "(Landroid/widget/CompoundButton;Z)V", f(callback_on_switch as *const ())),
                ("onCheckedChanged", "(Landroid/widget/RadioGroup;I)V", f(callback_on_radio as *const ())),
            ],
        ),
    ];
    for (name, methods) in &classes {
        let Ok(class) = env.find_class(name) else {
            log_error(&format!("class {name} not found"));
            return -1;
        };
        let registered = env.register_natives(class, methods);
        env.delete_local(class);
        if registered.is_err() {
            log_error(&format!("cannot register natives for {name}"));
            return -1;
        }
    }
    JNI_VERSION_1_6
}
