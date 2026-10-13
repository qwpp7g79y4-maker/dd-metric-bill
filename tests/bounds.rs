//! Bounds for duplicate lines and for many metric names.
//!
//! The duplicate line is `requests:1|c|#env:test`. The name test is a smaller
//! stand-in for 80_000 unique names: `render` and `diff` must not scan the
//! metric list once per metric.

use std::time::{Duration, Instant};

use dd_metric_bill::allow::{AllowReport, MetricAllow};
use dd_metric_bill::count::{CountReport, MetricCount, Rule, CUSTOM_METRICS_SOURCE};
use dd_metric_bill::diff::diff_samples;
use dd_metric_bill::parse::{parse_dogstatsd, Kind, Sample, Tag};
use dd_metric_bill::render::render_count;
use dd_metric_bill::tags::{MetricTags, TagKey};

const NAMES: usize = 40_000;
const LIMIT: Duration = Duration::from_secs(2);

fn rule() -> Rule {
    Rule {
        code: "D3",
        text: "count".to_string(),
        source: CUSTOM_METRICS_SOURCE,
    }
}

#[test]
fn many_unique_metric_names_stay_fast() {
    let mut metrics = Vec::with_capacity(NAMES);
    let mut tags = Vec::with_capacity(NAMES);
    let mut allow_metrics = Vec::with_capacity(NAMES);
    let mut samples = Vec::with_capacity(NAMES);
    for i in 0..NAMES {
        let name = format!("m{i}");
        metrics.push(MetricCount {
            name: name.clone(),
            kind: Kind::Count,
            combinations: 1,
            count: 1,
            rule: rule(),
        });
        tags.push(MetricTags {
            name: name.clone(),
            kind: Kind::Count,
            keys: vec![TagKey {
                key: "env".to_string(),
                unique_values: 1,
                added: 0,
            }],
        });
        allow_metrics.push(MetricAllow {
            name: name.clone(),
            kind: Kind::Count,
            ingested: None,
            indexed: 1,
        });
        samples.push(Sample {
            name,
            kind: Kind::Count,
            tags: vec![Tag {
                key: "env".to_string(),
                value: Some("test".to_string()),
            }],
            d10: false,
            counter_total: false,
        });
    }
    let report = CountReport {
        total: NAMES as u64,
        metrics,
    };
    let allow = AllowReport {
        metrics: allow_metrics,
        ingested: None,
        indexed: NAMES as u64,
    };

    let start = Instant::now();
    let text = render_count(&report, false, None, &tags, &[], Some(&allow), None);
    let render_elapsed = start.elapsed();
    assert!(
        text.starts_with(&format!("total: {NAMES}\n")),
        "total line changed"
    );
    assert!(
        render_elapsed < LIMIT,
        "render of {NAMES} names took {render_elapsed:?}, limit {LIMIT:?}"
    );

    let start = Instant::now();
    let diff = diff_samples(&samples, &samples, 4, 1, false, None).expect("diff");
    let diff_elapsed = start.elapsed();
    assert_eq!(diff.delta, 0);
    assert_eq!(diff.metrics.len(), NAMES);
    assert!(
        diff_elapsed < LIMIT,
        "diff of {NAMES} names took {diff_elapsed:?}, limit {LIMIT:?}"
    );
}

#[test]
fn duplicate_requests_line_counts_once() {
    let line = "requests:1|c|#env:test\n";
    let samples = parse_dogstatsd(&line.repeat(4)).expect("parse");
    assert_eq!(samples.len(), 1, "a repeated line is one sample");
    assert_eq!(samples[0].name, "requests");
    assert_eq!(samples[0].kind, Kind::Count);
    assert_eq!(
        samples[0].tags,
        vec![Tag {
            key: "env".to_string(),
            value: Some("test".to_string()),
        }]
    );
}

const DUPLICATE_LINE: &[u8] = b"requests:1|c|#env:test\n";
const DUPLICATE_LINES: usize = 1_000_000;

/// One million copies of `requests:1|c|#env:test` stay one metric, and on
/// Linux the process stays under 100 MiB of resident memory.
#[test]
fn million_duplicate_requests_lines_stay_under_100mb() {
    let dir = std::env::temp_dir().join(format!(
        "ddmb-dup-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir(&dir).expect("temporary directory");
    let path = dir.join("dup.dogstatsd");
    let mut buf = Vec::with_capacity(DUPLICATE_LINE.len() * DUPLICATE_LINES);
    for _ in 0..DUPLICATE_LINES {
        buf.extend_from_slice(DUPLICATE_LINE);
    }
    std::fs::write(&path, &buf).expect("write sample");
    let path = path.to_str().expect("path is utf-8").to_string();

    let (code, stdout, stderr, rss_kb) = count_file(&path);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(code, Some(0), "stderr={}", String::from_utf8_lossy(&stderr));
    assert!(
        stderr.is_empty(),
        "stderr={}",
        String::from_utf8_lossy(&stderr)
    );
    let stdout = String::from_utf8(stdout).expect("stdout is utf-8");
    assert!(stdout.starts_with("total: 1\n"), "{stdout}");
    assert!(
        stdout.contains("requests type: count combinations: 1 count: 1 rule: D3"),
        "{stdout}"
    );
    if let Some(rss_kb) = rss_kb {
        assert!(
            rss_kb < 100 * 1024,
            "peak RSS {rss_kb} KiB is not under 100 MiB"
        );
    }
}

fn count_file(path: &str) -> (Option<i32>, Vec<u8>, Vec<u8>, Option<i64>) {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        count_file_rss(path)
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
            .args(["count", path])
            .output()
            .expect("binary start");
        (output.status.code(), output.stdout, output.stderr, None)
    }
}

/// `wait4` reports `ru_maxrss` in KiB on Linux.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn count_file_rss(path: &str) -> (Option<i32>, Vec<u8>, Vec<u8>, Option<i64>) {
    use std::io::Read;
    use std::process::{Command, Stdio};

    let mut child = Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
        .args(["count", path])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary start");
    let pid = i32::try_from(child.id()).expect("pid fits");
    let start = Instant::now();
    let (code, rss_kb) = loop {
        if start.elapsed() >= Duration::from_secs(30) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("timeout counting duplicate lines");
        }
        match wait4_rss(pid, 1) {
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Ok(Some(done)) => break done,
            Err(err) => panic!("wait4: {err}"),
        }
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        out.read_to_end(&mut stdout).expect("stdout");
    }
    if let Some(mut err) = child.stderr.take() {
        err.read_to_end(&mut stderr).expect("stderr");
    }
    // `wait4` already reaped the child. `Child`'s destructor would signal
    // that pid, which may already belong to another process.
    #[allow(clippy::mem_forget)]
    std::mem::forget(child);
    (code, stdout, stderr, Some(rss_kb))
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn wait4_rss(pid: i32, options: i32) -> std::io::Result<Option<(Option<i32>, i64)>> {
    #[repr(C)]
    struct Timeval {
        sec: i64,
        usec: i64,
    }
    #[repr(C)]
    struct Rusage {
        utime: Timeval,
        stime: Timeval,
        maxrss: i64,
        _rest: [i64; 14],
    }
    let mut status = 0i32;
    let mut usage = std::mem::MaybeUninit::<Rusage>::zeroed();
    let rc = syscall4(
        61,
        i64::from(pid),
        std::ptr::addr_of_mut!(status) as i64,
        i64::from(options),
        usage.as_mut_ptr() as i64,
    );
    if rc == 0 {
        return Ok(None);
    }
    if rc < 0 {
        return Err(std::io::Error::from_raw_os_error(
            i32::try_from(-rc).unwrap_or(i32::MAX),
        ));
    }
    // SAFETY: `wait4` wrote the `rusage` when it returned a pid.
    let usage = unsafe { usage.assume_init() };
    let code = if status & 0x7f == 0 {
        Some((status >> 8) & 0xff)
    } else {
        None
    };
    Ok(Some((code, usage.maxrss)))
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn syscall4(number: i64, a: i64, b: i64, c: i64, d: i64) -> i64 {
    let ret: i64;
    // SAFETY: x86_64 Linux `wait4`. `rcx` and `r11` are clobbered by `syscall`.
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
