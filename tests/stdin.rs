//! The sample path `-` reads standard input instead of a file. For the same
//! bytes, the output and exit code must be identical to passing a real file
//! path, for `count` and `diff`. A FIFO (an unrelated non-regular-file path)
//! is still refused; stdin is not.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
}

fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

struct Run {
    code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run(args: &[&std::ffi::OsStr]) -> Run {
    let output = binary().args(args).output().expect("binary start");
    Run {
        code: output.status.code().unwrap_or(101),
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

/// Runs `args`, piping `input` on stdin, where one of `args` is `-`.
fn run_stdin(args: &[&std::ffi::OsStr], input: &[u8]) -> Run {
    let mut child = binary()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary start");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait");
    Run {
        code: output.status.code().unwrap_or(101),
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

fn assert_same(file: &Run, stdin: &Run, label: &str) {
    assert_eq!(file.code, stdin.code, "{label}: exit code differs");
    assert_eq!(file.stdout, stdin.stdout, "{label}: stdout differs");
    assert_eq!(file.stderr, stdin.stderr, "{label}: stderr differs");
}

#[test]
fn count_dogstatsd_from_stdin_matches_file() {
    let data = b"a:1|c\nb:1|c\n";
    let file = tmp("stdin-count.dogstatsd", data);
    let via_file = run(&["count".as_ref(), file.as_os_str()]);
    let via_stdin = run_stdin(&["count".as_ref(), "-".as_ref()], data);
    assert!(String::from_utf8_lossy(&via_stdin.stdout).contains("total: 2"));
    assert_same(&via_file, &via_stdin, "count dogstatsd");
}

#[test]
fn count_jsonl_from_stdin_matches_file() {
    let data = b"{\"metric\":\"a\",\"type\":\"count\",\"tags\":[]}\n";
    let file = tmp("stdin-count.jsonl", data);
    let via_file = run(&[
        "count".as_ref(),
        "--format".as_ref(),
        "jsonl".as_ref(),
        file.as_os_str(),
    ]);
    let via_stdin = run_stdin(
        &[
            "count".as_ref(),
            "--format".as_ref(),
            "jsonl".as_ref(),
            "-".as_ref(),
        ],
        data,
    );
    assert_same(&via_file, &via_stdin, "count jsonl");
}

#[test]
fn count_with_prices_from_stdin_matches_file() {
    let prices = Path::new(env!("CARGO_MANIFEST_DIR")).join("prices/datadog-example.json");
    let data = b"a:1|c\nb:1|c\n";
    let file = tmp("stdin-count-priced.dogstatsd", data);
    let via_file = run(&[
        "count".as_ref(),
        "--prices".as_ref(),
        prices.as_os_str(),
        file.as_os_str(),
    ]);
    let via_stdin = run_stdin(
        &[
            "count".as_ref(),
            "--prices".as_ref(),
            prices.as_os_str(),
            "-".as_ref(),
        ],
        data,
    );
    assert!(String::from_utf8_lossy(&via_stdin.stdout).contains("USD"));
    assert_same(&via_file, &via_stdin, "count with prices");
}

#[test]
fn diff_old_from_stdin_matches_file() {
    let old_data = b"a:1|c\n";
    let new_data = b"a:1|c\nb:1|c\n";
    let old = tmp("stdin-diff-old.dogstatsd", old_data);
    let new = tmp("stdin-diff-new.dogstatsd", new_data);
    let via_file = run(&["diff".as_ref(), old.as_os_str(), new.as_os_str()]);
    let via_stdin = run_stdin(&["diff".as_ref(), "-".as_ref(), new.as_os_str()], old_data);
    assert_same(&via_file, &via_stdin, "diff old from stdin");
}

#[test]
fn diff_new_from_stdin_matches_file() {
    let old_data = b"a:1|c\n";
    let new_data = b"a:1|c\nb:1|c\n";
    let old = tmp("stdin-diff-old-2.dogstatsd", old_data);
    let new = tmp("stdin-diff-new-2.dogstatsd", new_data);
    let via_file = run(&["diff".as_ref(), old.as_os_str(), new.as_os_str()]);
    let via_stdin = run_stdin(&["diff".as_ref(), old.as_os_str(), "-".as_ref()], new_data);
    assert_same(&via_file, &via_stdin, "diff new from stdin");
}

#[test]
fn invalid_line_from_stdin_matches_file_exit_2() {
    let data = b"page.views:1|c\nbeta.secret:1|nope|#token:canary\n";
    let file = tmp("stdin-invalid.dogstatsd", data);
    let via_file = run(&["count".as_ref(), file.as_os_str()]);
    let via_stdin = run_stdin(&["count".as_ref(), "-".as_ref()], data);
    assert_same(&via_file, &via_stdin, "invalid line from stdin");
    assert_eq!(via_stdin.code, 2);
    assert!(via_stdin.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&via_stdin.stderr);
    assert!(stderr.contains("line 2"), "{stderr}");
    assert!(!stderr.contains("canary"), "{stderr}");
}

#[test]
fn empty_stdin_matches_empty_file() {
    let file = tmp("stdin-empty.dogstatsd", b"");
    let via_file = run(&["count".as_ref(), file.as_os_str()]);
    let via_stdin = run_stdin(&["count".as_ref(), "-".as_ref()], b"");
    assert_same(&via_file, &via_stdin, "empty input");
    assert!(
        String::from_utf8_lossy(&via_stdin.stdout).starts_with("total: 0"),
        "{}",
        String::from_utf8_lossy(&via_stdin.stdout)
    );
}

/// `--prices -` is out of scope: only the sample path reads `-`. A FIFO is
/// unaffected by the stdin feature and keeps the existing rejection.
#[test]
fn fifo_sample_is_still_rejected_unlike_stdin() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("stdin-fifo-dir");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let fifo = dir.join("fifo");
    let status = Command::new("mkfifo").arg(&fifo).status().expect("mkfifo");
    assert!(status.success());
    let output = binary()
        .arg("count")
        .arg(&fifo)
        .stdin(Stdio::null())
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    assert!(stderr.contains("not a regular file"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}
