//! The manual page lists the same long options as the built binary's `--help`.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

fn binary() -> Command {
    let path = env!("CARGO_BIN_EXE_dd-metric-bill");
    let mut cmd = Command::new(path);
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"));
    cmd
}

fn read_repo(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {rel}: {err}"))
}

fn run(cmd: &mut Command) -> (Option<i32>, String, String) {
    let output = cmd.output().expect("binary start");
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    (output.status.code(), stdout, stderr)
}

/// Whitespace-separated tokens that start with `--`, with wrapping punctuation removed.
fn long_options(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for raw in text.split_whitespace() {
        let token = raw.trim_matches(|c: char| {
            matches!(
                c,
                ',' | '.' | ';' | ':' | '\'' | '"' | '`' | '(' | ')' | '[' | ']' | '{' | '}'
            )
        });
        if token.starts_with("--")
            && token.len() > 2
            && token
                .bytes()
                .skip(2)
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            found.insert(token.to_string());
        }
    }
    found
}

fn commands(help: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_commands = false;
    for line in help.lines() {
        if line == "Commands:" {
            in_commands = true;
            continue;
        }
        if !in_commands {
            continue;
        }
        if line.is_empty() || !(line.starts_with(' ') || line.starts_with('\t')) {
            break;
        }
        let Some(name) = line.split_whitespace().next() else {
            continue;
        };
        names.push(name.to_string());
    }
    assert!(!names.is_empty(), "no commands in --help:\n{help}");
    names
}

struct Example {
    args: Vec<String>,
    code: i32,
}

fn command_rest(line: &str) -> Option<&str> {
    let line = line.trim().strip_prefix("$ ").unwrap_or(line.trim());
    line.strip_prefix("dd-metric-bill ")
}

fn example_commands(text: &str) -> Vec<Example> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(rest) = command_rest(line) else {
            continue;
        };
        let args = rest.split_whitespace().map(str::to_string).collect();
        let mut code = None;
        for later in &lines[index + 1..] {
            if command_rest(later).is_some() {
                break;
            }
            if let Some(raw) = later.trim().strip_prefix("Exit code:") {
                code = Some(raw.trim().parse::<i32>().expect("exit code is an integer"));
                break;
            }
        }
        let code = code.unwrap_or_else(|| panic!("no Exit code after: dd-metric-bill {rest}"));
        found.push(Example { args, code });
    }
    assert!(
        !found.is_empty(),
        "examples/README.md has no dd-metric-bill command"
    );
    found
}

#[test]
fn help_long_options_match_the_manual() {
    let (code, help, err) = run(binary().arg("--help"));
    assert_eq!(code, Some(0), "stderr: {err}");
    assert!(!err.contains("panicked"), "{err}");

    let man = read_repo("man/dd-metric-bill.1");
    for heading in [
        ".SH NAME",
        ".SH SYNOPSIS",
        ".SH DESCRIPTION",
        ".SH OPTIONS",
        ".SH EXIT STATUS",
        ".SH EXAMPLES",
    ] {
        assert!(man.contains(heading), "man page is missing {heading}");
    }

    let help_opts = long_options(&help);
    let man_opts = long_options(&man);
    let only_man: Vec<_> = man_opts.difference(&help_opts).cloned().collect();
    let only_help: Vec<_> = help_opts.difference(&man_opts).cloned().collect();
    assert!(
        only_man.is_empty() && only_help.is_empty(),
        "man page has {only_man:?}\nhelp has {only_help:?}\nhelp:\n{help}"
    );

    for name in commands(&help) {
        let (code, sub, err) = run(binary().args([&name, "--help"]));
        assert_eq!(code, Some(0), "{name} --help stderr: {err}");
        let missing: Vec<_> = long_options(&sub).difference(&man_opts).cloned().collect();
        assert!(
            missing.is_empty(),
            "{name} --help options missing from the man page: {missing:?}\n{sub}"
        );
    }
}

#[test]
fn unknown_option_is_not_a_panic() {
    let (code, _stdout, err) = run(binary().arg("--not-a-real-option"));
    assert_ne!(code, Some(0), "{err}");
    assert!(
        !err.to_lowercase().contains("panicked"),
        "stderr contains panicked:\n{err}"
    );
}

#[test]
fn example_command_exits_as_documented() {
    let doc = read_repo("examples/README.md");
    for example in example_commands(&doc) {
        let (code, stdout, err) = run(binary().args(&example.args));
        assert_eq!(
            code,
            Some(example.code),
            "dd-metric-bill {}\nstdout:\n{stdout}\nstderr:\n{err}",
            example.args.join(" ")
        );
    }
}
