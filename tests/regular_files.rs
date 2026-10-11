//! Only regular files. The binary does not open a directory, FIFO, or device.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static SERIAL: AtomicU64 = AtomicU64::new(0);

/// A hanging read of `/dev/zero` or a FIFO with no writer fails here,
/// and the child is killed so it does not keep running.
const DEADLINE: Duration = Duration::from_secs(1);

const PRICE_JSON: &str = concat!(
    r#"{"currency":"TST","source":"source-test","date":"2026-01-02","#,
    r#""ingested_per_100":2}"#,
);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let n = SERIAL.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("dd-metric-bill-test-file-{nanos}-{n}"));
        std::fs::create_dir(&path).expect("temporary directory");
        Self(path)
    }

    fn write_file(&self, name: &str, contents: &str) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, contents).expect("write file");
        path.to_str().expect("path is utf-8").to_string()
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct ChildGuard(Option<Child>);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(kind) = self.0.as_mut() {
            let _ = kind.kill();
            let _ = kind.wait();
        }
    }
}

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

fn output_within(args: &[&str], limit: Duration) -> Output {
    let mut kind = ChildGuard(Some(
        binary()
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("binary start"),
    ));
    let start = Instant::now();
    loop {
        let ready = match kind.0.as_mut().expect("process").try_wait() {
            Ok(status) => status.is_some(),
            Err(err) => panic!("waiting for the process: {err}"),
        };
        if ready {
            let process = kind.0.take().expect("process");
            return process.wait_with_output().expect("output");
        }
        if start.elapsed() >= limit {
            panic!("timeout after {limit:?}, the process is killed, args: {args:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn expected_rejection(path: &str, args: &[&str], kind: &str) {
    let output = output_within(args, DEADLINE);
    assert_eq!(output.status.code(), Some(2), "{}", failure(&output));
    assert!(output.stdout.is_empty(), "{}", failure(&output));
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    assert!(
        stderr.contains(&format!("{path}:")),
        "the message does not name the path: {path}\n{stderr}"
    );
    assert!(
        stderr.contains("not a regular file"),
        "the message lacks 'not a regular file':\n{stderr}"
    );
    assert!(stderr.contains(&format!("cannot read {kind}")), "{stderr}");
}

/// Count, diff (both positions), and `--prices` use the same read path.
fn four_times(error: &str, good: &str) {
    expected_rejection(error, &["count", error], "sample");
    expected_rejection(error, &["diff", error, good], "sample");
    expected_rejection(error, &["diff", good, error], "sample");
    expected_rejection(error, &["count", "--prices", error, good], "price file");
}

fn good_sample(map: &TempDir) -> String {
    map.write_file("sample.dogstatsd", "a:1|c\n")
}

#[test]
fn dev_zero_is_not_a_regular_file() {
    let map = TempDir::new();
    four_times("/dev/zero", &good_sample(&map));
}

#[test]
fn fifo_without_writer_is_not_a_regular_file() {
    let map = TempDir::new();
    let fifo = map.join("pipe");
    let status = Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo start");
    assert!(status.success(), "mkfifo failed: {status}");
    let path = fifo.to_str().expect("path is utf-8");
    four_times(path, &good_sample(&map));
}

#[test]
fn directory_is_not_a_regular_file() {
    let map = TempDir::new();
    let path = map.0.to_str().expect("path is utf-8").to_string();
    four_times(&path, &good_sample(&map));
}

#[test]
fn dev_null_is_not_a_regular_file() {
    let map = TempDir::new();
    let good = good_sample(&map);
    four_times("/dev/null", &good);

    // Symlink to a device: metadata follows the link, and the message names the path the user gave.
    let link = map.join("null-link");
    std::os::unix::fs::symlink("/dev/null", &link).expect("symlink");
    let path = link.to_str().expect("path is utf-8");
    expected_rejection(path, &["count", path], "sample");
}

#[test]
fn socket_is_not_a_regular_file() {
    let map = TempDir::new();
    let sock = map.join("sock");
    let _luister = std::os::unix::net::UnixListener::bind(&sock).expect("socket");
    let path = sock.to_str().expect("path is utf-8");
    four_times(path, &good_sample(&map));
}

fn output_ok(args: &[&str]) -> String {
    let output = output_within(args, DEADLINE);
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    assert!(output.stderr.is_empty(), "{}", failure(&output));
    String::from_utf8(output.stdout).expect("stdout is utf-8")
}

#[test]
fn regular_file_and_symlink_work() {
    let map = TempDir::new();
    let sample = map.write_file("sample.dogstatsd", "a:1|c\n");
    let sample_link = map.join("sample-link");
    std::os::unix::fs::symlink(Path::new(&sample), &sample_link).expect("symlink");
    let link = sample_link.to_str().expect("path is utf-8");

    let direct = output_ok(&["count", &sample]);
    let via = output_ok(&["count", link]);
    assert_eq!(direct, via, "symlink to a sample differs");
    assert!(
        direct.contains("a type: count combinations: 1 count: 1 rule: D3"),
        "{direct}"
    );

    let old = map.write_file("old.dogstatsd", "a:1|c\n");
    let new = map.write_file("new.dogstatsd", "a:1|c\nb:1|c\n");
    let old_link = map.join("old-link");
    let new_link = map.join("new-link");
    std::os::unix::fs::symlink(Path::new(&old), &old_link).expect("symlink");
    std::os::unix::fs::symlink(Path::new(&new), &new_link).expect("symlink");
    let diff_direct = output_ok(&["diff", &old, &new]);
    let diff_via = output_ok(&[
        "diff",
        old_link.to_str().expect("path is utf-8"),
        new_link.to_str().expect("path is utf-8"),
    ]);
    assert_eq!(diff_direct, diff_via, "symlink to a diff sample differs");
    assert!(diff_direct.contains("delta: 1\n"), "{diff_direct}");

    let price = map.write_file("price.json", PRICE_JSON);
    let price_link = map.join("price-link");
    std::os::unix::fs::symlink(Path::new(&price), &price_link).expect("symlink");
    let with = output_ok(&["count", "--prices", &price, &sample]);
    let with_link = output_ok(&[
        "count",
        "--prices",
        price_link.to_str().expect("path is utf-8"),
        &sample,
    ]);
    assert_eq!(with, with_link, "symlink to a price file differs");
    assert!(with.contains("source: source-test\n"), "{with}");
    assert!(with.contains("price: TST per 100\n"), "{with}");
}
