//! Builds the Min Keyboard APK with no Java toolchain:
//!
//! 1. compiles `libminkeyboard.so` for each ABI with cargo, linked by `rust-lld` against the NDK sysroot;
//! 2. writes `classes.dex` (see [`dex`]);
//! 3. compiles resources and the manifest with `aapt2` (a native SDK tool) and adds the dictionaries;
//! 4. aligns the archive and signs it with APK Signature Scheme v2 (see [`sign`]).
//!
//! Run `cargo run -p apkbuild -- --help` for options.

mod dex;
mod hash;
mod sign;
mod zip;

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};

use sign::{KeySource, Signer};

const MIN_SDK: u32 = 26;
const TARGET_SDK: u32 = 34;
const VERSION_CODE: u32 = 8;
const LIBRARY: &str = "minkeyboard";
/// Android 15 devices can use 16 KiB pages; libraries must be aligned for them.
const PAGE_SIZE: usize = 16384;

struct Abi {
    name: &'static str,
    rust_target: &'static str,
    sysroot_dir: &'static str,
    clang_dir: &'static str,
}

const ABIS: [Abi; 3] = [
    Abi { name: "arm64-v8a", rust_target: "aarch64-linux-android", sysroot_dir: "aarch64-linux-android", clang_dir: "aarch64" },
    Abi { name: "armeabi-v7a", rust_target: "armv7-linux-androideabi", sysroot_dir: "arm-linux-androideabi", clang_dir: "arm" },
    Abi { name: "x86_64", rust_target: "x86_64-linux-android", sysroot_dir: "x86_64-linux-android", clang_dir: "x86_64" },
];

const USAGE: &str = "\
Usage: apkbuild [options]
       apkbuild dex <out.dex>

Options:
  --out <file>            output APK (default: target/apk/min-keyboard.apk)
  --abi <list>            comma-separated ABIs (default: arm64-v8a,armeabi-v7a,x86_64)
  --sdk <dir>             Android SDK (default: $ANDROID_HOME or $ANDROID_SDK_ROOT)
  --ndk <dir>             Android NDK; only its sysroot libraries are used
                          (default: $ANDROID_NDK_HOME, $ANDROID_NDK_ROOT, or the newest <sdk>/ndk/*)
  --keystore <file>       PKCS#12 signing keystore (default: ~/.android/debug.keystore, created if missing)
  --keystore-pass <pass>  keystore password (default: android)
  --key <pem> --cert <pem>  sign with a PEM private key and certificate instead
  --debug-rust            build the Rust library without optimisation
";

struct Options {
    out: Option<PathBuf>,
    abis: Vec<&'static Abi>,
    sdk: Option<PathBuf>,
    ndk: Option<PathBuf>,
    keystore: Option<PathBuf>,
    keystore_pass: String,
    key: Option<PathBuf>,
    cert: Option<PathBuf>,
    release_rust: bool,
}

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("apkbuild: {message}");
    exit(1)
}

fn parse_args(args: Vec<String>) -> Options {
    let mut options = Options {
        out: None,
        abis: ABIS.iter().collect(),
        sdk: None,
        ndk: None,
        keystore: None,
        keystore_pass: "android".into(),
        key: None,
        cert: None,
        release_rust: true,
    };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| fail(format!("{arg} needs a value")));
        match arg.as_str() {
            "--out" => options.out = Some(value().into()),
            "--sdk" => options.sdk = Some(value().into()),
            "--ndk" => options.ndk = Some(value().into()),
            "--keystore" => options.keystore = Some(value().into()),
            "--keystore-pass" => options.keystore_pass = value(),
            "--key" => options.key = Some(value().into()),
            "--cert" => options.cert = Some(value().into()),
            "--debug-rust" => options.release_rust = false,
            "--abi" => {
                options.abis = value()
                    .split(',')
                    .map(|name| ABIS.iter().find(|abi| abi.name == name).unwrap_or_else(|| fail(format!("unknown ABI {name}"))))
                    .collect()
            }
            "-h" | "--help" => {
                print!("{USAGE}");
                exit(0)
            }
            other => fail(format!("unknown option {other}\n\n{USAGE}")),
        }
    }
    options
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap_or_else(|e| fail(e))
}

fn env_path(names: &[&str]) -> Option<PathBuf> {
    names.iter().find_map(env::var_os).map(PathBuf::from)
}

/// The entry of `dir` with the highest version-like name that satisfies `accept`.
fn newest(dir: &Path, accept: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(dir).ok()?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| accept(p)).collect();
    let version = |p: &PathBuf| -> Vec<u64> {
        p.file_name().unwrap_or_default().to_string_lossy().split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse().ok()).collect()
    };
    found.sort_by_key(version);
    found.pop()
}

fn run(command: &mut Command) {
    let status = command.status().unwrap_or_else(|e| fail(format!("cannot run {:?}: {e}", command.get_program())));
    if !status.success() {
        fail(format!("{:?} failed with {status}", command.get_program()));
    }
}

fn host_tag() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin-x86_64"
    } else {
        "linux-x86_64"
    }
}

/// Builds `libminkeyboard.so` for one ABI and returns its path.
fn build_library(root: &Path, ndk: &Path, abi: &Abi, release: bool) -> PathBuf {
    let prebuilt = ndk.join("toolchains/llvm/prebuilt").join(host_tag());
    let libs = prebuilt.join("sysroot/usr/lib").join(abi.sysroot_dir).join(MIN_SDK.to_string());
    let clang =
        newest(&prebuilt.join("lib/clang"), |p| p.is_dir()).unwrap_or_else(|| fail(format!("no lib/clang in {}", prebuilt.display())));
    let unwind = clang.join("lib/linux").join(abi.clang_dir);
    for needed in [libs.join("libc.so"), libs.join("crtbegin_so.o"), unwind.join("libunwind.a")] {
        if !needed.exists() {
            fail(format!("{} is missing; is --ndk an Android NDK?", needed.display()));
        }
    }
    let link_args = [
        format!("-L{}", libs.display()),
        format!("-L{}", unwind.display()),
        libs.join("crtbegin_so.o").display().to_string(),
        libs.join("crtend_so.o").display().to_string(),
        format!("-zmax-page-size={PAGE_SIZE}"),
        format!("-soname=lib{LIBRARY}.so"),
        "--hash-style=both".into(),
        "--build-id=sha1".into(),
        "-znoexecstack".into(),
    ];
    let mut rustflags = String::from("-C linker-flavor=ld.lld");
    for arg in &link_args {
        rustflags.push_str(" -C link-arg=");
        rustflags.push_str(arg);
    }
    let triple_env = abi.rust_target.to_uppercase().replace('-', "_");
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .current_dir(root)
        .args(["build", "-p", "min-keyboard-android", "--lib", "--target", abi.rust_target])
        .env(format!("CARGO_TARGET_{triple_env}_LINKER"), "rust-lld")
        .env(format!("CARGO_TARGET_{triple_env}_RUSTFLAGS"), rustflags);
    if release {
        command.arg("--release");
    }
    println!("building lib{LIBRARY}.so for {}", abi.name);
    run(&mut command);
    let target_dir = env_path(&["CARGO_TARGET_DIR"]).unwrap_or_else(|| root.join("target"));
    target_dir.join(abi.rust_target).join(if release { "release" } else { "debug" }).join(format!("lib{LIBRARY}.so"))
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("dex") {
        let out = args.get(1).unwrap_or_else(|| fail("dex needs an output path"));
        fs::write(out, dex::write(&dex::min_keyboard_classes())).unwrap_or_else(|e| fail(e));
        return;
    }
    let options = parse_args(args);
    if let Err(e) = build(&options) {
        fail(e);
    }
}

fn build(options: &Options) -> io::Result<()> {
    let root = repo_root();
    let sdk = options
        .sdk
        .clone()
        .or_else(|| env_path(&["ANDROID_HOME", "ANDROID_SDK_ROOT"]))
        .unwrap_or_else(|| fail("set ANDROID_HOME or pass --sdk"));
    let build_tools = newest(&sdk.join("build-tools"), |p| p.join("aapt2").exists())
        .unwrap_or_else(|| fail(format!("no build-tools with aapt2 in {}", sdk.display())));
    let platform = sdk.join("platforms").join(format!("android-{TARGET_SDK}"));
    let android_jar = if platform.join("android.jar").exists() {
        platform.join("android.jar")
    } else {
        newest(&sdk.join("platforms"), |p| p.join("android.jar").exists())
            .unwrap_or_else(|| fail(format!("no platforms/*/android.jar in {}", sdk.display())))
            .join("android.jar")
    };
    let ndk = options
        .ndk
        .clone()
        .or_else(|| env_path(&["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"]))
        .or_else(|| newest(&sdk.join("ndk"), |p| p.join("toolchains").is_dir()))
        .unwrap_or_else(|| fail("set ANDROID_NDK_HOME or pass --ndk"));

    let work = root.join("target/apk");
    fs::create_dir_all(&work)?;
    let libraries: Vec<(&Abi, PathBuf)> =
        options.abis.iter().map(|abi| (*abi, build_library(&root, &ndk, abi, options.release_rust))).collect();

    println!("compiling resources with {}", build_tools.join("aapt2").display());
    let aapt2 = build_tools.join("aapt2");
    let compiled = work.join("resources.zip");
    run(Command::new(&aapt2).arg("compile").arg("--dir").arg(root.join("android/res")).arg("-o").arg(&compiled));
    let base = work.join("base.apk");
    run(Command::new(&aapt2)
        .arg("link")
        .arg("-o")
        .arg(&base)
        .arg("-I")
        .arg(&android_jar)
        .arg("--manifest")
        .arg(root.join("android/AndroidManifest.xml"))
        .arg("-A")
        .arg(root.join("assets"))
        .args(["--min-sdk-version", &MIN_SDK.to_string(), "--target-sdk-version", &TARGET_SDK.to_string()])
        .args(["--version-code", &VERSION_CODE.to_string(), "--version-name", env!("CARGO_PKG_VERSION")])
        .arg(&compiled));

    let mut entries = zip::read(&fs::read(&base)?)?;
    entries.retain(|e| e.name != "classes.dex" && !e.name.starts_with("lib/"));
    entries.push(zip::Entry::stored("classes.dex", dex::write(&dex::min_keyboard_classes())));
    for (abi, library) in &libraries {
        entries.push(zip::Entry::stored(&format!("lib/{}/lib{LIBRARY}.so", abi.name), fs::read(library)?));
    }
    let (unsigned, layout) = zip::write(&entries, PAGE_SIZE);

    let source = match (&options.key, &options.cert, &options.keystore) {
        (Some(key), Some(cert), _) => KeySource::Pem { key: key.clone(), cert: cert.clone() },
        (Some(_), None, _) | (None, Some(_), _) => fail("--key and --cert go together"),
        (None, None, Some(path)) => KeySource::Pkcs12 { path: path.clone(), password: options.keystore_pass.clone() },
        (None, None, None) => {
            let home = env_path(&["HOME"]).unwrap_or_else(|| fail("HOME is not set"));
            let path = home.join(".android/debug.keystore");
            if !path.exists() {
                println!("creating debug keystore {}", path.display());
                Signer::create_debug_keystore(&path, &options.keystore_pass)?;
            }
            KeySource::Pkcs12 { path, password: options.keystore_pass.clone() }
        }
    };
    let signed = sign::sign_v2(&unsigned, &layout, &Signer::new(&source)?)?;
    let out = options.out.clone().unwrap_or_else(|| work.join("min-keyboard.apk"));
    fs::write(&out, &signed)?;
    println!("wrote {} ({} KiB, sha256 {})", out.display(), signed.len() / 1024, hash::hex(&hash::sha256(&signed)));
    Ok(())
}
