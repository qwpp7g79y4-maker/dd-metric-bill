use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

/// Sentences shared by the README, the reference, and `--help`.
const COUNT_ONLY: &str = "Only count accepts --explain, --plan, --hosts, --allow-tags, --prices and --unbounded-threshold.";
const HOSTS_TOGETHER: &str = "--hosts only works together with --plan";
const UNTYPED_TOGETHER: &str = "--untyped-as only works together with --format prometheus";

/// Clap options, not fields of `Cli`. A new global option does not belong here.
const NOT_CLI_FIELD: &[&str] = &["--help", "--version"];

const ONE: &[&str] = &["m:1|c|#host:a\n"];
const TWO_HOSTS: &[&str] = &["m:1|c|#host:a\nm:1|c|#host:b\n"];
const TWO_TAGS: &[&str] = &["m:1|c|#env:prod,host:a\n"];
const HIST: &[&str] = &["h:1|h|#host:a\n"];
const DIST: &[&str] = &["d:1|d|#host:a\n"];
const THRESHOLD: &[&str] = &["m:1|c|#bucket:v1\nm:1|c|#bucket:v2\nm:1|c|#bucket:v3\n"];
const UNTYPED: &[&str] = &["a 1\n"];
const DIFF_OK: &[&str] = &["m:1|c|#host:a\n", "m:1|c|#host:b\n"];
const DIFF_TAGS: &[&str] = &[
    "m:1|c|#env:a,host:a\n",
    "m:1|c|#env:a,host:a\nm:1|c|#env:b,host:b\n",
];
const DIFF_HIST: &[&str] = &["h:1|h|#host:a\n", "h:1|h|#host:a\nh:1|h|#host:b\n"];
const DIFF_DIST: &[&str] = &["d:1|d|#host:a\n", "d:1|d|#host:a\nd:1|d|#host:b\n"];
const DIFF_UNTYPED: &[&str] = &["a 1\n", "b 1\n"];

struct Case {
    count: Expectation,
    diff: Expectation,
}

enum Expectation {
    /// Both calls exit 0 and stdout differs. `with` sets the option, `baseline` does not.
    Output {
        baseline: &'static [&'static str],
        with: &'static [&'static str],
        samples: &'static [&'static str],
    },
    /// `baseline` exits 0. `with` exits with the named code.
    Exitcode {
        baseline: &'static [&'static str],
        with: &'static [&'static str],
        samples: &'static [&'static str],
        code: i32,
    },
    /// Exit 3, empty stdout, stderr exactly `option does not apply to diff: <option>`.
    DoesNotApply {
        args: &'static [&'static str],
        samples: &'static [&'static str],
    },
    /// Without the other option, exit 3. With that option, stdout differs.
    Together {
        only: &'static [&'static str],
        message: &'static str,
        baseline: &'static [&'static str],
        with: &'static [&'static str],
        samples: &'static [&'static str],
    },
}

const CASES: &[(&str, Case)] = &[
    (
        "--format",
        Case {
            count: Expectation::Exitcode {
                baseline: &[],
                with: &["--format", "jsonl"],
                samples: ONE,
                code: 2,
            },
            diff: Expectation::Exitcode {
                baseline: &[],
                with: &["--format", "jsonl"],
                samples: DIFF_OK,
                code: 2,
            },
        },
    ),
    (
        "--json",
        Case {
            count: Expectation::Output {
                baseline: &[],
                with: &["--json"],
                samples: ONE,
            },
            diff: Expectation::Output {
                baseline: &[],
                with: &["--json"],
                samples: DIFF_OK,
            },
        },
    ),
    (
        "--explain",
        Case {
            count: Expectation::Output {
                baseline: &[],
                with: &["--explain"],
                samples: ONE,
            },
            diff: Expectation::DoesNotApply {
                args: &["--explain"],
                samples: DIFF_OK,
            },
        },
    ),
    (
        "--hosts",
        Case {
            count: Expectation::Together {
                only: &["--hosts", "9"],
                message: HOSTS_TOGETHER,
                baseline: &["--plan", "pro"],
                with: &["--plan", "pro", "--hosts", "9"],
                samples: TWO_HOSTS,
            },
            diff: Expectation::DoesNotApply {
                args: &["--hosts", "3"],
                samples: DIFF_OK,
            },
        },
    ),
    (
        "--plan",
        Case {
            count: Expectation::Output {
                baseline: &[],
                with: &["--plan", "pro"],
                samples: TWO_HOSTS,
            },
            diff: Expectation::DoesNotApply {
                args: &["--plan", "pro"],
                samples: DIFF_OK,
            },
        },
    ),
    (
        "--histogram-aggregates",
        Case {
            count: Expectation::Output {
                baseline: &[],
                with: &["--histogram-aggregates", "6"],
                samples: HIST,
            },
            diff: Expectation::Output {
                baseline: &[],
                with: &["--histogram-aggregates", "6"],
                samples: DIFF_HIST,
            },
        },
    ),
    (
        "--histogram-percentiles",
        Case {
            count: Expectation::Output {
                baseline: &[],
                with: &["--histogram-percentiles", "0"],
                samples: HIST,
            },
            diff: Expectation::Output {
                baseline: &[],
                with: &["--histogram-percentiles", "0"],
                samples: DIFF_HIST,
            },
        },
    ),
    (
        "--percentiles",
        Case {
            count: Expectation::Output {
                baseline: &[],
                with: &["--percentiles"],
                samples: DIST,
            },
            diff: Expectation::Output {
                baseline: &[],
                with: &["--percentiles"],
                samples: DIFF_DIST,
            },
        },
    ),
    (
        "--allow-tags",
        Case {
            count: Expectation::Output {
                baseline: &[],
                with: &["--allow-tags", "env"],
                samples: TWO_TAGS,
            },
            diff: Expectation::DoesNotApply {
                args: &["--allow-tags", "env"],
                samples: DIFF_OK,
            },
        },
    ),
    (
        "--prices",
        Case {
            count: Expectation::Output {
                baseline: &[],
                with: &["--prices", "prices/datadog-example.json"],
                samples: ONE,
            },
            diff: Expectation::DoesNotApply {
                args: &["--prices", "prices/datadog-example.json"],
                samples: DIFF_OK,
            },
        },
    ),
    (
        "--budget",
        Case {
            count: Expectation::Exitcode {
                baseline: &[],
                with: &["--budget", "0"],
                samples: ONE,
                code: 1,
            },
            diff: Expectation::Exitcode {
                baseline: &[],
                with: &["--budget", "0"],
                samples: DIFF_OK,
                code: 1,
            },
        },
    ),
    (
        "--top",
        Case {
            count: Expectation::Output {
                baseline: &[],
                with: &["--top", "1"],
                samples: TWO_TAGS,
            },
            diff: Expectation::Output {
                baseline: &[],
                with: &["--top", "1"],
                samples: DIFF_TAGS,
            },
        },
    ),
    (
        "--unbounded-threshold",
        Case {
            count: Expectation::Output {
                baseline: &[],
                with: &["--unbounded-threshold", "2"],
                samples: THRESHOLD,
            },
            diff: Expectation::DoesNotApply {
                args: &["--unbounded-threshold", "5"],
                samples: DIFF_OK,
            },
        },
    ),
    (
        "--untyped-as",
        Case {
            count: Expectation::Together {
                only: &["--untyped-as", "gauge"],
                message: UNTYPED_TOGETHER,
                baseline: &["--format", "prometheus"],
                with: &["--format", "prometheus", "--untyped-as", "gauge"],
                samples: UNTYPED,
            },
            diff: Expectation::Together {
                only: &["--untyped-as", "gauge"],
                message: UNTYPED_TOGETHER,
                baseline: &["--format", "prometheus"],
                with: &["--format", "prometheus", "--untyped-as", "gauge"],
                samples: DIFF_UNTYPED,
            },
        },
    ),
];

fn binary() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"));
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"));
    cmd
}

fn failure(output: &Output) -> String {
    format!(
        "status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("reading {}: {err}", path.display()))
}

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .to_str()
        .expect("path is utf-8")
        .to_string()
}

fn help_of(args: &[&str]) -> String {
    let output = binary().args(args).output().expect("binary start");
    assert_eq!(output.status.code(), Some(0), "{}", failure(&output));
    String::from_utf8(output.stdout).expect("help is utf-8")
}

struct TempPath(std::path::PathBuf);

impl TempPath {
    fn new(contents: &str) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("dd-metric-bill-test-opt-{nanos}-{n}.txt"));
        std::fs::write(&path, contents).expect("write sample");
        Self(path)
    }

    fn as_str(&self) -> &str {
        self.0.to_str().expect("path is utf-8")
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn paths(contents: &[&str]) -> Vec<TempPath> {
    contents.iter().copied().map(TempPath::new).collect()
}

struct Outcome {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run_case(sub: &str, flags: &[&str], files: &[TempPath]) -> Outcome {
    let mut args = Vec::with_capacity(1 + flags.len() + files.len());
    args.push(sub.to_string());
    args.extend(flags.iter().copied().map(str::to_string));
    args.extend(files.iter().map(|path| path.as_str().to_string()));
    let output = binary().args(&args).output().expect("binary start");
    Outcome {
        code: output.status.code(),
        stdout: String::from_utf8(output.stdout).expect("stdout is utf-8"),
        stderr: String::from_utf8(output.stderr).expect("stderr is utf-8"),
    }
}

fn shorten(text: &str) -> String {
    let plat = text.replace('\n', "\\n");
    if plat.chars().count() > 160 {
        let end = plat
            .char_indices()
            .nth(160)
            .map(|(i, _)| i)
            .unwrap_or(plat.len());
        format!("{}…", &plat[..end])
    } else {
        plat
    }
}

fn flags_have(flags: &[&str], option: &str) -> bool {
    flags.contains(&option)
}

fn other_options<'a>(flags: &'a [&str], option: &str) -> Vec<&'a str> {
    flags
        .iter()
        .copied()
        .filter(|part| part.starts_with("--") && *part != option)
        .collect()
}

fn require_not_other_option(option: &str, flags: &[&str]) -> Option<String> {
    let other = other_options(flags, option);
    if other.is_empty() {
        None
    } else {
        Some(format!(
            "{option} appears together with {other:?}; without that option the case must exit 3"
        ))
    }
}

fn value_after<'a>(flags: &'a [&str], option: &str) -> Option<&'a str> {
    let pos = flags.iter().position(|part| *part == option)?;
    flags
        .get(pos + 1)
        .copied()
        .filter(|value| !value.starts_with("--"))
}

fn default<'a>(help: &'a str, option: &str) -> Option<&'a str> {
    for line in help.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('-') {
            continue;
        }
        if !trimmed.split_whitespace().any(|word| word == option) {
            continue;
        }
        let marker = "[default: ";
        let start = line.find(marker)? + marker.len();
        let rest = &line[start..];
        let end = rest.find(']')?;
        return Some(rest[..end].trim());
    }
    None
}

fn check_not_default(help: &str, option: &str, flags: &[&str]) -> Option<String> {
    let default = default(help, option)?;
    match value_after(flags, option) {
        Some(value) if value != default => None,
        Some(value) => Some(format!(
            "{option} {value} is the default; a different value must change the output"
        )),
        None => Some(format!(
            "{option} has default {default}; the case must set a different value"
        )),
    }
}

fn count_samples(sub: &str) -> usize {
    match sub {
        "count" => 1,
        "diff" => 2,
        _ => panic!("unknown subcommand {sub}"),
    }
}

fn check(help: &str, sub: &str, option: &str, expectation: &Expectation) -> Option<String> {
    let samples = match expectation {
        Expectation::Output { samples, .. }
        | Expectation::Exitcode { samples, .. }
        | Expectation::DoesNotApply { samples, .. }
        | Expectation::Together { samples, .. } => *samples,
    };
    if samples.len() != count_samples(sub) {
        return Some(format!(
            "{sub} expects {} samples, got {}",
            count_samples(sub),
            samples.len()
        ));
    }
    match expectation {
        Expectation::Output {
            baseline,
            with,
            samples,
        } => {
            if !flags_have(with, option) || flags_have(baseline, option) {
                return Some(format!("{option} belongs only in the `with` call"));
            }
            if let Some(error) = require_not_other_option(option, with) {
                return Some(error);
            }
            if let Some(error) = check_not_default(help, option, with) {
                return Some(error);
            }
            let files = paths(samples);
            let left = run_case(sub, baseline, &files);
            let right = run_case(sub, with, &files);
            if left.code != Some(0) || right.code != Some(0) || left.stdout == right.stdout {
                Some(format!(
                    "no effect on the output: baseline status={:?} with status={:?} baseline={} with={}",
                    left.code,
                    right.code,
                    shorten(&left.stdout),
                    shorten(&right.stdout)
                ))
            } else {
                None
            }
        }
        Expectation::Exitcode {
            baseline,
            with,
            samples,
            code,
        } => {
            if *code == 0 {
                return Some(format!("{option}: exit 0 does not count as an effect"));
            }
            if !flags_have(with, option) || flags_have(baseline, option) {
                return Some(format!("{option} belongs only in the `with` call"));
            }
            if let Some(error) = require_not_other_option(option, with) {
                return Some(error);
            }
            if let Some(error) = check_not_default(help, option, with) {
                return Some(error);
            }
            let files = paths(samples);
            let left = run_case(sub, baseline, &files);
            let right = run_case(sub, with, &files);
            if left.code != Some(0) || right.code != Some(*code) {
                Some(format!(
                    "exit code: baseline {:?} with {:?} (expected {code}) stderr={}",
                    left.code,
                    right.code,
                    shorten(&right.stderr)
                ))
            } else {
                None
            }
        }
        Expectation::DoesNotApply { args, samples } => {
            if sub != "diff" {
                return Some("rejection is only specified for diff".to_string());
            }
            if !flags_have(args, option) {
                return Some(format!("{option} is missing from the rejection"));
            }
            if let Some(error) = require_not_other_option(option, args) {
                return Some(error);
            }
            if let Some(error) = check_not_default(help, option, args) {
                return Some(error);
            }
            let files = paths(samples);
            let out = run_case("diff", args, &files);
            let message = format!("option does not apply to diff: {option}\n");
            if out.code != Some(3) || !out.stdout.is_empty() || out.stderr != message {
                Some(format!(
                    "rejection: status={:?} stdout={} stderr={}",
                    out.code,
                    shorten(&out.stdout),
                    shorten(&out.stderr)
                ))
            } else {
                None
            }
        }
        Expectation::Together {
            only,
            message,
            baseline,
            with,
            samples,
        } => check_together(
            help,
            sub,
            option,
            TogetherArgs {
                only,
                message,
                baseline,
                with,
                samples,
            },
        ),
    }
}

struct TogetherArgs<'a> {
    only: &'a [&'a str],
    message: &'a str,
    baseline: &'a [&'a str],
    with: &'a [&'a str],
    samples: &'a [&'a str],
}

fn check_together(help: &str, sub: &str, option: &str, case: TogetherArgs<'_>) -> Option<String> {
    let TogetherArgs {
        only,
        message,
        baseline,
        with,
        samples,
    } = case;
    if let Some(error) = require_not_other_option(option, only) {
        return Some(error);
    }
    if !flags_have(only, option) {
        return Some(format!("{option} is missing from the `only` call"));
    }
    if other_options(with, option).is_empty()
        || !flags_have(with, option)
        || flags_have(baseline, option)
    {
        return Some(format!(
            "{option}: the combined call must set the option together with another option"
        ));
    }
    if let Some(error) = check_not_default(help, option, with) {
        return Some(error);
    }
    let files = paths(samples);
    let out = run_case(sub, only, &files);
    let expected = format!("{message}\n");
    if out.code != Some(3) || !out.stdout.is_empty() || out.stderr != expected {
        return Some(format!(
            "without the combination: status={:?} stdout={} stderr={}",
            out.code,
            shorten(&out.stdout),
            shorten(&out.stderr)
        ));
    }
    let left = run_case(sub, baseline, &files);
    let right = run_case(sub, with, &files);
    if left.code != Some(0) || right.code != Some(0) || left.stdout == right.stdout {
        Some(format!(
            "combined, no effect: baseline status={:?} with status={:?} baseline={} with={}",
            left.code,
            right.code,
            shorten(&left.stdout),
            shorten(&right.stdout)
        ))
    } else {
        None
    }
}

fn global_options(help: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_options = false;
    for line in help.lines() {
        if !in_options {
            if line == "Options:" {
                in_options = true;
            }
            continue;
        }
        if line.is_empty() {
            break;
        }
        let trimmed = line.trim_start();
        if !trimmed.starts_with('-') {
            continue;
        }
        for word in trimmed.split_whitespace() {
            let Some(rest) = word.strip_prefix("--") else {
                continue;
            };
            if rest.is_empty()
                || !rest
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                continue;
            }
            let name = format!("--{rest}");
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    assert!(!names.is_empty(), "no options in --help:\n{help}");
    names
}

fn assert_exit3(args: &[String], option: &str) {
    let output = binary().args(args).output().expect("binary start");
    assert_eq!(output.status.code(), Some(3), "{}", failure(&output));
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty:\n{}",
        failure(&output)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    assert_eq!(
        stderr,
        format!("option does not apply to diff: {option}\n"),
        "{stderr}"
    );
}

fn assert_message(args: &[String], message: &str) {
    let output = binary().args(args).output().expect("binary start");
    assert_eq!(output.status.code(), Some(3), "{}", failure(&output));
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty:\n{}",
        failure(&output)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    assert_eq!(stderr, format!("{message}\n"), "{stderr}");
}

fn assert_rejection(flag: &[&str]) {
    let option = flag
        .iter()
        .copied()
        .find(|part| part.starts_with("--"))
        .expect("option");
    let mut args = vec!["diff".to_string()];
    args.extend(flag.iter().copied().map(str::to_string));
    args.push(fixture("diff-old.dogstatsd"));
    args.push(fixture("diff-new.dogstatsd"));
    assert_exit3(&args, option);
}

#[test]
fn diff_explain_exits_3() {
    assert_rejection(&["--explain"]);
    let dropped = std::env::temp_dir().join(format!(
        "dd-metric-bill-test-opt-missing-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&dropped);
    let path = dropped.to_str().expect("path is utf-8").to_string();
    assert_exit3(
        &[
            "diff".to_string(),
            "--explain".to_string(),
            path.clone(),
            path,
        ],
        "--explain",
    );
}

#[test]
fn diff_plan_exits_3() {
    assert_rejection(&["--plan", "pro"]);
}

#[test]
fn diff_hosts_exits_3() {
    assert_rejection(&["--hosts", "3"]);
}

#[test]
fn count_hosts_without_plan_exits_3() {
    assert_message(
        &[
            "count".to_string(),
            "--hosts".to_string(),
            "3".to_string(),
            fixture("b1-count.dogstatsd"),
        ],
        HOSTS_TOGETHER,
    );
    let dropped = std::env::temp_dir().join(format!(
        "dd-metric-bill-test-opt-hosts-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&dropped);
    let path = dropped.to_str().expect("path is utf-8").to_string();
    assert_message(
        &[
            "count".to_string(),
            "--hosts".to_string(),
            "3".to_string(),
            path,
        ],
        HOSTS_TOGETHER,
    );
    let price = std::env::temp_dir().join(format!(
        "dd-metric-bill-test-opt-hosts-price-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&price);
    assert_message(
        &[
            "count".to_string(),
            "--hosts".to_string(),
            "9".to_string(),
            "--prices".to_string(),
            price.to_str().expect("path is utf-8").to_string(),
            fixture("b1-count.dogstatsd"),
        ],
        HOSTS_TOGETHER,
    );
    assert_rejection(&["--hosts", "3"]);
}

#[test]
fn untyped_as_without_prometheus_exits_3() {
    let dropped = std::env::temp_dir().join(format!(
        "dd-metric-bill-test-opt-untyped-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&dropped);
    let path = dropped.to_str().expect("path is utf-8").to_string();
    assert_message(
        &[
            "count".to_string(),
            "--untyped-as".to_string(),
            "gauge".to_string(),
            path.clone(),
        ],
        UNTYPED_TOGETHER,
    );
    assert_message(
        &[
            "diff".to_string(),
            "--untyped-as".to_string(),
            "count".to_string(),
            path.clone(),
            path,
        ],
        UNTYPED_TOGETHER,
    );
    assert_message(
        &[
            "count".to_string(),
            "--format".to_string(),
            "jsonl".to_string(),
            "--untyped-as".to_string(),
            "gauge".to_string(),
            fixture("b1-count.dogstatsd"),
        ],
        UNTYPED_TOGETHER,
    );
}

#[test]
fn diff_allow_tags_exits_3() {
    assert_rejection(&["--allow-tags", "env"]);
}

#[test]
fn diff_prices_exits_3() {
    assert_rejection(&["--prices", "prices/datadog-example.json"]);
}

#[test]
fn diff_unbounded_threshold_exits_3() {
    assert_rejection(&["--unbounded-threshold", "5"]);
    assert_rejection(&["--unbounded-threshold", "50"]);
}

#[test]
fn diff_prices_broken_or_missing_is_exit_3() {
    let broken = TempPath::new("{\n");
    let mut args = vec![
        "diff".to_string(),
        "--prices".to_string(),
        broken.as_str().to_string(),
    ];
    args.push(fixture("diff-old.dogstatsd"));
    args.push(fixture("diff-new.dogstatsd"));
    assert_exit3(&args, "--prices");

    let dropped = std::env::temp_dir().join(format!(
        "dd-metric-bill-test-opt-not-price-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&dropped);
    let mut args = vec![
        "diff".to_string(),
        "--prices".to_string(),
        dropped.to_str().expect("path is utf-8").to_string(),
    ];
    args.push(fixture("diff-old.dogstatsd"));
    args.push(fixture("diff-new.dogstatsd"));
    assert_exit3(&args, "--prices");
}

#[test]
fn every_global_option_has_an_effect_or_exit_3() {
    let help = help_of(&["--help"]);
    let names = global_options(&help);
    let out_help: BTreeSet<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|name| !NOT_CLI_FIELD.contains(name))
        .collect();
    let covered: BTreeSet<&str> = CASES.iter().map(|(name, _)| *name).collect();

    let mut errors = Vec::new();
    for name in &out_help {
        if !covered.contains(name) {
            errors.push(format!("no case for global option {name}"));
        }
    }
    for name in &covered {
        if !out_help.contains(name) {
            errors.push(format!("case {name} is not a global option in --help"));
        }
    }

    for (option, case) in CASES {
        for (sub, expectation) in [("count", &case.count), ("diff", &case.diff)] {
            if let Some(error) = check(&help, sub, option, expectation) {
                errors.push(format!("{sub} {option}: {error}"));
            }
        }
    }
    assert!(
        errors.is_empty(),
        "global options without an effect or exit 3:\n{}",
        errors.join("\n")
    );
}

#[test]
fn count_only_and_combination_rules_match_the_docs() {
    let readme = read("README.md");
    let reference = read("docs/reference.md");
    for sentence in [COUNT_ONLY, HOSTS_TOGETHER, UNTYPED_TOGETHER] {
        assert!(
            readme.contains(sentence),
            "README is missing the sentence: {sentence}"
        );
        assert!(
            reference.contains(sentence),
            "reference is missing the sentence: {sentence}"
        );
        for args in [&["--help"][..], &["count", "--help"], &["diff", "--help"]] {
            let help = help_of(args);
            assert!(
                help.contains(sentence),
                "{args:?} is missing the sentence: {sentence}"
            );
        }
    }
}

#[test]
fn prometheus_label_with_spaces_around_equals_counts_the_same() {
    let tight = TempPath::new("# TYPE a gauge\na{host=\"x\"} 1\n");
    let loose = TempPath::new("# TYPE a gauge\na{host = \"x\"} 1\n");
    let left = run_case("count", &["--format", "prometheus"], &[tight]);
    let right = run_case("count", &["--format", "prometheus"], &[loose]);
    assert_eq!(left.code, Some(0), "{}", left.stderr);
    assert_eq!(left.stdout, right.stdout, "spaced:\n{}", right.stdout);
    assert!(left.stdout.contains("total: 1\n"), "{}", left.stdout);
}
