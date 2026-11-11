//! `--output PATH` writes the report to a file. An existing file is replaced.
//! A missing parent directory is exit 2 and is not created. Exit codes stay 0, 1, 2 and 3.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn run(args: &[&str]) -> (Option<i32>, Vec<u8>, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("binary start");
    (
        output.status.code(),
        output.stdout,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn sample() -> String {
    format!(
        "{}/tests/fixtures/quickstart.dogstatsd",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn scratch() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "dd-metric-bill-output-{}-{}-{}",
        std::process::id(),
        nanos,
        N.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).expect("create scratch dir");
    dir
}

#[test]
fn output_flag_writes_a_new_file() {
    for args in [&["--help"][..], &["count", "--help"], &["diff", "--help"]] {
        let (code, stdout, stderr) = run(args);
        assert_eq!(code, Some(0), "{args:?} {stderr}");
        let help = String::from_utf8_lossy(&stdout);
        assert!(help.contains("--output"), "{args:?}\n{help}");
    }

    let root = scratch();
    let file = sample();
    let count_report = root.join("count.txt");
    assert!(!count_report.exists());
    let (code, stdout, stderr) = run(&["count", "--output", count_report.to_str().unwrap(), &file]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.is_empty(), "stdout must stay empty");
    assert!(stderr.is_empty(), "{stderr}");
    let (plain_code, expected, plain_err) = run(&["count", &file]);
    assert_eq!(plain_code, Some(0), "{plain_err}");
    assert!(!expected.is_empty());
    assert_eq!(fs::read(&count_report).unwrap(), expected);

    let diff_report = root.join("diff.txt");
    let (code, stdout, stderr) = run(&[
        "diff",
        "--output",
        diff_report.to_str().unwrap(),
        &file,
        &file,
    ]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.is_empty());
    assert!(stderr.is_empty(), "{stderr}");
    let (plain_code, expected, plain_err) = run(&["diff", &file, &file]);
    assert_eq!(plain_code, Some(0), "{plain_err}");
    assert_eq!(fs::read(&diff_report).unwrap(), expected);
    fs::remove_dir_all(&root).ok();
}

#[test]
fn output_flag_overwrites_an_existing_file() {
    let root = scratch();
    let report = root.join("report.txt");
    fs::write(&report, b"stale report that must be replaced\n").unwrap();
    let file = sample();
    let (code, stdout, stderr) = run(&["count", "--output", report.to_str().unwrap(), &file]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.is_empty());
    assert!(stderr.is_empty(), "{stderr}");
    let (plain_code, expected, plain_err) = run(&["count", &file]);
    assert_eq!(plain_code, Some(0), "{plain_err}");
    let body = fs::read(&report).unwrap();
    assert_eq!(body, expected);
    assert!(!String::from_utf8_lossy(&body).contains("stale"));
    fs::remove_dir_all(&root).ok();
}

#[test]
fn output_flag_missing_directory_exits_two() {
    let root = scratch();
    let missing_dir = root.join("no-such-dir");
    let report = missing_dir.join("report.txt");
    let file = sample();
    let (code, stdout, stderr) = run(&["count", "--output", report.to_str().unwrap(), &file]);
    assert_eq!(code, Some(2), "{stderr}");
    assert!(stdout.is_empty());
    assert!(stderr.contains("could not write the report"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(!report.exists());
    assert!(
        !missing_dir.exists(),
        "a missing parent directory must not be created"
    );
    fs::remove_dir_all(&root).ok();
}
