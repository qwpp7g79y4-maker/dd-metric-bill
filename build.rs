//! Git commit for `--version` in a debug build.
//! When git does not print a commit, nothing is recorded.

fn main() {
    println!("cargo:rustc-check-cfg=cfg(has_git_sha)");
    let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") else {
        return;
    };
    watch_head(&manifest);
    if let Some(sha) = git_sha(&manifest) {
        println!("cargo:rustc-cfg=has_git_sha");
        println!("cargo:rustc-env=GIT_SHA={sha}");
    }
}

/// Full commit from `git rev-parse HEAD`, or nothing.
fn git_sha(manifest: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["-C", manifest, "rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = std::str::from_utf8(&out.stdout).ok()?.trim();
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

/// Rebuild when HEAD or the branch ref changes. A worktree keeps those outside `.git` in this directory.
fn watch_head(manifest: &str) {
    let Some(head_path) = git_path(manifest, "HEAD") else {
        return;
    };
    println!("cargo:rerun-if-changed={}", head_path.display());
    let Ok(text) = std::fs::read_to_string(&head_path) else {
        return;
    };
    let Some(rest) = text.strip_prefix("ref: ") else {
        return;
    };
    let rel = rest.trim();
    if rel.is_empty() || rel.contains('\0') {
        return;
    }
    if let Some(ref_path) = git_path(manifest, rel) {
        println!("cargo:rerun-if-changed={}", ref_path.display());
    }
}

fn git_path(manifest: &str, name: &str) -> Option<std::path::PathBuf> {
    let out = std::process::Command::new("git")
        .args(["-C", manifest, "rev-parse", "--git-path", name])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = std::str::from_utf8(&out.stdout).ok()?.trim();
    if text.is_empty() {
        return None;
    }
    let path = std::path::PathBuf::from(text);
    if path.is_absolute() {
        Some(path)
    } else {
        Some(std::path::Path::new(manifest).join(path))
    }
}
