//! Only regular files. The binary does not open a directory, FIFO, or device.
//! Device, FIFO, socket and symlink cases use Unix paths and APIs, so they run on Unix only.

#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static SERIAL: AtomicU64 = AtomicU64::new(0);

/// A hanging read of `/dev/zero` or a FIFO with no writer fails here,
/// and the child is killed so it does not keep running.
const DEADLINE: Duration = Duration::from_secs(1);

#[cfg(unix)]
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

    #[cfg(unix)]
    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
struct ShortDir(PathBuf);

#[cfg(unix)]
impl ShortDir {
    fn new() -> Self {
        let n = SERIAL.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!("/tmp/ddmb-{}-{n}", std::process::id()));
        std::fs::create_dir(&path).expect("short temporary directory");
        Self(path)
    }
}

#[cfg(unix)]
impl Drop for ShortDir {
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

fn good_sample(tmp: &TempDir) -> String {
    tmp.write_file("sample.dogstatsd", "a:1|c\n")
}

#[test]
#[cfg(unix)]
fn dev_zero_is_not_a_regular_file() {
    let tmp = TempDir::new();
    four_times("/dev/zero", &good_sample(&tmp));
}

#[test]
#[cfg(unix)]
fn fifo_without_writer_is_not_a_regular_file() {
    let tmp = TempDir::new();
    let fifo = tmp.join("pipe");
    let status = Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo start");
    assert!(status.success(), "mkfifo failed: {status}");
    let path = fifo.to_str().expect("path is utf-8");
    four_times(path, &good_sample(&tmp));
}

/// A symlink to a FIFO is the FIFO. The check follows the link and exits 2
/// without waiting for a writer.
#[test]
#[cfg(unix)]
fn symlink_to_fifo_is_not_a_regular_file() {
    let tmp = TempDir::new();
    let fifo = tmp.join("pipe");
    let status = Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo start");
    assert!(status.success(), "mkfifo failed: {status}");
    let link = tmp.join("link");
    std::os::unix::fs::symlink(&fifo, &link).expect("symlink");
    let path = link.to_str().expect("path is utf-8");
    four_times(path, &good_sample(&tmp));
}

#[test]
fn directory_is_not_a_regular_file() {
    let tmp = TempDir::new();
    let path = tmp.0.to_str().expect("path is utf-8").to_string();
    four_times(&path, &good_sample(&tmp));
}

#[test]
#[cfg(unix)]
fn dev_null_is_not_a_regular_file() {
    let tmp = TempDir::new();
    let good = good_sample(&tmp);
    four_times("/dev/null", &good);

    // Symlink to a device: the open follows the link, and the message names the path the user gave.
    let link = tmp.join("null-link");
    std::os::unix::fs::symlink("/dev/null", &link).expect("symlink");
    let path = link.to_str().expect("path is utf-8");
    expected_rejection(path, &["count", path], "sample");
}

#[test]
#[cfg(unix)]
fn socket_is_not_a_regular_file() {
    let tmp = TempDir::new();
    // A socket path may be at most about 100 bytes, so it goes in a short
    // directory under /tmp instead of a possibly long $TMPDIR.
    let short = ShortDir::new();
    let sock = short.0.join("sock");
    let _listener = std::os::unix::net::UnixListener::bind(&sock).expect("socket");
    let path = sock.to_str().expect("path is utf-8");
    four_times(path, &good_sample(&tmp));
}

#[cfg(unix)]
fn output_ok(args: &[&str]) -> String {
    let output = output_within(args, DEADLINE);
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    assert!(output.stderr.is_empty(), "{}", failure(&output));
    String::from_utf8(output.stdout).expect("stdout is utf-8")
}

#[test]
#[cfg(unix)]
fn regular_file_and_symlink_work() {
    let tmp = TempDir::new();
    let sample = tmp.write_file("sample.dogstatsd", "a:1|c\n");
    let sample_link = tmp.join("sample-link");
    std::os::unix::fs::symlink(Path::new(&sample), &sample_link).expect("symlink");
    let link = sample_link.to_str().expect("path is utf-8");

    let direct = output_ok(&["count", &sample]);
    let via = output_ok(&["count", link]);
    assert_eq!(direct, via, "symlink to a sample differs");
    assert!(
        direct.contains("a type: count combinations: 1 count: 1 rule: D3"),
        "{direct}"
    );

    let old = tmp.write_file("old.dogstatsd", "a:1|c\n");
    let new = tmp.write_file("new.dogstatsd", "a:1|c\nb:1|c\n");
    let old_link = tmp.join("old-link");
    let new_link = tmp.join("new-link");
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

    let price = tmp.write_file("price.json", PRICE_JSON);
    let price_link = tmp.join("price-link");
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

/// A file name that is not UTF-8 is still a valid path on Linux.
#[test]
#[cfg(target_os = "linux")]
fn file_name_that_is_not_utf8_works() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let tmp = TempDir::new();
    let path = tmp.0.join(OsStr::from_bytes(b"sample-\xff.dogstatsd"));
    std::fs::write(&path, "a:1|c\n").expect("write sample");
    let output = binary()
        .arg("count")
        .arg(&path)
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert!(stdout.starts_with("total: 1\n"), "{stdout}");
    let output = binary()
        .args(["diff"])
        .arg(&path)
        .arg(&path)
        .output()
        .expect("binary start");
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
}

/// The path is a regular file at the check and a symlink to a FIFO at the open.
/// The process must exit 2 within 2 seconds, not block in `open`.
#[test]
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn symlink_becomes_fifo_between_check_and_read() {
    let tmp = TempDir::new();
    let real = tmp.write_file("real.dogstatsd", "a:1|c\n");
    let fifo = tmp.join("pipe");
    let status = Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo start");
    assert!(status.success(), "mkfifo failed: {status}");
    let link = tmp.join("link");
    std::os::unix::fs::symlink(Path::new(&real), &link).expect("symlink to file");
    let fifo_link = tmp.join("link-fifo");
    std::os::unix::fs::symlink(&fifo, &fifo_link).expect("symlink to fifo");
    use std::os::unix::ffi::OsStrExt;
    let link_bytes = link.as_os_str().as_bytes().to_vec();

    let mut child = ChildGuard(Some(unsafe {
        binary()
            .arg("count")
            .arg(&link)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .pre_exec(trace::traceme)
            .spawn()
            .expect("binary start")
    }));
    let pid = i32::try_from(child.0.as_ref().expect("child").id()).expect("pid fits");
    let start = Instant::now();
    let limit = Duration::from_secs(2);
    let status = trace::wait_stop(pid).expect("exec stop");
    assert!(
        trace::is_stopped(status),
        "child did not stop after exec: {status:#x}"
    );
    trace::ptrace(
        trace::PTRACE_SETOPTIONS,
        pid,
        0,
        i64::from(trace::PTRACE_O_TRACESYSGOOD),
    )
    .expect("set options");
    trace::ptrace(trace::PTRACE_SYSCALL, pid, 0, 0).expect("resume");

    let mut entering = true;
    let mut swapped = false;
    let status = loop {
        if start.elapsed() >= limit {
            panic!("timeout after {limit:?}, the open waited on the FIFO");
        }
        let status = match trace::wait_flags(pid, trace::WNOHANG) {
            Ok(Some(status)) => status,
            Ok(None) => {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            Err(err) => panic!("wait: {err}"),
        };
        if let Some(code) = trace::exit_code(status) {
            break code;
        }
        if trace::stop_signal(status) == Some(trace::SIGTRAP | 0x80) {
            if entering && !swapped && trace::openat_path_is(pid, &link_bytes) {
                std::fs::rename(&fifo_link, &link).expect("swap symlink to fifo");
                swapped = true;
            }
            entering = !entering;
            trace::ptrace(trace::PTRACE_SYSCALL, pid, 0, 0).expect("resume syscall");
        } else if let Some(sig) = trace::stop_signal(status) {
            let deliver = if sig == trace::SIGTRAP {
                0
            } else {
                i64::from(sig)
            };
            trace::ptrace(trace::PTRACE_SYSCALL, pid, 0, deliver).expect("resume signal");
        }
    };
    assert!(swapped, "the open of the symlink was not seen");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut out) = child.0.as_mut().expect("child").stdout.take() {
        std::io::Read::read_to_end(&mut out, &mut stdout).expect("stdout");
    }
    if let Some(mut err) = child.0.as_mut().expect("child").stderr.take() {
        std::io::Read::read_to_end(&mut err, &mut stderr).expect("stderr");
    }
    assert_eq!(status, 2, "stdout={stdout:?} stderr={stderr:?}");
    assert!(stdout.is_empty(), "{stdout:?}");
    // The tracer already reaped the child. `Child`'s destructor would signal
    // that pid, which may already belong to another process.
    #[allow(clippy::mem_forget)]
    std::mem::forget(child.0.take());
    let stderr = String::from_utf8(stderr).expect("stderr is utf-8");
    let shown = link.to_str().expect("path is utf-8");
    assert!(
        stderr.contains(&format!("{shown}:")),
        "the message does not name the path\n{stderr}"
    );
    assert!(
        stderr.contains("not a regular file"),
        "the message lacks 'not a regular file':\n{stderr}"
    );
}

/// x86_64 Linux ptrace. Syscall numbers and register slots are the kernel ABI.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod trace {
    use std::io::{Read, Seek, SeekFrom};

    pub const PTRACE_SYSCALL: i64 = 24;
    pub const PTRACE_SETOPTIONS: i64 = 0x4200;
    pub const PTRACE_O_TRACESYSGOOD: i32 = 1;
    pub const WNOHANG: i32 = 1;
    pub const SIGTRAP: i32 = 5;
    const PTRACE_TRACEME: i64 = 0;
    const PTRACE_GETREGS: i64 = 12;
    const SYS_OPENAT: u64 = 257;
    const SYS_PTRACE: i64 = 101;
    const SYS_WAIT4: i64 = 61;
    /// `user_regs_struct.rsi` on x86_64: the pathname argument of `openat`.
    const RSI: usize = 13;
    /// `user_regs_struct.orig_rax`: the syscall number.
    const ORIG_RAX: usize = 15;

    pub fn traceme() -> std::io::Result<()> {
        let rc = syscall4(SYS_PTRACE, PTRACE_TRACEME, 0, 0, 0);
        if rc < 0 {
            Err(std::io::Error::from_raw_os_error(errno_code(rc)))
        } else {
            Ok(())
        }
    }

    pub fn ptrace(req: i64, pid: i32, addr: i64, data: i64) -> std::io::Result<i64> {
        let rc = syscall4(SYS_PTRACE, req, i64::from(pid), addr, data);
        if rc < 0 {
            Err(std::io::Error::from_raw_os_error(errno_code(rc)))
        } else {
            Ok(rc)
        }
    }

    pub fn wait_stop(pid: i32) -> std::io::Result<i32> {
        match wait_flags(pid, 0)? {
            Some(status) => Ok(status),
            None => panic!("blocking wait returned no status"),
        }
    }

    pub fn wait_flags(pid: i32, options: i32) -> std::io::Result<Option<i32>> {
        let mut status = 0i32;
        let rc = syscall4(
            SYS_WAIT4,
            i64::from(pid),
            std::ptr::addr_of_mut!(status) as i64,
            i64::from(options),
            0,
        );
        if rc == 0 {
            Ok(None)
        } else if rc < 0 {
            Err(std::io::Error::from_raw_os_error(errno_code(rc)))
        } else {
            Ok(Some(status))
        }
    }

    pub fn is_stopped(status: i32) -> bool {
        status & 0xff == 0x7f
    }

    pub fn stop_signal(status: i32) -> Option<i32> {
        if is_stopped(status) {
            Some((status >> 8) & 0xff)
        } else {
            None
        }
    }

    pub fn exit_code(status: i32) -> Option<i32> {
        if status & 0x7f == 0 {
            Some((status >> 8) & 0xff)
        } else {
            None
        }
    }

    pub fn openat_path_is(pid: i32, want: &[u8]) -> bool {
        let mut regs = [0u64; 27];
        if ptrace(PTRACE_GETREGS, pid, 0, regs.as_mut_ptr() as i64).is_err() {
            return false;
        }
        if regs[ORIG_RAX] != SYS_OPENAT {
            return false;
        }
        read_cstr(pid, regs[RSI]).as_deref() == Some(want)
    }

    fn read_cstr(pid: i32, addr: u64) -> Option<Vec<u8>> {
        let mut mem = std::fs::File::open(format!("/proc/{pid}/mem")).ok()?;
        mem.seek(SeekFrom::Start(addr)).ok()?;
        let mut buf = [0u8; 4096];
        let n = mem.read(&mut buf).ok()?;
        let end = buf[..n].iter().position(|byte| *byte == 0).unwrap_or(n);
        Some(buf[..end].to_vec())
    }

    fn errno_code(rc: i64) -> i32 {
        i32::try_from(-rc).unwrap_or(i32::MAX)
    }

    fn syscall4(number: i64, a: i64, b: i64, c: i64, d: i64) -> i64 {
        let ret: i64;
        // SAFETY: the numbers are the x86_64 Linux syscall ABI. `rcx` and `r11`
        // are clobbered by `syscall`. The closure runs after `fork` for `traceme`.
        unsafe {
            std::arch::asm!(
                "syscall",
                inlateout("rax") number => ret,
                in("rdi") a,
                in("rsi") b,
                in("rdx") c,
                in("r10") d,
                lateout("rcx") _,
                lateout("r11") _,
            );
        }
        ret
    }
}
