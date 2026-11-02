//! Android NDK functions used directly: logging and reading the APK's assets.

use std::ffi::{c_char, c_int, c_void, CString};
use std::io;

use crate::jni::{jobject, Env, Global, RawEnv};

#[cfg(target_os = "android")]
mod ndk {
    use super::*;

    #[link(name = "log")]
    extern "C" {
        pub fn __android_log_write(priority: c_int, tag: *const c_char, text: *const c_char) -> c_int;
    }

    #[link(name = "android")]
    extern "C" {
        pub fn AAssetManager_fromJava(env: RawEnv, asset_manager: jobject) -> *mut c_void;
        pub fn AAssetManager_open(manager: *mut c_void, name: *const c_char, mode: c_int) -> *mut c_void;
        pub fn AAsset_getLength64(asset: *mut c_void) -> i64;
        pub fn AAsset_read(asset: *mut c_void, buffer: *mut c_void, count: usize) -> c_int;
        pub fn AAsset_close(asset: *mut c_void);
    }
}

/// Desktop builds (tests, `cargo check`) have no NDK; logging goes to stderr and there are no assets.
#[cfg(not(target_os = "android"))]
#[allow(non_snake_case, clippy::missing_safety_doc)]
mod ndk {
    use super::*;

    pub unsafe fn __android_log_write(_: c_int, _: *const c_char, text: *const c_char) -> c_int {
        eprintln!("{}", std::ffi::CStr::from_ptr(text).to_string_lossy());
        0
    }
    pub unsafe fn AAssetManager_fromJava(_: RawEnv, _: jobject) -> *mut c_void {
        std::ptr::null_mut()
    }
    pub unsafe fn AAssetManager_open(_: *mut c_void, _: *const c_char, _: c_int) -> *mut c_void {
        std::ptr::null_mut()
    }
    pub unsafe fn AAsset_getLength64(_: *mut c_void) -> i64 {
        0
    }
    pub unsafe fn AAsset_read(_: *mut c_void, _: *mut c_void, _: usize) -> c_int {
        0
    }
    pub unsafe fn AAsset_close(_: *mut c_void) {}
}

const ANDROID_LOG_ERROR: c_int = 6;

pub fn log_error(message: &str) {
    let tag = c"MinKeyboard";
    let text = CString::new(message.replace('\0', " ")).unwrap_or_default();
    unsafe {
        ndk::__android_log_write(ANDROID_LOG_ERROR, tag.as_ptr(), text.as_ptr());
    }
}

/// The APK's `assets/` folder, through the NDK asset manager (which is thread safe).
pub struct ApkAssets {
    manager: *mut c_void,
    /// Keeps the Java `AssetManager` alive while the native one is used.
    _java: Global,
}

unsafe impl Send for ApkAssets {}
unsafe impl Sync for ApkAssets {}

impl ApkAssets {
    /// `asset_manager` is a `android.content.res.AssetManager`.
    pub fn new(env: Env, asset_manager: jobject) -> Option<Self> {
        let java = Global(env.new_global(asset_manager));
        let manager = unsafe { ndk::AAssetManager_fromJava(env.raw(), java.0) };
        if manager.is_null() {
            env.delete_global(java.0);
            return None;
        }
        Some(ApkAssets { manager, _java: java })
    }
}

impl engines::Assets for ApkAssets {
    fn read(&self, name: &str) -> io::Result<Vec<u8>> {
        const AASSET_MODE_STREAMING: c_int = 2;
        let path = CString::new(name).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        unsafe {
            let asset = ndk::AAssetManager_open(self.manager, path.as_ptr(), AASSET_MODE_STREAMING);
            if asset.is_null() {
                return Err(io::Error::new(io::ErrorKind::NotFound, format!("no asset {name}")));
            }
            let mut data = Vec::with_capacity(ndk::AAsset_getLength64(asset).max(0) as usize);
            let mut buffer = vec![0u8; 1 << 16];
            loop {
                let count = ndk::AAsset_read(asset, buffer.as_mut_ptr().cast(), buffer.len());
                if count <= 0 {
                    ndk::AAsset_close(asset);
                    if count < 0 {
                        return Err(io::Error::other(format!("cannot read asset {name}")));
                    }
                    return Ok(data);
                }
                data.extend_from_slice(&buffer[..count as usize]);
            }
        }
    }
}
