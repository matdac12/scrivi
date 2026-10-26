//! Integration tests reach the real binary through `CARGO_BIN_EXE_scrivi`, as
//! tasca's do. The handoff tests (05) will start a resident the same way.

use std::process::Command;

const EXE: &str = env!("CARGO_BIN_EXE_scrivi");

#[test]
fn binary_runs_and_exits_cleanly() {
    let status = Command::new(EXE)
        .arg("notes.md")
        .status()
        .expect("start scrivi");
    assert!(status.success());
}
