// Compiles Scintilla's core and Win32 platform layer into a static library.
// No Lexilla: the prototype styles markdown itself from pulldown-cmark events.
use std::path::PathBuf;

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let sci = dir.join("vendor").join("scintilla");

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .warnings(false)
        .flag("/EHsc")
        .flag("/utf-8")
        .define("NDEBUG", None)
        .define("UNICODE", None)
        .define("_UNICODE", None)
        .include(sci.join("include"))
        .include(sci.join("src"))
        .include(sci.join("win32"));
    for sub in ["src", "win32"] {
        for entry in std::fs::read_dir(sci.join(sub)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "cxx") {
                println!("cargo:rerun-if-changed={}", path.display());
                build.file(path);
            }
        }
    }
    build.compile("scintilla");

    for lib in ["user32", "gdi32", "imm32", "ole32", "oleaut32", "advapi32", "msimg32"] {
        println!("cargo:rustc-link-lib={lib}");
    }

    // Per-monitor v2 DPI awareness via the linker, as tasca does.
    println!("cargo:rerun-if-changed=scrivi-proto.manifest");
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
        dir.join("scrivi-proto.manifest").display()
    );
}
