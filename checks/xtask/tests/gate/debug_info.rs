//! A test build carries no debug information. `.cargo/config.toml` sets it for every build of the
//! repository, so it holds locally (where `~/.cargo/config.toml` may ask for line tables) and in
//! CI alike; this test binary is one such build.

#[cfg(target_os = "linux")]
#[test]
fn a_test_binary_carries_no_debug_sections() {
    use std::process::Command;

    let binary = std::env::current_exe().expect("the test binary's path");
    let listing = Command::new("readelf")
        .args(["-S", "--wide"])
        .arg(&binary)
        .output()
        .expect("readelf runs");
    assert!(
        listing.status.success(),
        "readelf exits {:?}",
        listing.status
    );
    let sections = String::from_utf8_lossy(&listing.stdout);
    let debug: Vec<&str> = sections
        .lines()
        .filter_map(|line| {
            line.split_whitespace()
                .find(|word| word.starts_with(".debug_"))
        })
        .collect();
    assert!(
        sections.contains(".text") && debug.is_empty(),
        "{} carries {debug:?}",
        binary.display()
    );
}
