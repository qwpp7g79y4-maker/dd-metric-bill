//! `--quiet` and `--verbose`. A pipe prints no hint and no context line.
//! `--verbose` adds that context. Exit codes stay 0, 1, 2 and 3.

use std::process::Command;

fn run(args: &[&str]) -> (Option<i32>, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("binary start");
    (
        output.status.code(),
        String::from_utf8(output.stdout).expect("stdout is utf-8"),
        String::from_utf8(output.stderr).expect("stderr is utf-8"),
    )
}

fn sample() -> String {
    format!(
        "{}/tests/fixtures/quickstart.dogstatsd",
        env!("CARGO_MANIFEST_DIR")
    )
}

#[test]
fn help_lists_quiet_and_verbose() {
    for args in [&["--help"][..], &["count", "--help"], &["diff", "--help"]] {
        let (code, stdout, stderr) = run(args);
        assert_eq!(code, Some(0), "{args:?} {stderr}");
        assert!(stdout.contains("--quiet"), "{args:?}\n{stdout}");
        assert!(stdout.contains("--verbose"), "{args:?}\n{stdout}");
    }
}

#[test]
fn pipe_and_quiet_omit_the_hint_verbose_prints_it() {
    let missing = "/nonexistent/ddmb-verbosity.dogstatsd";
    let (code, stdout, stderr) = run(&["count", missing]);
    assert_eq!(code, Some(2), "{stderr}");
    assert!(stdout.is_empty(), "{stdout}");
    assert_eq!(stderr.lines().count(), 1, "{stderr:?}");
    assert!(stderr.contains("cannot read sample"), "{stderr}");
    assert!(!stderr.contains("hint:"), "{stderr}");

    let (qcode, qout, qerr) = run(&["--quiet", "count", missing]);
    assert_eq!(qcode, Some(2), "{qerr}");
    assert!(qout.is_empty());
    assert_eq!(qerr, stderr, "quiet matches a pipe");

    let (vcode, vout, verr) = run(&["--verbose", "count", missing]);
    assert_eq!(vcode, Some(2), "{verr}");
    assert!(vout.is_empty());
    assert_eq!(verr.lines().count(), 2, "{verr:?}");
    assert!(verr.lines().nth(1).unwrap().starts_with("hint: "), "{verr}");
}

#[test]
fn quiet_with_verbose_is_exit_3() {
    let file = sample();
    let (code, stdout, stderr) = run(&["count", "--quiet", "--verbose", &file]);
    assert_eq!(code, Some(3), "{stderr}");
    assert!(stdout.is_empty(), "{stdout}");
    assert!(stderr.contains("error:"), "{stderr}");
    assert_ne!(code, Some(1), "no new threshold exit");
}

#[test]
fn verbose_context_stays_off_stdout() {
    let file = sample();
    let (code, stdout, stderr) = run(&["count", &file]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
    let (vcode, vout, verr) = run(&["count", "--verbose", &file]);
    assert_eq!(vcode, Some(0), "{verr}");
    assert_eq!(vout, stdout, "verbose does not change the report");
    assert!(verr.starts_with("context: samples "), "{verr:?}");
    assert!(verr.contains("custom metrics "), "{verr:?}");
    assert_eq!(verr.lines().count(), 1, "{verr:?}");
    let (qcode, qout, qerr) = run(&["count", "--quiet", &file]);
    assert_eq!(qcode, Some(0), "{qerr}");
    assert_eq!(qout, stdout);
    assert!(qerr.is_empty(), "{qerr}");
}

#[test]
fn diff_verbose_names_both_sides() {
    let old = format!(
        "{}/tests/fixtures/diff-old.dogstatsd",
        env!("CARGO_MANIFEST_DIR")
    );
    let new = format!(
        "{}/tests/fixtures/diff-new.dogstatsd",
        env!("CARGO_MANIFEST_DIR")
    );
    let (code, stdout, stderr) = run(&["diff", &old, &new]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
    let (vcode, vout, verr) = run(&["diff", "--verbose", &old, &new]);
    assert_eq!(vcode, Some(0), "{verr}");
    assert_eq!(vout, stdout);
    assert!(verr.contains("->"), "{verr:?}");
    assert_eq!(verr.lines().count(), 1, "{verr:?}");
}

#[test]
fn budget_exit_stays_1_and_the_pipe_is_one_line() {
    let file = sample();
    let (code, stdout, stderr) = run(&["count", "--budget", "0", &file]);
    assert_eq!(code, Some(1), "{stderr}");
    assert!(!stdout.is_empty(), "{stdout}");
    assert_eq!(stderr.lines().count(), 1, "{stderr:?}");
    assert!(stderr.starts_with("budget exceeded:"), "{stderr}");
    assert!(!stderr.contains("hint:"), "{stderr}");

    let (qcode, qout, qerr) = run(&["--quiet", "count", "--budget", "0", &file]);
    assert_eq!(qcode, Some(1), "{qerr}");
    assert_eq!(qout, stdout);
    assert_eq!(qerr, stderr);

    let (vcode, vout, verr) = run(&["--verbose", "count", "--budget", "0", &file]);
    assert_eq!(vcode, Some(1), "{verr}");
    assert_eq!(vout, stdout, "budget still prints the report");
    assert_eq!(verr.lines().count(), 2, "{verr:?}");
    assert!(verr.lines().next().unwrap().starts_with("budget exceeded:"));
    assert!(verr.lines().nth(1).unwrap().starts_with("hint: "));
}

#[test]
fn diff_rejection_stays_one_line_without_a_new_exit() {
    let old = format!(
        "{}/tests/fixtures/diff-old.dogstatsd",
        env!("CARGO_MANIFEST_DIR")
    );
    let new = format!(
        "{}/tests/fixtures/diff-new.dogstatsd",
        env!("CARGO_MANIFEST_DIR")
    );
    let (code, stdout, stderr) = run(&["diff", "--explain", &old, &new]);
    assert_eq!(code, Some(3), "{stderr}");
    assert!(stdout.is_empty());
    assert_eq!(stderr, "option does not apply to diff: --explain\n");
    let (vcode, vout, verr) = run(&["diff", "--verbose", "--explain", &old, &new]);
    assert_eq!(vcode, Some(3), "{verr}");
    assert!(vout.is_empty());
    assert_eq!(
        verr, stderr,
        "verbose does not renumber or extend this line"
    );
}
