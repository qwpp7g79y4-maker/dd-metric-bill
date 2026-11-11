//! `--version` and `-V` print the crate version and exit 0.
//! A debug build also prints the git commit when git provides one.

use std::process::Command;

fn run(flag: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_dd-metric-bill"))
        .arg(flag)
        .env("NO_COLOR", "1")
        .env("CLICOLOR_FORCE", "0")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("spawn dd-metric-bill")
}

fn git_sha() -> Option<String> {
    let out = Command::new("git")
        .args(["-C", env!("CARGO_MANIFEST_DIR"), "rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let text = text.trim();
    if is_sha(text) {
        Some(text.to_string())
    } else {
        None
    }
}

fn is_sha(text: &str) -> bool {
    let n = text.len();
    (n == 40 || n == 64) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

#[test]
fn version_flags_print_the_crate_version() {
    let prefix = format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    let from_git = git_sha();
    let compiled = option_env!("GIT_SHA");
    if let Some(sha) = compiled {
        assert!(is_sha(sha), "compiled hash is not a git commit: {sha}");
    }
    for flag in ["--version", "-V"] {
        let out = run(flag);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(0), "{flag}\n{stdout}\n{stderr}");
        assert!(stderr.is_empty(), "{flag} stderr={stderr}");
        assert!(stdout.ends_with('\n'), "{flag} {stdout:?}");
        assert!(
            !stdout[..stdout.len() - 1].contains('\n'),
            "{flag} {stdout:?}"
        );
        let line = stdout.trim_end_matches('\n');
        let rest = line
            .strip_prefix(&prefix)
            .unwrap_or_else(|| panic!("{flag} {line}"));
        #[cfg(debug_assertions)]
        match (compiled, from_git.as_deref()) {
            (Some(sha), Some(live)) => {
                assert_eq!(sha, live, "{flag} build hash and git HEAD differ");
                assert_eq!(rest, format!(" ({sha})"), "{flag} {line}");
            }
            (None, None) => assert!(rest.is_empty(), "{flag} invented a hash: {line}"),
            (Some(sha), None) => panic!("{flag} shows {sha}, but git did not provide a commit"),
            (None, Some(live)) => panic!("{flag} omitted the commit git provided: {live}"),
        }
        #[cfg(not(debug_assertions))]
        {
            assert!(rest.is_empty(), "{flag} release build shows extra: {line}");
            let _ = (compiled, &from_git);
        }
    }
}
