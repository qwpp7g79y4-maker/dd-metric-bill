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

#[cfg(unix)]
#[test]
fn output_flag_does_not_write_through_a_planted_symlink() {
    let root = scratch();
    let report = root.join("report.txt");
    let victim = root.join("victim.txt");
    fs::write(&victim, b"precious\n").unwrap();
    // The temporary name holds the process id. `sh` knows it as `$$` and
    // `exec` keeps it, so the link is planted on the exact name the run picks.
    let script = format!(
        "ln -s {victim} {root}/.report.txt.dd-metric-bill-$$.tmp; exec {bin} count --output {report} {sample}",
        victim = victim.display(),
        root = root.display(),
        bin = env!("CARGO_BIN_EXE_dd-metric-bill"),
        report = report.display(),
        sample = sample(),
    );
    let out = Command::new("sh")
        .args(["-c", &script])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("sh start");
    assert_eq!(
        out.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("could not write the report"));
    assert_eq!(fs::read(&victim).unwrap(), b"precious\n");
    assert!(!report.exists());
    fs::remove_dir_all(&root).ok();
}

#[cfg(unix)]
#[test]
fn output_flag_replaces_a_symlink_without_following_it() {
    use std::os::unix::fs::symlink;
    let root = scratch();
    let target = root.join("target.txt");
    let report = root.join("report.txt");
    fs::write(&target, b"untouched\n").unwrap();
    symlink(&target, &report).unwrap();
    let (code, _, stderr) = run(&["count", "--output", report.to_str().unwrap(), &sample()]);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(fs::read(&target).unwrap(), b"untouched\n");
    let meta = fs::symlink_metadata(&report).unwrap();
    assert!(meta.file_type().is_file(), "the link is replaced by a file");
    fs::remove_dir_all(&root).ok();
}

#[cfg(unix)]
#[test]
fn output_flag_does_not_replace_special_files() {
    use std::os::unix::fs::FileTypeExt;
    use std::os::unix::net::UnixListener;
    let root = scratch();

    let dir = root.join("a-directory");
    fs::create_dir(&dir).unwrap();
    let fifo = root.join("a-fifo");
    let made = Command::new("mkfifo").arg(&fifo).status().expect("mkfifo");
    assert!(made.success());
    // macOS limits a socket path to about 104 bytes and its temp dir is long,
    // so the socket lives in a short directory directly under /tmp.
    let sock_dir = PathBuf::from(format!("/tmp/ddmb-{}", std::process::id()));
    fs::create_dir_all(&sock_dir).unwrap();
    let sock = sock_dir.join("s");
    fs::remove_file(&sock).ok();
    let _listener = UnixListener::bind(&sock).unwrap();

    for path in [&dir, &fifo, &sock] {
        let (code, stdout, stderr) = run(&["count", "--output", path.to_str().unwrap(), &sample()]);
        assert_eq!(code, Some(2), "{path:?}: {stderr}");
        assert!(stdout.is_empty());
        assert!(stderr.contains("not a regular file"), "{stderr}");
        assert!(!stderr.contains("panicked"), "{stderr}");
    }
    assert!(fs::symlink_metadata(&dir).unwrap().is_dir());
    assert!(fs::symlink_metadata(&fifo).unwrap().file_type().is_fifo());
    assert!(fs::symlink_metadata(&sock).unwrap().file_type().is_socket());
    fs::remove_dir_all(&sock_dir).ok();
    fs::remove_dir_all(&root).ok();
}
