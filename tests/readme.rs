//! Documentation tests: README.md, README.nl.md, docs/reference.md and
//! docs/reference.nl.md.
//!
//! Every ```console example is run against the binary. Every exit-3 row in the
//! usage-error tables is run against the binary. The English and Dutch pages
//! must have the same sections, the same source URLs, and the same examples.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const README: &str = "README.md";
const README_NL: &str = "README.nl.md";
const REF: &str = "docs/reference.md";
const REF_NL: &str = "docs/reference.nl.md";

const PRICE_LIMIT: &str = "A price has at most 12 digits after the decimal point (`MAX_SCALE`) and a numerator of at most 10^18 (`MAX_NUMERATOR`). Trailing zeros in the fractional part are dropped before that limit. A price outside it makes the price file invalid (exit 2, message \"field is too large\").";

const QUOTE_UTF8: &str = "Metric names MAY use any UTF-8 characters.";
const QUOTE_REGEX: &str = "Metric names SHOULD match the regex `[a-zA-Z_:][a-zA-Z0-9_:]*` for the best experience and compatibility (see the warning below). Metric names outside of that set will require quoting e.g. when used in PromQL (see the UTF-8 guide).";

const B3_TOTAL: &str = "counter names ending in `_total` must be specified without the `_total` suffix. For example, to collect `promhttp_metric_handler_requests_total`, specify the metric name `promhttp_metric_handler_requests`. This submits to Datadog the metric name appended with `.count`, `promhttp_metric_handler_requests.count`.";

const LABEL_GRAMMAR: &str = "label_pairs = label_pair { \",\" label_pair } [ \",\" ]";

fn bin() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"));
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"));
    cmd
}

fn read(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {rel}: {err}"))
}

/// The text under a level-2 heading, up to the next level-2 heading.
fn section<'a>(text: &'a str, heading: &str) -> &'a str {
    let marker = format!("{heading}\n");
    let start = text
        .find(&marker)
        .unwrap_or_else(|| panic!("missing heading {heading}"))
        + marker.len();
    let rest = &text[start..];
    match rest.find("\n## ") {
        Some(end) => &rest[..end],
        None => rest,
    }
}

fn headings(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| line.starts_with("## "))
        .collect()
}

fn urls(text: &str) -> BTreeSet<String> {
    text.split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .filter(|word| word.starts_with("https://"))
        .map(|word| word.trim_end_matches(['.', ',', '"']).to_string())
        .collect()
}

fn backtick(cell: &str) -> &str {
    let cell = cell.trim();
    cell.strip_prefix('`')
        .and_then(|rest| rest.strip_suffix('`'))
        .unwrap_or_else(|| panic!("cell not in backticks: {cell}"))
}

/// `\n` in a table cell is a real newline. The cell itself has none.
fn message_of(cell: &str) -> String {
    let mut text = backtick(cell).replace("\\n", "\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

struct Row {
    call: String,
    args: Vec<String>,
    message: String,
}

fn exit3_rows(doc: &str, heading: &str) -> Vec<Row> {
    let part = section(doc, heading);
    let mut rows = Vec::new();
    for line in part.lines().filter(|line| line.starts_with('|')) {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() < 8 {
            continue;
        }
        let cells: Vec<&str> = parts[1..7].iter().map(|cell| cell.trim()).collect();
        if cells[4] != "3" {
            continue;
        }
        let call = backtick(cells[3]).to_string();
        let args = call.split_whitespace().map(str::to_string).collect();
        rows.push(Row {
            call,
            args,
            message: message_of(cells[5]),
        });
    }
    assert!(!rows.is_empty(), "{heading} has no exit-3 row");
    rows
}

/// Every ```console block: the first line is `$ dd-metric-bill ARGS`, the rest is stdout.
fn examples(doc: &str) -> Vec<(Vec<String>, String)> {
    let mut out = Vec::new();
    let mut lines = doc.lines();
    while let Some(line) = lines.next() {
        if line != "```console" {
            continue;
        }
        let call = lines.next().expect("empty console block");
        let args: Vec<String> = call
            .strip_prefix("$ dd-metric-bill ")
            .unwrap_or_else(|| {
                panic!("console block does not start with `$ dd-metric-bill `: {call}")
            })
            .split_whitespace()
            .map(str::to_string)
            .collect();
        let mut body = String::new();
        let mut closed = false;
        for line in lines.by_ref() {
            if line == "```" {
                closed = true;
                break;
            }
            body.push_str(line);
            body.push('\n');
        }
        assert!(closed, "console block not closed: {call}");
        out.push((args, body));
    }
    out
}

#[test]
fn readme_has_the_sections() {
    let readme = read(README);
    for heading in [
        "## Why",
        "## Installation",
        "## System requirements",
        "## Quick start",
        "## How it counts",
        "## Compared with alternatives",
        "## Limitations",
        "## Support",
        "## License",
        "## Contact",
    ] {
        assert!(
            readme.lines().any(|line| line == heading),
            "missing {heading}"
        );
    }
    let limits = section(&readme, "## Limitations");
    for part in [
        "Metric Name pricing",
        "hourly average",
        "no network",
        "per metric",
        "paid hosts",
        "Latest Version",
        "B2 (Mapping Prometheus Metrics to Datadog Metrics",
        "B3 (Datadog, OpenMetrics, section Missing untyped metrics",
        "By default, the integration skips metrics that come without a type on a Prometheus exposition.",
        "not verified",
        "Only regular files are read",
        "There is no `--show-values` flag.",
        PRICE_LIMIT,
    ] {
        assert!(limits.contains(part), "Limitations is missing {part}");
    }
    assert!(section(&readme, "## License").contains("PEPAI"));
    let contact = section(&readme, "## Contact");
    assert!(contact.contains("pep_ai@icloud.com"));
    assert!(contact.contains("(https://github.com/qwpp7g79y4-maker/dd-metric-bill/issues)"));
    assert!(contact.contains("[SECURITY.md](SECURITY.md)"));
    let readme_nl = read(README_NL);
    assert!(section(&readme_nl, "## Contact").contains("[SECURITY.md](SECURITY.md)"));
    assert!(section(&readme, "## Support").contains("(https://ko-fi.com/pepai)"));
    assert!(section(&readme_nl, "## Steunen").contains("(https://ko-fi.com/pepai)"));
    assert_eq!(read(".github/FUNDING.yml"), "ko_fi: pepai\n");
    let install = section(&readme, "## Installation");
    let install_nl = section(&readme_nl, "## Installatie");
    for (text, part) in [(&install, "EN"), (&install_nl, "NL")] {
        let crates_io = text
            .find("cargo install --locked dd-metric-bill")
            .expect("crates.io install line");
        let releases = text
            .find("(https://github.com/qwpp7g79y4-maker/dd-metric-bill/releases)")
            .expect("releases line");
        let from_git = text
            .find("cargo install --locked --git https://github.com/qwpp7g79y4-maker/dd-metric-bill")
            .expect("git install line");
        assert!(
            crates_io < releases && crates_io < from_git,
            "{part}: crates.io is the first install option"
        );
    }
    assert!(install.contains("Rust 1.85 or newer"));
    let badge = "[![crates.io](https://img.shields.io/crates/v/dd-metric-bill.svg)](https://crates.io/crates/dd-metric-bill)";
    assert!(readme.contains(badge), "README misses the crates.io badge");
    assert!(
        readme_nl.contains(badge),
        "README.nl.md misses the crates.io badge"
    );
    assert!(section(&readme_nl, "## Installatie").contains("Rust 1.85 of nieuwer"));
    assert!(read("Cargo.toml").contains("rust-version = \"1.85\""));
    let requirements = section(&readme, "## System requirements");
    let requirements_nl = section(&readme_nl, "## Systeemvereisten");
    let release = read(".github/workflows/release.yml");
    for target in [
        "x86_64-unknown-linux-gnu",
        "x86_64-unknown-linux-musl",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-pc-windows-msvc",
    ] {
        assert!(requirements.contains(target), "requirements miss {target}");
        assert!(
            requirements_nl.contains(target),
            "NL requirements miss {target}"
        );
        assert!(
            release.contains(&format!("target: {target}")),
            "release.yml misses {target}"
        );
    }
    assert!(requirements.contains("glibc 2.34") && release.contains("GLIBC_2.34"));
    assert!(requirements.contains("macOS 10.12") && release.contains("macos_min: \"10.12\""));
    assert!(requirements.contains("macOS 11") && release.contains("macos_min: \"11.0\""));
    let security = read("SECURITY.md");
    assert!(security.contains("pep_ai@icloud.com"));
    assert!(security.contains("| 0.1.x | Yes |"));
    assert!(readme.contains("nicholas-ruest/series-budget"));
    assert!(!readme.contains("--show-values` exists"));
}

#[test]
fn language_switch_on_top_of_every_page() {
    for (rel, line) in [
        (README, "English · [Nederlands](README.nl.md)"),
        (README_NL, "[English](README.md) · Nederlands"),
        (REF, "English · [Nederlands](reference.nl.md)"),
        (REF_NL, "[English](reference.md) · Nederlands"),
    ] {
        let text = read(rel);
        let third = text.lines().nth(2).unwrap_or_default();
        assert_eq!(third, line, "{rel} has no language switch on line 3");
    }
}

#[test]
fn english_and_dutch_pages_match() {
    for (en, nl) in [(README, README_NL), (REF, REF_NL)] {
        let en_text = read(en);
        let nl_text = read(nl);
        assert_eq!(
            headings(&en_text).len(),
            headings(&nl_text).len(),
            "{en} and {nl} have a different number of sections"
        );
        assert_eq!(
            urls(&en_text),
            urls(&nl_text),
            "{en} and {nl} cite different URLs"
        );
        assert_eq!(
            examples(&en_text),
            examples(&nl_text),
            "{en} and {nl} have different examples"
        );
    }
}

#[test]
fn licenses_and_manifest_name_pepai() {
    assert!(read("LICENSE-MIT").contains("Copyright (c) 2026 PEPAI"));
    assert!(read("LICENSE-APACHE").contains("Copyright 2026 PEPAI"));
    let cargo = read("Cargo.toml");
    assert!(cargo.contains("PEPAI <pep_ai@icloud.com>"), "{cargo}");
    assert!(
        cargo.contains("Estimate how many billable Datadog custom metrics a sample produces."),
        "{cargo}"
    );
}

#[test]
fn sources_are_quoted_verbatim() {
    let readme = read(README);
    let how = section(&readme, "## How it counts");
    assert!(how.contains("https://docs.datadoghq.com/account_management/billing/custom_metrics"));
    assert!(how.contains("Reordering tag values doesn't add uniqueness"));
    assert!(how.contains("read 7 October 2026"));
    let nl = read(README_NL);
    let how_nl = section(&nl, "## Hoe het rekent");
    assert!(how_nl.contains("Reordering tag values doesn't add uniqueness"));
    assert!(how_nl.contains("gelezen 7 oktober 2026"));

    let reference = read(REF);
    let reference_nl = read(REF_NL);
    for part in [
        QUOTE_UTF8,
        QUOTE_REGEX,
        "https://prometheus.io/docs/concepts/data_model/",
        "9 October 2026",
        "without quotes",
        "request.Latency",
        B3_TOTAL,
        "not verified",
        LABEL_GRAMMAR,
        "https://github.com/prometheus/docs/blob/main/docs/instrumenting/exposition_formats.md",
        "8 October 2026",
        "a{host = \"x\"} 1",
        "milder than the grammar, never stricter",
        "zmij",
        "serde_json",
        "1.0.151",
        "1.0.23",
        "David Tolnay",
        "no network, file, or process access",
        "There is no `--show-values` flag.",
        PRICE_LIMIT,
        "MAX_SCALE",
        "MAX_NUMERATOR",
    ] {
        assert!(reference.contains(part), "reference is missing {part}");
    }
    assert!(reference_nl.contains(B3_TOTAL));
    assert!(reference_nl.contains(QUOTE_UTF8));
    assert!(reference_nl.contains(LABEL_GRAMMAR));
    assert!(reference_nl.contains("niet geverifieerd"));
    assert!(reference_nl.contains("gelezen 8 oktober 2026"));
    assert!(reference_nl.contains("gelezen 9 oktober 2026"));
}

#[test]
fn examples_match_the_binary() {
    for rel in [README, README_NL] {
        let doc = read(rel);
        let mut count = 0;
        for (args, expected) in examples(&doc) {
            count += 1;
            let output = bin().args(&args).output().expect("start binary");
            let out = String::from_utf8(output.stdout).expect("stdout is utf-8");
            let err = String::from_utf8(output.stderr).expect("stderr is utf-8");
            assert_eq!(
                output.status.code(),
                Some(0),
                "{rel} {args:?}\nstderr: {err}"
            );
            assert!(err.is_empty(), "{rel} {args:?} stderr: {err}");
            assert_eq!(out, expected, "{rel} {args:?}");
        }
        assert!(count >= 4, "{rel} has only {count} examples");
    }
}

#[test]
fn price_example_is_the_file() {
    let fixture = read("prices/datadog-example.json");
    let block = format!("```json\n{fixture}");
    for rel in [REF, REF_NL] {
        assert!(
            read(rel).contains(block.trim_end()),
            "{rel}: price example differs from prices/datadog-example.json"
        );
    }
}

#[test]
fn option_and_error_tables_match_the_binary() {
    let help = bin().arg("--help").output().expect("start binary");
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).expect("help is utf-8");
    for (rel, options, errors) in [
        (REF, "## Options", "## Exit codes and usage errors"),
        (REF_NL, "## Opties", "## Exitcodes en gebruiksfouten"),
    ] {
        let doc = read(rel);
        let table = section(&doc, options);
        for line in help.lines() {
            let Some(start) = line.find("--") else {
                continue;
            };
            let name = line[start..].split_whitespace().next().unwrap();
            assert!(
                table.contains(name),
                "{rel}: option table is missing {name}"
            );
        }
        let mut messages = BTreeSet::new();
        let mut calls = BTreeSet::new();
        for row in exit3_rows(&doc, errors) {
            let output = bin().args(&row.args).output().expect("start binary");
            let out = String::from_utf8(output.stdout).expect("stdout is utf-8");
            let err = String::from_utf8(output.stderr).expect("stderr is utf-8");
            assert_eq!(
                output.status.code(),
                Some(3),
                "{rel} {:?} stderr: {err}",
                row.args
            );
            assert!(out.is_empty(), "{rel} {:?} stdout: {out}", row.args);
            assert_eq!(err, row.message, "{rel} {:?}", row.args);
            messages.insert(row.message);
            calls.insert(row.call);
        }
        for needed in [
            "--hosts only works together with --plan\n",
            "--untyped-as only works together with --format prometheus\n",
            "option does not apply to diff: --explain\n",
            "option does not apply to diff: --prices\n",
            "option does not apply to diff: --unbounded-threshold\n",
        ] {
            assert!(
                messages.contains(needed),
                "{rel}: table is missing {needed:?}"
            );
        }
        for call in [
            "diff --explain tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd",
            "diff --json --explain tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd",
            "diff --prices prices/datadog-example.json tests/fixtures/diff-old.dogstatsd tests/fixtures/diff-new.dogstatsd",
            "count --hosts 1 tests/fixtures/empty.dogstatsd",
            "count --format nope tests/fixtures/empty.dogstatsd",
        ] {
            assert!(calls.contains(call), "{rel}: table is missing {call}");
        }
    }
}

#[test]
fn release_archive_ships_quickstart_fixtures() {
    let release = read(".github/workflows/release.yml");
    let readme = read(README);
    let quick = section(&readme, "## Quick start");
    let mut paths = Vec::new();
    for (args, _) in examples(quick) {
        for arg in &args {
            if arg.starts_with("tests/fixtures/") {
                paths.push(arg.clone());
            }
        }
    }
    assert!(paths.len() >= 5, "{paths:?}");
    for path in &paths {
        assert!(release.contains(path), "release.yml does not ship {path}");
    }
    assert!(
        release.contains("count tests/fixtures/b1-count.dogstatsd"),
        "the archive smoke test does not run the quickstart"
    );
}

#[test]
fn fixture_readme_describes_every_file_as_synthetic() {
    let text = read("tests/fixtures/README.md");
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut count = 0;
    for entry in fs::read_dir(&dir).expect("fixtures") {
        let name = entry
            .expect("entry")
            .file_name()
            .into_string()
            .expect("utf-8");
        if name == "README.md" {
            continue;
        }
        count += 1;
        let part = section(&text, &format!("## {name}"));
        assert!(
            part.to_lowercase().contains("synthetic"),
            "{name} is not described as synthetic"
        );
        assert!(
            part.trim().chars().count() >= 40,
            "{name} has too short a description"
        );
    }
    assert!(count > 0);
    let intro = text.to_lowercase();
    assert!(
        intro.contains("written by hand"),
        "fixture README does not say the files were written by hand"
    );
}

#[test]
fn reference_version_matches_the_package() {
    let expected = format!("dd-metric-bill {}", env!("CARGO_PKG_VERSION"));
    for rel in [REF, REF_NL] {
        assert_eq!(version_example(&read(rel)), expected, "{rel}");
    }
}

fn version_example(text: &str) -> String {
    let marker = "dd-metric-bill ";
    let mut rest = text;
    while let Some(index) = rest.find(marker) {
        let after = &rest[index + marker.len()..];
        let token = after
            .split(|c: char| !c.is_ascii_digit() && c != '.')
            .next()
            .unwrap_or("");
        if token.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return format!("dd-metric-bill {token}");
        }
        rest = &rest[index + marker.len()..];
    }
    panic!("missing version example");
}

#[test]
fn reference_states_nan_inf_per_format() {
    let en = read(REF);
    let nl = read(REF_NL);
    assert!(en.contains("including `NaN`, `Inf`, and `inf`, is invalid (exit 2)"));
    assert!(en.contains("`NaN`, `Inf`, and `inf` are not JSON, so the line is invalid (exit 2)"));
    assert!(en.contains(
        "`NaN`, `+Inf`, `-Inf`, `Inf`, and `inf` are accepted. The series counts; the value is not part of the bill."
    ));
    assert!(nl.contains("inclusief `NaN`, `Inf` en `inf`, is ongeldig (exit 2)"));
    assert!(nl.contains("`NaN`, `Inf` en `inf` zijn geen JSON, dus de regel is ongeldig (exit 2)"));
    assert!(nl.contains(
        "`NaN`, `+Inf`, `-Inf`, `Inf` en `inf` worden geaccepteerd. De serie telt; de waarde hoort niet bij de rekening."
    ));
}

#[test]
fn readme_and_reference_describe_one_host_per_line() {
    for rel in [README, README_NL, REF, REF_NL] {
        let text = read(rel);
        assert!(
            text.contains("host has more than one value"),
            "{rel} does not describe the host conflict"
        );
        assert!(text.contains("host:a,host:a"), "{rel}");
        assert!(text.contains("m:1|c|#host:a,host:b"), "{rel}");
        assert!(text.contains("\"tags\":[\"host:a\",\"host:b\"]"), "{rel}");
        assert!(text.contains("{host=\"a\",host=\"b\"}"), "{rel}");
    }
}

/// Every relative link in the README pages and in docs/ is copied into the
/// release archive. `prices/datadog-example.json` is copied with that directory.
#[test]
fn release_archive_ships_linked_paths() {
    let release = read(".github/workflows/release.yml");
    let (files, trees) = cp_sources(&release);
    let mut links = Vec::new();
    for rel in doc_pages() {
        for link in markdown_links(&read(&rel)) {
            let Some(path) = link_path(&link) else {
                continue;
            };
            links.push(resolve_link(&rel, path));
        }
    }
    assert!(links.iter().any(|path| path == "SECURITY.md"), "{links:?}");
    assert!(links.len() >= 6, "{links:?}");
    for path in &links {
        let abs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
        assert!(abs.is_file(), "{path} is linked but missing");
        assert!(
            ships(&files, &trees, path),
            "release.yml does not ship {path}\nfiles: {files:?}\ntrees: {trees:?}"
        );
    }
    assert!(
        ships(&files, &trees, "prices/datadog-example.json"),
        "release.yml does not ship prices/datadog-example.json\n{files:?} {trees:?}"
    );
}

fn doc_pages() -> Vec<String> {
    let mut pages = vec!["README.md".to_string(), "README.nl.md".to_string()];
    let docs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs");
    for entry in fs::read_dir(docs).expect("docs") {
        let entry = entry.expect("docs entry");
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".md") {
            pages.push(format!("docs/{name}"));
        }
    }
    pages
}

fn markdown_links(text: &str) -> Vec<String> {
    let mut links = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b']' && bytes[index + 1] == b'(' {
            let start = index + 2;
            if let Some(end) = text[start..].find(')') {
                links.push(text[start..start + end].to_string());
                index = start + end;
                continue;
            }
        }
        index += 1;
    }
    links
}

fn link_path(link: &str) -> Option<&str> {
    let link = link.trim();
    if link.is_empty() || link.starts_with('#') || link.contains("://") {
        return None;
    }
    let path = link.split('#').next().unwrap_or(link);
    let path = path.split_whitespace().next().unwrap_or(path);
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

fn resolve_link(from_file: &str, link: &str) -> String {
    let mut stack: Vec<String> = std::path::Path::new(from_file)
        .parent()
        .unwrap_or(std::path::Path::new(""))
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    for component in std::path::Path::new(link).components() {
        match component {
            std::path::Component::Normal(part) => stack.push(part.to_string_lossy().into_owned()),
            std::path::Component::ParentDir => {
                stack.pop();
            }
            std::path::Component::CurDir => {}
            _ => {}
        }
    }
    stack.join("/")
}

fn cp_sources(release: &str) -> (Vec<String>, Vec<String>) {
    let text = release.replace("\\\n", " ");
    let mut files = Vec::new();
    let mut trees = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("cp ") else {
            continue;
        };
        let mut args: Vec<&str> = rest
            .split_whitespace()
            .map(|part| part.trim_matches('"'))
            .collect();
        if args.is_empty() {
            continue;
        }
        let recursive = args[0] == "-R" || args[0] == "-r";
        if recursive {
            args.remove(0);
        }
        if args.len() < 2 {
            continue;
        }
        args.pop();
        if recursive {
            trees.extend(args.iter().map(|part| (*part).to_string()));
        } else {
            files.extend(args.iter().map(|part| (*part).to_string()));
        }
    }
    (files, trees)
}

fn ships(files: &[String], trees: &[String], path: &str) -> bool {
    if files.iter().any(|file| file == path) {
        return true;
    }
    trees
        .iter()
        .any(|dir| path == dir || path.starts_with(&format!("{dir}/")))
}
