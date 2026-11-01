use std::process::Command;

fn run_version_flag(flag: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_hn"))
        .arg(flag)
        .output()
        .expect("failed to run hn binary");
    assert!(output.status.success(), "hn {flag} should exit 0");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("hn {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn long_version_flag() {
    run_version_flag("--version");
}

#[test]
fn short_version_flag() {
    run_version_flag("-V");
}
