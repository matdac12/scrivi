//! scrivi: a minimal text editor for Windows that replaces Notepad.

mod app;
mod chrome;
mod editor;
mod launch;
mod mdstyle;
mod prefs;
mod proto;
mod rendered;
mod shell;
mod spell;
mod tabs;
mod textfile;
mod watch;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(launch::run(&args));
}
