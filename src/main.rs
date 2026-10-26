//! scrivi: a minimal text editor for Windows that replaces Notepad.
#![cfg_attr(not(test), windows_subsystem = "windows")]

mod app;
mod chrome;
mod editor;
mod find;
mod launch;
mod mdstyle;
mod mdview;
mod prefs;
mod proto;
mod rendered;
mod shell;
mod spell;
mod spellview;
mod tabs;
mod textfile;
mod watch;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(launch::run(&args));
}
