//! Min Keyboard for the Linux desktop. Work in progress: the input logic is done and tested;
//! the X11 (XIM) and Wayland (input-method-v2) servers that use it come next.
#![allow(dead_code)]

mod core;
mod keysym;
mod session;

fn main() {
    let config = core::Config::load();
    let mut core = core::Core::new(config, core::assets_dir());
    let language = core.language;
    core.dictionaries.preload(language);
    eprintln!("min-keyboard-ime: the X11 and Wayland input method servers are not finished yet");
    std::process::exit(1);
}
