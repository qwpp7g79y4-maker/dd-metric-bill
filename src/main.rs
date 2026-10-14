use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
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
}

#[derive(Subcommand)]
enum Command {
    /// Count billable custom metrics in one or more samples.
    #[command(after_help = AFTER_HELP)]
    Count {
        /// Sample file.
        #[arg(value_name = "SAMPLE", required = true)]
        samples: Vec<PathBuf>,
    },
    /// Compare two samples.
    #[command(after_help = AFTER_HELP)]
    Diff {
        /// Old sample.
        #[arg(value_name = "OLD")]
        old: PathBuf,
        /// New sample.
        #[arg(value_name = "NEW")]
        new: PathBuf,
    },
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
    let command = Cli::command();
    match command.try_get_matches() {
        Ok(matches) => {
            let threshold_explicit = matches.value_source("unbounded_threshold")
                == Some(clap::parser::ValueSource::CommandLine);
            match Cli::from_arg_matches(&matches) {
                Ok(cli) => ExitCode::from(run(&cli, threshold_explicit)),
                Err(err) => ExitCode::from(usage_exit(&err)),
            }
        }
        Err(err) => ExitCode::from(usage_exit(&err)),
    }
}

fn usage_exit(err: &clap::Error) -> u8 {
    let code = match err.kind() {
        clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => EXIT_OK,
        _ => EXIT_USAGE,
    };
    if let Err(io_err) = err.print() {
        note(format_args!("could not write the message: {io_err}"));
        return EXIT_USAGE;
    }
    code
}

/// Writes one stderr line. A full or closed stderr must not panic.
fn note(args: std::fmt::Arguments<'_>) {
    let mut err = io::stderr().lock();
    let _ = writeln!(err, "{args}");
}

/// Writes the report to stdout. A failed write is exit 2 and does not panic.
fn write_report(text: &str) -> u8 {
    let mut out = io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => EXIT_OK,
        Err(err) => {
            note(format_args!("could not write the report: {err}"));
            EXIT_INPUT
        }
    }
}

fn run(cli: &Cli, threshold_explicit: bool) -> u8 {
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
    let unbounded = unbounded_tags(&all, cli.unbounded_threshold);
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
    if write_report(&rendered) != EXIT_OK {
        return EXIT_INPUT;
    }
    budget_status(cli.budget, report.total)
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
    if write_report(&rendered) != EXIT_OK {
        return EXIT_INPUT;
    }
    budget_status(cli.budget, report.new_total)
}

fn usage_error(err: &CountError) -> u8 {
    note(format_args!("usage error: {err}"));
    EXIT_USAGE
}

fn budget_status(budget: Option<u64>, total: u64) -> u8 {
    match budget {
        Some(limit) if total > limit => {
            note(format_args!("budget exceeded: {total} > {limit}"));
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
            note(format_args!("{err}"));
            Err(EXIT_INPUT)
        }
        Err(err) => {
            note(format_args!(
                "invalid price file: {}: {err}",
                path.display()
            ));
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
    let text = read_text(path, "sample")?;
    match parse_format(format, &text) {
        Ok(mut samples) => {
            if let Some(chosen) = untyped_as {
                apply_untyped(&mut samples, chosen);
            }
            Ok(samples)
        }
        Err(err) => {
            note(format_args!("{err}"));
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
/// the read does not wait for a writer.
fn read_text(path: &Path, kind: &str) -> Result<String, u8> {
    let mut file = match open_regular(path) {
        Ok(file) => file,
        Err(reason) => {
            refuse_read(kind, path, reason);
            return Err(EXIT_INPUT);
        }
    };
    let mut text = String::new();
    match file.read_to_string(&mut text) {
        Ok(_) => Ok(text),
        Err(err) => {
            refuse_read(kind, path, err);
            Err(EXIT_INPUT)
        }
    }
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
    note(format_args!(
        "cannot read {kind}: {}: {reason}",
        path.display()
    ));
}

fn parse_format(format: Format, text: &str) -> Result<Vec<Sample>, ParseError> {
    match format {
        Format::Dogstatsd => parse_dogstatsd(text),
        Format::Jsonl => parse_jsonl(text),
        Format::Prometheus => parse_prometheus(text),
    }
}
