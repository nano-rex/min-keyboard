//! Minimal JNI bindings. The function table indices follow `jni.h`; only the calls this app
//! uses are wrapped. Every call that can throw clears the exception and reports `Err`, so a
//! failing framework call (for example on a closed input connection) never crashes the IME.

// JNI references (`jobject`) are opaque handles that are only passed back to the VM, never
// dereferenced here, so functions taking them need not be `unsafe`.
#![allow(non_camel_case_types, non_snake_case, clippy::missing_safety_doc, clippy::not_unsafe_ptr_arg_deref)]

use std::ffi::{c_char, c_void, CString};
use std::ptr::null_mut;

pub type jint = i32;
pub type jlong = i64;
pub type jboolean = u8;
pub type jfloat = f32;
pub type jobject = *mut c_void;
pub type jclass = jobject;
pub type jmethodID = *mut c_void;
pub type jfieldID = *mut c_void;

pub const JNI_VERSION_1_6: jint = 0x0001_0006;

#[repr(C)]
#[derive(Clone, Copy)]
pub union jvalue {
    pub z: jboolean,
    pub i: jint,
    pub j: jlong,
    pub f: jfloat,
    pub l: jobject,
}

#[repr(C)]
pub struct JNINativeMethod {
    pub name: *const c_char,
    pub signature: *const c_char,
    pub fn_ptr: *mut c_void,
}

/// `JNIEnv*`: a pointer to a pointer to the function table.
pub type RawEnv = *mut *const *const c_void;
/// `JavaVM*`.
pub type RawVm = *mut *const *const c_void;

// Indices into JNINativeInterface (see jni.h).
const FIND_CLASS: usize = 6;
const EXCEPTION_DESCRIBE: usize = 16;
const EXCEPTION_CLEAR: usize = 17;
const NEW_GLOBAL_REF: usize = 21;
const DELETE_GLOBAL_REF: usize = 22;
const DELETE_LOCAL_REF: usize = 23;
const NEW_OBJECT_A: usize = 30;
const GET_OBJECT_CLASS: usize = 31;
const IS_INSTANCE_OF: usize = 32;
const GET_METHOD_ID: usize = 33;
const CALL_OBJECT_METHOD_A: usize = 36;
const CALL_BOOLEAN_METHOD_A: usize = 39;
const CALL_INT_METHOD_A: usize = 51;
const CALL_LONG_METHOD_A: usize = 54;
const CALL_FLOAT_METHOD_A: usize = 57;
const CALL_VOID_METHOD_A: usize = 63;
const CALL_NONVIRTUAL_OBJECT_METHOD_A: usize = 66;
const CALL_NONVIRTUAL_BOOLEAN_METHOD_A: usize = 69;
const CALL_NONVIRTUAL_VOID_METHOD_A: usize = 93;
const GET_FIELD_ID: usize = 94;
const GET_OBJECT_FIELD: usize = 95;
const GET_INT_FIELD: usize = 100;
const GET_LONG_FIELD: usize = 101;
const GET_FLOAT_FIELD: usize = 102;
const GET_STATIC_METHOD_ID: usize = 113;
const CALL_STATIC_OBJECT_METHOD_A: usize = 116;
const CALL_STATIC_VOID_METHOD_A: usize = 143;
const GET_STATIC_FIELD_ID: usize = 144;
const GET_STATIC_OBJECT_FIELD: usize = 145;
const GET_STATIC_INT_FIELD: usize = 150;
const NEW_STRING: usize = 163;
const GET_STRING_LENGTH: usize = 164;
const REGISTER_NATIVES: usize = 215;
const GET_STRING_REGION: usize = 220;
const EXCEPTION_CHECK: usize = 228;

// Indices into JNIInvokeInterface.
const ATTACH_CURRENT_THREAD: usize = 4;
const DETACH_CURRENT_THREAD: usize = 5;
const GET_ENV: usize = 6;

/// A Java value passed to or returned from a call.
#[derive(Clone, Copy, Debug)]
pub enum Value {
    Void,
    Bool(bool),
    Int(i32),
    Long(i64),
    Float(f32),
    Obj(jobject),
}

impl Value {
    fn raw(self) -> jvalue {
        match self {
            Value::Void => jvalue { j: 0 },
            Value::Bool(b) => jvalue { z: b as u8 },
            Value::Int(i) => jvalue { i },
            Value::Long(j) => jvalue { j },
            Value::Float(f) => jvalue { f },
            Value::Obj(l) => jvalue { l },
        }
    }

    pub fn obj(self) -> jobject {
        if let Value::Obj(o) = self {
            o
        } else {
            null_mut()
        }
    }

    pub fn bool(self) -> bool {
        matches!(self, Value::Bool(true))
    }

    pub fn int(self) -> i32 {
        if let Value::Int(i) = self {
            i
        } else {
            0
        }
    }

    pub fn float(self) -> f32 {
        if let Value::Float(f) = self {
            f
        } else {
            0.0
        }
    }
}

/// A failed Java call; the exception has already been printed to the log and cleared.
#[derive(Debug)]
pub struct JavaException;

pub type JResult<T> = Result<T, JavaException>;

#[derive(Clone, Copy)]
pub struct Env {
    raw: RawEnv,
}

macro_rules! jni_fn {
    ($self:ident, $index:expr, fn($($arg:ty),*) -> $ret:ty) => {{
        let table = *$self.raw;
        let f: unsafe extern "system" fn(RawEnv, $($arg),*) -> $ret = std::mem::transmute(*table.add($index));
        f
    }};
}

fn cstring(text: &str) -> CString {
    CString::new(text).expect("JNI names never contain NUL")
}

/// The return type character of a method signature, e.g. `V` for `(I)V`.
fn return_kind(signature: &str) -> u8 {
    let after = signature.rfind(')').map(|i| i + 1).unwrap_or(0);
    signature.as_bytes().get(after).copied().unwrap_or(b'V')
}

impl Env {
    pub unsafe fn from_raw(raw: RawEnv) -> Self {
        Env { raw }
    }

    pub fn raw(&self) -> RawEnv {
        self.raw
    }

    /// Clears a pending exception, logging it. Returns true if there was one.
    pub fn check_exception(&self) -> bool {
        unsafe {
            if jni_fn!(self, EXCEPTION_CHECK, fn() -> jboolean)(self.raw) != 0 {
                jni_fn!(self, EXCEPTION_DESCRIBE, fn() -> ())(self.raw);
                jni_fn!(self, EXCEPTION_CLEAR, fn() -> ())(self.raw);
                return true;
            }
        }
        false
    }

    fn checked<T>(&self, value: T) -> JResult<T> {
        if self.check_exception() {
            Err(JavaException)
        } else {
            Ok(value)
        }
    }

    pub fn find_class(&self, name: &str) -> JResult<jclass> {
        let name = cstring(name);
        let class = unsafe { jni_fn!(self, FIND_CLASS, fn(*const c_char) -> jclass)(self.raw, name.as_ptr()) };
        self.checked(class).and_then(|c| if c.is_null() { Err(JavaException) } else { Ok(c) })
    }

    pub fn object_class(&self, obj: jobject) -> jclass {
        unsafe { jni_fn!(self, GET_OBJECT_CLASS, fn(jobject) -> jclass)(self.raw, obj) }
    }

    pub fn is_instance_of(&self, obj: jobject, class: jclass) -> bool {
        unsafe { jni_fn!(self, IS_INSTANCE_OF, fn(jobject, jclass) -> jboolean)(self.raw, obj, class) != 0 }
    }

    pub fn delete_local(&self, obj: jobject) {
        if !obj.is_null() {
            unsafe { jni_fn!(self, DELETE_LOCAL_REF, fn(jobject) -> ())(self.raw, obj) }
        }
    }

    pub fn new_global(&self, obj: jobject) -> jobject {
        unsafe { jni_fn!(self, NEW_GLOBAL_REF, fn(jobject) -> jobject)(self.raw, obj) }
    }

    pub fn delete_global(&self, obj: jobject) {
        if !obj.is_null() {
            unsafe { jni_fn!(self, DELETE_GLOBAL_REF, fn(jobject) -> ())(self.raw, obj) }
        }
    }

    fn method_id(&self, class: jclass, name: &str, signature: &str, is_static: bool) -> JResult<jmethodID> {
        let (name, sig) = (cstring(name), cstring(signature));
        let index = if is_static { GET_STATIC_METHOD_ID } else { GET_METHOD_ID };
        let id = unsafe {
            jni_fn!(self, index, fn(jclass, *const c_char, *const c_char) -> jmethodID)(self.raw, class, name.as_ptr(), sig.as_ptr())
        };
        self.checked(id).and_then(|id| if id.is_null() { Err(JavaException) } else { Ok(id) })
    }

    fn field_id(&self, class: jclass, name: &str, signature: &str, is_static: bool) -> JResult<jfieldID> {
        let (name, sig) = (cstring(name), cstring(signature));
        let index = if is_static { GET_STATIC_FIELD_ID } else { GET_FIELD_ID };
        let id = unsafe {
            jni_fn!(self, index, fn(jclass, *const c_char, *const c_char) -> jfieldID)(self.raw, class, name.as_ptr(), sig.as_ptr())
        };
        self.checked(id).and_then(|id| if id.is_null() { Err(JavaException) } else { Ok(id) })
    }

    /// Calls an instance method, dispatching on the signature's return type.
    pub fn call(&self, obj: jobject, name: &str, signature: &str, args: &[Value]) -> JResult<Value> {
        if obj.is_null() {
            return Err(JavaException);
        }
        let class = self.object_class(obj);
        let id = self.method_id(class, name, signature, false);
        self.delete_local(class);
        let id = id?;
        let args: Vec<jvalue> = args.iter().map(|a| a.raw()).collect();
        let a = args.as_ptr();
        let value = unsafe {
            match return_kind(signature) {
                b'V' => {
                    jni_fn!(self, CALL_VOID_METHOD_A, fn(jobject, jmethodID, *const jvalue) -> ())(self.raw, obj, id, a);
                    Value::Void
                }
                b'Z' => Value::Bool(
                    jni_fn!(self, CALL_BOOLEAN_METHOD_A, fn(jobject, jmethodID, *const jvalue) -> jboolean)(self.raw, obj, id, a) != 0,
                ),
                b'I' => Value::Int(jni_fn!(self, CALL_INT_METHOD_A, fn(jobject, jmethodID, *const jvalue) -> jint)(self.raw, obj, id, a)),
                b'J' => {
                    Value::Long(jni_fn!(self, CALL_LONG_METHOD_A, fn(jobject, jmethodID, *const jvalue) -> jlong)(self.raw, obj, id, a))
                }
                b'F' => {
                    Value::Float(jni_fn!(self, CALL_FLOAT_METHOD_A, fn(jobject, jmethodID, *const jvalue) -> jfloat)(self.raw, obj, id, a))
                }
                _ => {
                    Value::Obj(jni_fn!(self, CALL_OBJECT_METHOD_A, fn(jobject, jmethodID, *const jvalue) -> jobject)(self.raw, obj, id, a))
                }
            }
        };
        self.checked(value)
    }

    /// Calls the superclass implementation of a method (`super.name(...)`).
    pub fn call_super(&self, obj: jobject, superclass: &str, name: &str, signature: &str, args: &[Value]) -> JResult<Value> {
        let class = self.find_class(superclass)?;
        let id = self.method_id(class, name, signature, false);
        let args: Vec<jvalue> = args.iter().map(|a| a.raw()).collect();
        let a = args.as_ptr();
        let value = id.map(|id| unsafe {
            match return_kind(signature) {
                b'V' => {
                    jni_fn!(self, CALL_NONVIRTUAL_VOID_METHOD_A, fn(jobject, jclass, jmethodID, *const jvalue) -> ())(
                        self.raw, obj, class, id, a,
                    );
                    Value::Void
                }
                b'Z' => Value::Bool(
                    jni_fn!(self, CALL_NONVIRTUAL_BOOLEAN_METHOD_A, fn(jobject, jclass, jmethodID, *const jvalue) -> jboolean)(
                        self.raw, obj, class, id, a,
                    ) != 0,
                ),
                _ => Value::Obj(jni_fn!(self, CALL_NONVIRTUAL_OBJECT_METHOD_A, fn(jobject, jclass, jmethodID, *const jvalue) -> jobject)(
                    self.raw, obj, class, id, a,
                )),
            }
        });
        self.delete_local(class);
        self.checked(value?)
    }

    pub fn call_static(&self, class_name: &str, name: &str, signature: &str, args: &[Value]) -> JResult<Value> {
        let class = self.find_class(class_name)?;
        let id = self.method_id(class, name, signature, true);
        let args: Vec<jvalue> = args.iter().map(|a| a.raw()).collect();
        let value = id.map(|id| unsafe {
            if return_kind(signature) == b'V' {
                jni_fn!(self, CALL_STATIC_VOID_METHOD_A, fn(jclass, jmethodID, *const jvalue) -> ())(self.raw, class, id, args.as_ptr());
                Value::Void
            } else {
                Value::Obj(jni_fn!(self, CALL_STATIC_OBJECT_METHOD_A, fn(jclass, jmethodID, *const jvalue) -> jobject)(
                    self.raw,
                    class,
                    id,
                    args.as_ptr(),
                ))
            }
        });
        self.delete_local(class);
        self.checked(value?)
    }

    pub fn new_object(&self, class_name: &str, signature: &str, args: &[Value]) -> JResult<jobject> {
        let class = self.find_class(class_name)?;
        let id = self.method_id(class, "<init>", signature, false);
        let args: Vec<jvalue> = args.iter().map(|a| a.raw()).collect();
        let obj = id.map(|id| unsafe {
            jni_fn!(self, NEW_OBJECT_A, fn(jclass, jmethodID, *const jvalue) -> jobject)(self.raw, class, id, args.as_ptr())
        });
        self.delete_local(class);
        self.checked(obj?).and_then(|o| if o.is_null() { Err(JavaException) } else { Ok(o) })
    }

    /// Reads an instance field: `I`, `J`, `F` or an object type.
    pub fn field(&self, obj: jobject, name: &str, signature: &str) -> JResult<Value> {
        if obj.is_null() {
            return Err(JavaException);
        }
        let class = self.object_class(obj);
        let id = self.field_id(class, name, signature, false);
        self.delete_local(class);
        let id = id?;
        let value = unsafe {
            match signature.as_bytes()[0] {
                b'I' => Value::Int(jni_fn!(self, GET_INT_FIELD, fn(jobject, jfieldID) -> jint)(self.raw, obj, id)),
                b'J' => Value::Long(jni_fn!(self, GET_LONG_FIELD, fn(jobject, jfieldID) -> jlong)(self.raw, obj, id)),
                b'F' => Value::Float(jni_fn!(self, GET_FLOAT_FIELD, fn(jobject, jfieldID) -> jfloat)(self.raw, obj, id)),
                _ => Value::Obj(jni_fn!(self, GET_OBJECT_FIELD, fn(jobject, jfieldID) -> jobject)(self.raw, obj, id)),
            }
        };
        self.checked(value)
    }

    /// Reads a static field: `I` or an object type.
    pub fn static_field(&self, class_name: &str, name: &str, signature: &str) -> JResult<Value> {
        let class = self.find_class(class_name)?;
        let id = self.field_id(class, name, signature, true);
        let value = id.map(|id| unsafe {
            if signature == "I" {
                Value::Int(jni_fn!(self, GET_STATIC_INT_FIELD, fn(jclass, jfieldID) -> jint)(self.raw, class, id))
            } else {
                Value::Obj(jni_fn!(self, GET_STATIC_OBJECT_FIELD, fn(jclass, jfieldID) -> jobject)(self.raw, class, id))
            }
        });
        self.delete_local(class);
        self.checked(value?)
    }

    /// A new `java.lang.String`. Built from UTF-16 so characters outside the BMP survive.
    pub fn string(&self, text: &str) -> JResult<jobject> {
        let units: Vec<u16> = text.encode_utf16().collect();
        let s = unsafe { jni_fn!(self, NEW_STRING, fn(*const u16, jint) -> jobject)(self.raw, units.as_ptr(), units.len() as jint) };
        self.checked(s).and_then(|s| if s.is_null() { Err(JavaException) } else { Ok(s) })
    }

    /// Converts a `java.lang.String` to Rust.
    pub fn read_string(&self, s: jobject) -> String {
        if s.is_null() {
            return String::new();
        }
        unsafe {
            let len = jni_fn!(self, GET_STRING_LENGTH, fn(jobject) -> jint)(self.raw, s);
            let mut units = vec![0u16; len.max(0) as usize];
            jni_fn!(self, GET_STRING_REGION, fn(jobject, jint, jint, *mut u16) -> ())(self.raw, s, 0, len, units.as_mut_ptr());
            String::from_utf16_lossy(&units)
        }
    }

    /// Converts any `CharSequence` (or null) to Rust via `toString()`.
    pub fn read_char_sequence(&self, cs: jobject) -> Option<String> {
        if cs.is_null() {
            return None;
        }
        let s = self.call(cs, "toString", "()Ljava/lang/String;", &[]).ok()?.obj();
        let text = self.read_string(s);
        self.delete_local(s);
        Some(text)
    }

    pub fn register_natives(&self, class: jclass, methods: &[(&str, &str, *mut c_void)]) -> JResult<()> {
        let names: Vec<(CString, CString)> = methods.iter().map(|(n, s, _)| (cstring(n), cstring(s))).collect();
        let table: Vec<JNINativeMethod> = names
            .iter()
            .zip(methods)
            .map(|((n, s), (_, _, f))| JNINativeMethod { name: n.as_ptr(), signature: s.as_ptr(), fn_ptr: *f })
            .collect();
        let result = unsafe {
            jni_fn!(self, REGISTER_NATIVES, fn(jclass, *const JNINativeMethod, jint) -> jint)(
                self.raw,
                class,
                table.as_ptr(),
                table.len() as jint,
            )
        };
        if self.check_exception() || result != 0 {
            Err(JavaException)
        } else {
            Ok(())
        }
    }
}

/// A local reference deleted when it goes out of scope, for objects created in loops.
pub struct Local {
    env: Env,
    pub obj: jobject,
}

impl Local {
    pub fn new(env: Env, obj: jobject) -> Self {
        Local { env, obj }
    }
}

impl Drop for Local {
    fn drop(&mut self) {
        self.env.delete_local(self.obj);
    }
}

/// A global reference that can be kept across calls and threads.
pub struct Global(pub jobject);

unsafe impl Send for Global {}
unsafe impl Sync for Global {}

#[derive(Clone, Copy)]
pub struct Vm(pub RawVm);

unsafe impl Send for Vm {}
unsafe impl Sync for Vm {}

impl Vm {
    /// The calling thread's `JNIEnv`, if it is attached.
    pub fn env(&self) -> Option<Env> {
        let mut env: RawEnv = null_mut();
        let result = unsafe {
            let table = *self.0;
            let get_env: unsafe extern "system" fn(RawVm, *mut RawEnv, jint) -> jint = std::mem::transmute(*table.add(GET_ENV));
            get_env(self.0, &mut env, JNI_VERSION_1_6)
        };
        (result == 0 && !env.is_null()).then_some(Env { raw: env })
    }

    /// Runs `f` on this (background) thread attached to the VM, detaching afterwards.
    pub fn with_attached<R>(&self, f: impl FnOnce(Env) -> R) -> Option<R> {
        if let Some(env) = self.env() {
            return Some(f(env));
        }
        unsafe {
            let table = *self.0;
            let attach: unsafe extern "system" fn(RawVm, *mut RawEnv, *mut c_void) -> jint =
                std::mem::transmute(*table.add(ATTACH_CURRENT_THREAD));
            let detach: unsafe extern "system" fn(RawVm) -> jint = std::mem::transmute(*table.add(DETACH_CURRENT_THREAD));
            let mut env: RawEnv = null_mut();
            if attach(self.0, &mut env, null_mut()) != 0 || env.is_null() {
                return None;
            }
            let result = f(Env { raw: env });
            detach(self.0);
            Some(result)
        }
    }
}
