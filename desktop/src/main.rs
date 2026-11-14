//! Min Keyboard for the Linux desktop: an input method that types English, Malay, Chinese,
//! Japanese and Korean into any application, without IBus or Fcitx. X applications reach it
//! through its own XIM server; Wayland applications through the input-method-v2 protocol.

mod core;
mod font;
mod keysym;
mod popup;
mod session;
mod x11;
mod xim;

use std::sync::{Arc, Mutex};

const USAGE: &str = "Usage: min-keyboard-ime [--x11-only | --wayland-only]

Runs the Min Keyboard input method for the X11 display ($DISPLAY) and the Wayland
compositor ($WAYLAND_DISPLAY), whichever are available.

X11 applications need:   XMODIFIERS=@im=minkeyboard GTK_IM_MODULE=xim
Wayland compositors need input-method-v2 (sway, Hyprland, river, labwc, Wayfire).

Ctrl+Space turns it on and off; Ctrl+F12 cycles the languages.
Settings: ~/.config/min-keyboard/config.ini";

fn main() {
    let mut x11_wanted = true;
    let mut wayland_wanted = true;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--x11-only" => wayland_wanted = false,
            "--wayland-only" => x11_wanted = false,
            "--version" => {
                println!("min-keyboard-ime {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            other => {
                eprintln!("min-keyboard-ime: unknown option {other}\n\n{USAGE}");
                std::process::exit(2);
            }
        }
    }

    let config = core::Config::load();
    let core = Arc::new(Mutex::new(core::Core::new(config, core::assets_dir())));
    {
        // Load the dictionaries in the background, one at a time, so keys are not slow later.
        let core = core.clone();
        std::thread::spawn(move || {
            let languages = core.lock().unwrap_or_else(|e| e.into_inner()).config.languages.clone();
            for language in languages {
                core.lock().unwrap_or_else(|e| e.into_inner()).dictionaries.preload(language);
            }
        });
    }

    let mut threads = Vec::new();
    if x11_wanted && std::env::var_os("DISPLAY").is_some() {
        let core = core.clone();
        threads.push(("X11", std::thread::spawn(move || xim::Server::start(core).and_then(xim::Server::run))));
    }
    if wayland_wanted && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        eprintln!("min-keyboard-ime: Wayland support is not finished yet; only X11 and XWayland applications are served");
    }
    if threads.is_empty() {
        eprintln!("min-keyboard-ime: no display to serve: set DISPLAY (X11) or WAYLAND_DISPLAY (Wayland)");
        std::process::exit(1);
    }
    let mut failures = 0;
    let total = threads.len();
    for (name, thread) in threads {
        match thread.join() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                eprintln!("min-keyboard-ime: {name}: {e}");
                failures += 1;
            }
            Err(_) => failures += 1,
        }
    }
    std::process::exit(if failures == total { 1 } else { 0 });
}
