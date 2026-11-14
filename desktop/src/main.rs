//! Min Keyboard for the Linux desktop. Work in progress.
#![allow(dead_code)]

mod core;
mod font;
mod keysym;
mod popup;
mod session;

fn main() {
    let config = core::Config::load();
    let mut core = core::Core::new(config, core::assets_dir());
    let language = core.language;
    core.dictionaries.preload(language);
    eprintln!("min-keyboard-ime: the X11 and Wayland input method servers are not finished yet");
    std::process::exit(1);
}
