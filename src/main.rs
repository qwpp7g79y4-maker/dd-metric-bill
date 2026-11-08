mod interrupt;

use std::cell::Cell;
use std::fs::File;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use dd_metric_bill::advice::add_advice;
use dd_metric_bill::allow::{parse_allow_tags, simulate};
use dd_metric_bill::cost::{estimate_cost, parse_prices, Prices};
use dd_metric_bill::count::{
    allotment, count_by_metric, unique_hosts, Allotment, CountError, Plan as CountPlan,
    DEFAULT_HISTOGRAM_AGGREGATES, DEFAULT_HISTOGRAM_PERCENTILES,
};
use dd_metric_bill::diff::diff_samples;
use dd_metric_bill::parse::{
    parse_dogstatsd, parse_jsonl, parse_prometheus, Kind, ParseError, Sample,
};
use dd_metric_bill::render::{render_count, render_count_json, render_diff, render_diff_json};
use dd_metric_bill::tags::{tag_breakdown, unbounded_tags, DEFAULT_UNBOUNDED_THRESHOLD};

const EXIT_OK: u8 = 0;
const EXIT_BUDGET: u8 = 1;
const EXIT_INPUT: u8 = 2;
const EXIT_USAGE: u8 = 3;

/// Largest sample or price file accepted, in bytes (256 MiB).
/// The measured input of 104857600 bytes (100 MiB) stays under this.
const MAX_INPUT_BYTES: u64 = 256 * 1024 * 1024;

const AFTER_HELP: &str = "\
Contracts with Metric Name pricing are not supported.
Only count accepts --explain, --plan, --hosts, --allow-tags, --prices and --unbounded-threshold.
--hosts only works together with --plan.
--untyped-as only works together with --format prometheus.";

#[derive(Parser)]
#[command(
    name = "dd-metric-bill",
    bin_name = "dd-metric-bill",
    version,
    about = "Estimate the number of billable Datadog custom metrics in a sample.",
    after_help = AFTER_HELP,
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Input format: dogstatsd, jsonl or prometheus.
    #[arg(
        long,
        global = true,
        value_enum,
        default_value = "dogstatsd",
        value_name = "FORMAT"
    )]
    format: Format,

    /// Write the output as JSON.
    #[arg(long, global = true)]
    json: bool,

    /// Show the rule and the source for each metric.
    #[arg(long, global = true)]
    explain: bool,

    /// Number of hosts for the allotment.
    #[arg(long, global = true, value_name = "N")]
    hosts: Option<u64>,

    /// Plan for the allotment: pro or enterprise.
    #[arg(long, global = true, value_enum, value_name = "PLAN")]
    plan: Option<Plan>,

    /// Number of histogram aggregates per tag combination.
    #[arg(long, global = true, value_name = "N", default_value_t = DEFAULT_HISTOGRAM_AGGREGATES)]
    histogram_aggregates: u64,

    /// Number of histogram percentiles per tag combination.
    #[arg(
        long,
        global = true,
        value_name = "N",
        default_value_t = DEFAULT_HISTOGRAM_PERCENTILES
    )]
    histogram_percentiles: u64,

    /// Include percentiles for distribution metrics.
    #[arg(long, global = true)]
    percentiles: bool,

    /// Comma-separated tag keys that may be indexed.
    #[arg(long, global = true, value_name = "LIST")]
    allow_tags: Option<String>,

    /// Path to a price file (JSON).
    #[arg(long, global = true, value_name = "FILE")]
    prices: Option<PathBuf>,

    /// Fail if the number of custom metrics is above N.
    #[arg(long, global = true, value_name = "N")]
    budget: Option<u64>,

    /// Number of tag keys in the breakdown.
    #[arg(long, global = true, value_name = "N")]
    top: Option<u64>,

    /// Unique values above N mark an unbounded tag.
    #[arg(
        long,
        global = true,
        value_name = "N",
        default_value_t = DEFAULT_UNBOUNDED_THRESHOLD
    )]
    unbounded_threshold: u64,

    /// Count Prometheus metrics without a type as gauge or count.
    #[arg(long, global = true, value_enum, value_name = "KIND")]
    untyped_as: Option<UntypedAs>,

    /// Hide the hint line. No context line.
    #[arg(long, global = true, conflicts_with = "verbose")]
    quiet: bool,

    /// Print the hint line, and on success one context line on stderr.
    #[arg(long, global = true, conflicts_with = "quiet")]
    verbose: bool,

    /// Write the report to this file instead of stdout.
    /// An existing file is replaced. A missing parent directory is an error.
    #[arg(long, global = true, value_name = "PATH")]
    output: Option<PathBuf>,
}

thread_local! {
    /// Tip/hint lines. A terminal shows them. A pipe does not, unless `--verbose`.
    static EXTRA: Cell<bool> = const { Cell::new(false) };
}

fn set_extra(extra: bool) {
    EXTRA.with(|cell| cell.set(extra));
}

fn extra() -> bool {
    EXTRA.with(|cell| cell.get())
}

/// `--quiet` hides hints even on a terminal. `--verbose` shows them even on a pipe.
/// Both flags together are a clap usage error and never reach a report.
fn extra_from_args() -> bool {
    let mut quiet = false;
    let mut verbose = false;
    // `args()` panics when a path is not Unicode. `args_os` does not.
    for arg in std::env::args_os().skip(1) {
        if arg == "--" {
            break;
        } else if arg == "--quiet" {
            quiet = true;
        } else if arg == "--verbose" {
            verbose = true;
        }
    }
    if quiet {
        false
    } else if verbose {
        true
    } else {
        io::stderr().is_terminal()
    }
}

#[derive(Subcommand)]
enum Command {
    /// Count billable custom metrics in one or more samples.
    #[command(after_help = AFTER_HELP)]
    Count {
        /// Sample file. `-` reads standard input.
        #[arg(value_name = "SAMPLE", required = true)]
        samples: Vec<PathBuf>,
    },
    /// Compare two samples.
    #[command(after_help = AFTER_HELP)]
    Diff {
        /// Old sample. `-` reads standard input.
        #[arg(value_name = "OLD")]
        old: PathBuf,
        /// New sample. `-` reads standard input.
        #[arg(value_name = "NEW")]
        new: PathBuf,
    },
    /// Print a shell completion script.
    Completions {
        /// Shell to print the script for: bash or zsh.
        #[arg(value_enum, value_name = "SHELL")]
        shell: CompletionShell,
    },
}

/// Shells for which `completions` can print a script.
#[derive(Clone, Copy, ValueEnum)]
enum CompletionShell {
    Bash,
    Zsh,
}

impl From<CompletionShell> for Shell {
    fn from(shell: CompletionShell) -> Self {
        match shell {
            CompletionShell::Bash => Shell::Bash,
            CompletionShell::Zsh => Shell::Zsh,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Dogstatsd,
    Jsonl,
    Prometheus,
}

#[derive(Clone, Copy, ValueEnum)]
enum Plan {
    Pro,
    Enterprise,
}

#[derive(Clone, Copy, ValueEnum)]
enum UntypedAs {
    Gauge,
    Count,
}

fn main() -> ExitCode {
    interrupt::install();
    set_extra(extra_from_args());
    let code = match Cli::command().try_get_matches() {
        Ok(matches) => {
            let threshold_explicit = matches.value_source("unbounded_threshold")
                == Some(clap::parser::ValueSource::CommandLine);
            match Cli::from_arg_matches(&matches) {
                Ok(cli) => run(&cli, threshold_explicit),
                Err(err) => usage_exit(&err),
            }
        }
        Err(err) => usage_exit(&err),
    };
    if code == interrupt::EXIT_INTERRUPTED || interrupt::interrupted() {
        note(format_args!("interrupted by SIGINT"));
        ExitCode::from(interrupt::EXIT_INTERRUPTED)
    } else {
        ExitCode::from(code)
    }
}

fn usage_exit(err: &clap::Error) -> u8 {
    let code = match err.kind() {
        clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => EXIT_OK,
        _ => EXIT_USAGE,
    };
    if let Err(io_err) = err.print() {
        note_hint(
            format_args!("could not write the message: {io_err}"),
            "check that standard output is still open and has free space, or redirect it to a file",
        );
        return EXIT_USAGE;
    }
    code
}

/// Writes one stderr line. A full or closed stderr must not panic.
fn note(args: std::fmt::Arguments<'_>) {
    let mut err = io::stderr().lock();
    let _ = writeln!(err, "{args}");
}

/// Writes an error line, then a `hint:` line when extra context is on.
///
/// Extra context is on for a terminal and for `--verbose`. A pipe, a redirect,
/// and `--quiet` keep the first line only, so captured stderr stays one line.
fn note_hint(args: std::fmt::Arguments<'_>, hint: &str) {
    let mut err = io::stderr().lock();
    if extra() {
        let _ = writeln!(err, "{args}\nhint: {hint}");
    } else {
        let _ = writeln!(err, "{args}");
    }
}

/// Path of a temporary file next to `path`, so the finished report can replace
/// `path` in one rename. SIGINT before that rename removes the temporary file
/// and leaves `path` as it was.
fn temp_beside(path: &Path) -> PathBuf {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut name = std::ffi::OsString::from(".");
    name.push(
        path.file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("report")),
    );
    name.push(format!(".dd-metric-bill-{}.tmp", std::process::id()));
    parent.join(name)
}

/// Writes the report to stdout, or to `--output` when that path is set.
/// A missing parent directory is not created. A failed write is exit 2.
/// SIGINT returns [`interrupt::EXIT_INTERRUPTED`] and does not leave a partial file.
fn write_report(path: Option<&Path>, text: &str) -> u8 {
    if interrupt::interrupted() {
        return interrupt::EXIT_INTERRUPTED;
    }
    let Some(path) = path else {
        let result = interrupt::with_sigint_blocked(|| {
            if interrupt::interrupted() {
                return None;
            }
            let mut out = io::stdout().lock();
            Some(out.write_all(text.as_bytes()).and_then(|()| out.flush()))
        });
        return match result {
            None => interrupt::EXIT_INTERRUPTED,
            Some(Ok(())) if interrupt::interrupted() => interrupt::EXIT_INTERRUPTED,
            Some(Ok(())) => EXIT_OK,
            Some(Err(_)) if interrupt::interrupted() => interrupt::EXIT_INTERRUPTED,
            Some(Err(err)) => {
                note_hint(
                    format_args!("could not write the report: {err}"),
                    "check that standard output is still open and has free space, or redirect it to a file",
                );
                EXIT_INPUT
            }
        };
    };
    let tmp = temp_beside(path);
    let created = File::create(&tmp).and_then(|mut file| {
        file.write_all(text.as_bytes())?;
        file.flush()
    });
    if let Err(err) = created {
        let _ = std::fs::remove_file(&tmp);
        if interrupt::interrupted() {
            return interrupt::EXIT_INTERRUPTED;
        }
        note_hint(
            format_args!("could not write the report to {}: {err}", path.display()),
            "check that the parent directory exists and is writable; it is not created",
        );
        return EXIT_INPUT;
    }
    let renamed = interrupt::with_sigint_blocked(|| {
        if interrupt::interrupted() {
            return None;
        }
        Some(std::fs::rename(&tmp, path))
    });
    match renamed {
        None => {
            let _ = std::fs::remove_file(&tmp);
            interrupt::EXIT_INTERRUPTED
        }
        Some(Ok(())) if interrupt::interrupted() => interrupt::EXIT_INTERRUPTED,
        Some(Ok(())) => EXIT_OK,
        Some(Err(_)) if interrupt::interrupted() => {
            let _ = std::fs::remove_file(&tmp);
            interrupt::EXIT_INTERRUPTED
        }
        Some(Err(err)) => {
            let _ = std::fs::remove_file(&tmp);
            note_hint(
                format_args!("could not write the report to {}: {err}", path.display()),
                "check that the parent directory exists and is writable; it is not created",
            );
            EXIT_INPUT
        }
    }
}

fn run(cli: &Cli, threshold_explicit: bool) -> u8 {
    if let Command::Completions { shell } = &cli.command {
        let mut script = Vec::new();
        generate(
            Shell::from(*shell),
            &mut Cli::command(),
            "dd-metric-bill",
            &mut script,
        );
        // Completions stay on stdout. Other options, including `--output`, are ignored.
        return match String::from_utf8(script) {
            Ok(text) => write_report(None, &text),
            Err(_) => EXIT_INPUT,
        };
    }
    // Refuse diff options first, so `diff --hosts` keeps the diff message, even without `--plan`.
    if let Command::Diff { .. } = &cli.command {
        if let Some(option) = diff_forbidden_option(cli, threshold_explicit) {
            note(format_args!("option does not apply to diff: {option}"));
            return EXIT_USAGE;
        }
    }
    if let Some(message) = combination_rejection(cli) {
        note(format_args!("{message}"));
        return EXIT_USAGE;
    }
    let prices = match cli.prices.as_deref() {
        Some(path) => match read_prices(path) {
            Ok(prices) => Some(prices),
            Err(code) => return code,
        },
        None => None,
    };
    match &cli.command {
        Command::Count { samples } => count_paths(cli, samples, prices.as_ref()),
        Command::Diff { old, new } => diff_paths(cli, old, new),
        Command::Completions { .. } => EXIT_OK,
    }
}

fn count_paths(cli: &Cli, paths: &[PathBuf], prices: Option<&Prices>) -> u8 {
    let mut all = Vec::new();
    for path in paths {
        match read_samples(path, cli.format, cli.untyped_as) {
            Ok(mut samples) => all.append(&mut samples),
            Err(code) => return code,
        }
    }
    let report = match count_by_metric(
        &all,
        cli.histogram_aggregates,
        cli.histogram_percentiles,
        cli.percentiles,
    ) {
        Ok(report) => report,
        Err(err) => return usage_error(&err),
    };
    let allow_tags = cli.allow_tags.as_deref().map(parse_allow_tags);
    let allow = match simulate(
        &all,
        allow_tags.as_ref(),
        cli.histogram_aggregates,
        cli.histogram_percentiles,
        cli.percentiles,
    ) {
        Ok(allow) => allow,
        Err(err) => return usage_error(&err),
    };
    let allotment = match allotment_for(cli, &all, allow.indexed, allow.ingested) {
        Ok(allotment) => allotment,
        Err(err) => return usage_error(&err),
    };
    let tags = tag_breakdown(&all, cli.top);
    let mut unbounded = unbounded_tags(&all, cli.unbounded_threshold);
    if let Err(err) = add_advice(
        &mut unbounded,
        &all,
        cli.histogram_aggregates,
        cli.histogram_percentiles,
        cli.percentiles,
    ) {
        return usage_error(&err);
    }
    let cost = prices.map(|prices| {
        estimate_cost(
            prices,
            allotment.as_ref().map(|item| item.allotment),
            allow.ingested,
            allow.indexed,
        )
    });
    let rendered = if cli.json {
        render_count_json(
            paths,
            &report,
            allotment.as_ref(),
            &tags,
            &unbounded,
            Some(&allow),
            cost.as_ref(),
        )
    } else {
        render_count(
            &report,
            cli.explain,
            allotment.as_ref(),
            &tags,
            &unbounded,
            Some(&allow),
            cost.as_ref(),
        )
    };
    let written = write_report(cli.output.as_deref(), &rendered);
    if written != EXIT_OK {
        return written;
    }
    let status = budget_status(cli.budget, report.total);
    if status == EXIT_OK && cli.verbose {
        note(format_args!(
            "context: samples {}, custom metrics {}",
            all.len(),
            report.total
        ));
    }
    status
}

fn combination_rejection(cli: &Cli) -> Option<&'static str> {
    if cli.hosts.is_some() && cli.plan.is_none() {
        return Some("--hosts only works together with --plan");
    }
    if cli.untyped_as.is_some() && !matches!(cli.format, Format::Prometheus) {
        return Some("--untyped-as only works together with --format prometheus");
    }
    None
}

/// `--unbounded-threshold` defaults to 50. Only an argument set on the
/// command line counts, otherwise `diff` would refuse it even without the flag.
fn diff_forbidden_option(cli: &Cli, threshold_explicit: bool) -> Option<&'static str> {
    if cli.explain {
        return Some("--explain");
    }
    if cli.plan.is_some() {
        return Some("--plan");
    }
    if cli.hosts.is_some() {
        return Some("--hosts");
    }
    if cli.allow_tags.is_some() {
        return Some("--allow-tags");
    }
    if cli.prices.is_some() {
        return Some("--prices");
    }
    if threshold_explicit {
        return Some("--unbounded-threshold");
    }
    None
}

fn diff_paths(cli: &Cli, old: &Path, new: &Path) -> u8 {
    let old_samples = match read_samples(old, cli.format, cli.untyped_as) {
        Ok(samples) => samples,
        Err(code) => return code,
    };
    let new_samples = match read_samples(new, cli.format, cli.untyped_as) {
        Ok(samples) => samples,
        Err(code) => return code,
    };
    let report = match diff_samples(
        &old_samples,
        &new_samples,
        cli.histogram_aggregates,
        cli.histogram_percentiles,
        cli.percentiles,
        cli.top,
    ) {
        Ok(report) => report,
        Err(err) => return usage_error(&err),
    };
    let rendered = if cli.json {
        render_diff_json(old, new, &report)
    } else {
        render_diff(&report)
    };
    let written = write_report(cli.output.as_deref(), &rendered);
    if written != EXIT_OK {
        return written;
    }
    let status = budget_status(cli.budget, report.new_total);
    if status == EXIT_OK && cli.verbose {
        note(format_args!(
            "context: samples {}+{}, custom metrics {} -> {}",
            old_samples.len(),
            new_samples.len(),
            report.old_total,
            report.new_total
        ));
    }
    status
}

fn usage_error(err: &CountError) -> u8 {
    note_hint(
        format_args!("usage error: {err}"),
        "lower the option values, or run dd-metric-bill --help",
    );
    EXIT_USAGE
}

fn budget_status(budget: Option<u64>, total: u64) -> u8 {
    match budget {
        Some(limit) if total > limit => {
            note_hint(
                format_args!("budget exceeded: {total} > {limit}"),
                "reduce the number of series (see the tag breakdown), or raise --budget",
            );
            EXIT_BUDGET
        }
        _ => EXIT_OK,
    }
}

fn read_prices(path: &Path) -> Result<Prices, u8> {
    let text = read_text(path, "price file")?;
    match parse_prices(&text) {
        Ok(prices) => Ok(prices),
        Err(err) if err.is_duplicate_key() => {
            note_hint(
                format_args!("{err}"),
                "remove the repeated key from the price file; see docs/reference.md#price-file",
            );
            Err(EXIT_INPUT)
        }
        Err(err) => {
            note_hint(
                format_args!("invalid price file: {}: {err}", path.display()),
                "fix the price file; see docs/reference.md#price-file for the format",
            );
            Err(EXIT_INPUT)
        }
    }
}

fn allotment_for(
    cli: &Cli,
    samples: &[Sample],
    indexed: u64,
    ingested: Option<u64>,
) -> Result<Option<Allotment>, CountError> {
    let Some(plan) = cli.plan else {
        return Ok(None);
    };
    let hosts = cli.hosts.unwrap_or_else(|| unique_hosts(samples));
    Ok(Some(allotment(count_plan(plan), hosts, indexed, ingested)?))
}

fn count_plan(plan: Plan) -> CountPlan {
    match plan {
        Plan::Pro => CountPlan::Pro,
        Plan::Enterprise => CountPlan::Enterprise,
    }
}

fn read_samples(
    path: &Path,
    format: Format,
    untyped_as: Option<UntypedAs>,
) -> Result<Vec<Sample>, u8> {
    let text = if path == Path::new("-") {
        read_stdin_text("sample")?
    } else {
        read_text(path, "sample")?
    };
    match parse_format(format, &text) {
        Ok(mut samples) => {
            if let Some(chosen) = untyped_as {
                apply_untyped(&mut samples, chosen);
            }
            Ok(samples)
        }
        Err(err) => {
            note_hint(format_args!("{err}"), parse_hint(format));
            Err(err.exit_code())
        }
    }
}

fn apply_untyped(samples: &mut [Sample], chosen: UntypedAs) {
    let kind = match chosen {
        UntypedAs::Gauge => Kind::Gauge,
        UntypedAs::Count => Kind::Count,
    };
    for sample in samples.iter_mut() {
        if sample.kind == Kind::Untyped {
            sample.kind = kind;
        }
    }
}

/// Reads text from a regular file.
///
/// The path is opened without waiting, and only that opened file is read,
/// after its type is checked. A symlink is followed. A directory, FIFO,
/// device, or other non-regular file is not read. The type comes from the
/// opened file, so a path that becomes a FIFO between a separate check and
/// the read does not wait for a writer. A file larger than
/// [`MAX_INPUT_BYTES`] is refused from its size and its body is not read.
fn read_text(path: &Path, kind: &str) -> Result<String, u8> {
    let file = match open_regular(path) {
        Ok(file) => file,
        Err(reason) => {
            refuse_read(kind, path, reason);
            return Err(EXIT_INPUT);
        }
    };
    if let Ok(meta) = file.metadata() {
        if meta.len() > MAX_INPUT_BYTES {
            refuse_too_large(kind, path);
            return Err(EXIT_INPUT);
        }
    }
    read_capped(file, kind, path)
}

/// Reads all of standard input as text, for the sample path `-`.
fn read_stdin_text(kind: &str) -> Result<String, u8> {
    read_capped(io::stdin().lock(), kind, Path::new("-"))
}

/// Reads at most one byte past [`MAX_INPUT_BYTES`]. A longer stream is exit 2
/// and the rest is not kept.
fn read_capped(reader: impl Read, kind: &str, path: &Path) -> Result<String, u8> {
    let mut text = String::new();
    match interrupt::StopOnSigint::new(reader)
        .take(MAX_INPUT_BYTES + 1)
        .read_to_string(&mut text)
    {
        Ok(_) if interrupt::interrupted() => Err(interrupt::EXIT_INTERRUPTED),
        Ok(_) if text.len() as u64 > MAX_INPUT_BYTES => {
            refuse_too_large(kind, path);
            Err(EXIT_INPUT)
        }
        Ok(_) => Ok(text),
        Err(_) if interrupt::interrupted() => Err(interrupt::EXIT_INTERRUPTED),
        Err(err) => {
            refuse_read(kind, path, err);
            Err(EXIT_INPUT)
        }
    }
}

fn refuse_too_large(kind: &str, path: &Path) {
    let hint = format!("split the input so each file is at most {MAX_INPUT_BYTES} bytes");
    note_hint(
        format_args!(
            "cannot read {kind}: {}: file is larger than {MAX_INPUT_BYTES} bytes",
            path.display()
        ),
        &hint,
    );
}

enum ReadFail {
    NotRegular,
    Io(std::io::Error),
}

impl std::fmt::Display for ReadFail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadFail::NotRegular => f.write_str("not a regular file"),
            ReadFail::Io(err) => write!(f, "{err}"),
        }
    }
}

/// Opens `path` for reading when the opened file is a regular file.
///
/// On Unix the open does not block: a FIFO returns at once, and the check
/// uses the opened file rather than a second lookup of the path.
#[cfg(unix)]
fn open_regular(path: &Path) -> Result<File, ReadFail> {
    use std::fs::OpenOptions;
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = OpenOptions::new();
    options.read(true);
    options.custom_flags(O_NONBLOCK);
    match options.open(path) {
        Ok(file) => {
            let meta = file.metadata().map_err(ReadFail::Io)?;
            if meta.is_file() {
                Ok(file)
            } else {
                Err(ReadFail::NotRegular)
            }
        }
        // A directory or a socket fails at open. Keep the same message as a
        // FIFO: the path exists and is not a regular file.
        Err(err) => match std::fs::metadata(path) {
            Ok(meta) if !meta.is_file() => Err(ReadFail::NotRegular),
            _ => Err(ReadFail::Io(err)),
        },
    }
}

/// `O_NONBLOCK`. Linux and Android use octal 04000; the BSDs and macOS use `0x4`.
#[cfg(any(target_os = "linux", target_os = "android"))]
const O_NONBLOCK: i32 = 0o4000;
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
))]
const O_NONBLOCK: i32 = 0x4;

#[cfg(not(unix))]
fn open_regular(path: &Path) -> Result<File, ReadFail> {
    match std::fs::metadata(path) {
        Ok(meta) if !meta.is_file() => Err(ReadFail::NotRegular),
        Ok(_) => File::open(path).map_err(ReadFail::Io),
        Err(err) => Err(ReadFail::Io(err)),
    }
}

fn refuse_read(kind: &str, path: &Path, reason: impl std::fmt::Display) {
    note_hint(
        format_args!("cannot read {kind}: {}: {reason}", path.display()),
        "check that the path exists, is a regular file you may read, or use - for standard input",
    );
}

/// What the user can do about a line that does not parse.
fn parse_hint(format: Format) -> &'static str {
    match format {
        Format::Dogstatsd => {
            "fix that line (name:value|type|#tag:value), or pass --format jsonl or --format prometheus if the sample is in that format"
        }
        Format::Jsonl => {
            "fix that line (one JSON object with metric, type and tags), or drop --format jsonl if the sample is DogStatsD"
        }
        Format::Prometheus => {
            "fix that line (Prometheus text format), or drop --format prometheus if the sample is DogStatsD"
        }
    }
}

fn parse_format(format: Format, text: &str) -> Result<Vec<Sample>, ParseError> {
    match format {
        Format::Dogstatsd => parse_dogstatsd(text),
        Format::Jsonl => parse_jsonl(text),
        Format::Prometheus => parse_prometheus(text),
    }
}
