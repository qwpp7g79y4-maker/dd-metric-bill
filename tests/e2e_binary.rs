//! Exit codes and a small synthetic count/diff, all through the built binary.
//!
//! The samples are written in the test. They are not a Datadog export.

use std::process::{Command, Output};

/// Parallel tests can read the same clock value (Windows has a coarse clock), so a counter keeps names unique.
static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
}

fn failure(output: &Output) -> String {
    format!(
        "status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is utf-8")
}

/// Usage failures name the problem, exit 3, and do not panic.
fn assert_usage(output: &Output, message: &str) {
    assert_eq!(output.status.code(), Some(3), "{}", failure(output));
    let stderr = stderr_of(output);
    assert!(
        stderr.contains(message),
        "stderr missing {message:?}:\n{stderr}"
    );
    assert!(
        !stderr.contains("panicked"),
        "stderr contains panicked:\n{stderr}"
    );
}

struct TempSample(std::path::PathBuf);

impl TempSample {
    fn new(contents: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let n = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("dd-metric-bill-test-e2e-{nanos}-{n}.txt"));
        std::fs::write(&path, contents).expect("write sample");
        Self(path)
    }

    fn as_str(&self) -> &str {
        self.0.to_str().expect("path is utf-8")
    }
}

impl Drop for TempSample {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn help_exits_0() {
    let output = binary().arg("--help").output().expect("binary start");
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("dd-metric-bill"), "{stdout}");
    assert!(!stderr_of(&output).contains("panicked"));
}

#[test]
fn unknown_option_exits_3_with_a_message() {
    let output = binary()
        .arg("--not-a-real-option")
        .output()
        .expect("binary start");
    assert_usage(&output, "unexpected argument");
}

#[test]
fn count_without_a_sample_exits_3_with_a_message() {
    let output = binary().arg("count").output().expect("binary start");
    assert_usage(&output, "<SAMPLE>");
}

#[test]
fn broken_dogstatsd_sample_exits_2() {
    let path = TempSample::new("page.views:1|nope\n");
    let output = binary()
        .args(["count", path.as_str()])
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(2), "{}", failure(&output));
    let stderr = stderr_of(&output);
    assert!(stderr.contains("invalid dogstatsd line 1"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(output.stdout.is_empty(), "{}", failure(&output));
}

/// One hand-written DogStatsD pair: two `env` values on the new side.
#[test]
fn count_and_diff_on_synthetic_dogstatsd() {
    let old = TempSample::new("page.views:1|c|#env:prod\n");
    let new = TempSample::new("page.views:1|c|#env:prod\npage.views:1|c|#env:dev\n");

    let counted = binary()
        .args(["count", new.as_str()])
        .output()
        .expect("binary start");
    assert_eq!(counted.status.code(), Some(0), "{}", failure(&counted));
    let count_text = String::from_utf8_lossy(&counted.stdout);
    assert!(count_text.contains("total: 2\n"), "{count_text}");
    assert!(
        count_text.contains("page.views type: count combinations: 2 count: 2"),
        "{count_text}"
    );
    assert!(counted.stderr.is_empty(), "{}", failure(&counted));

    let compared = binary()
        .args(["diff", old.as_str(), new.as_str()])
        .output()
        .expect("binary start");
    assert_eq!(compared.status.code(), Some(0), "{}", failure(&compared));
    let diff_text = String::from_utf8_lossy(&compared.stdout);
    assert!(diff_text.contains("old total: 1\n"), "{diff_text}");
    assert!(diff_text.contains("new total: 2\n"), "{diff_text}");
    assert!(diff_text.contains("delta: 1\n"), "{diff_text}");
    assert!(
        diff_text.contains("page.views type: count old: 1 new: 2 delta: 1"),
        "{diff_text}"
    );
    assert!(compared.stderr.is_empty(), "{}", failure(&compared));
}

#[test]
fn completions_print_a_script_for_bash_and_zsh() {
    for (shell, marker) in [
        ("bash", "complete -F _dd__metric__bill"),
        ("zsh", "#compdef dd-metric-bill"),
    ] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
            .args(["completions", shell])
            .output()
            .expect("run dd-metric-bill");
        assert_eq!(out.status.code(), Some(0), "{shell}");
        let text = String::from_utf8(out.stdout).expect("utf-8 script");
        assert!(text.contains(marker), "{shell}: {text}");
        assert!(text.contains("--budget"), "{shell}");
    }
    let bad = std::process::Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
        .args(["completions", "fish"])
        .output()
        .expect("run dd-metric-bill");
    assert_eq!(bad.status.code(), Some(3));
    assert!(bad.stdout.is_empty());
}
