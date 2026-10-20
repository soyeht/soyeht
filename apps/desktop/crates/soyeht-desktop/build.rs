//! The build script of the desktop client: the commit the build id names, and on Windows the icon
//! and version resource that `packaging/windows/soyeht.rc` describes.

use std::{env, path::Path, process::Command};

/// What the build id carries when no commit is known (a source archive has no git history).
const UNKNOWN_COMMIT: &str = "unknown";
/// Digits of the commit in the build id.
const COMMIT_DIGITS: &str = "--short=9";

fn main() {
    println!("cargo:rerun-if-env-changed=SOYEHT_BUILD_COMMIT");
    let commit = match env::var("SOYEHT_BUILD_COMMIT") {
        Ok(commit) if !commit.is_empty() => commit,
        _ => git_commit().unwrap_or_else(|| UNKNOWN_COMMIT.to_owned()),
    };
    println!("cargo:rustc-env=SOYEHT_BUILD_COMMIT={commit}");
    if env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        windows_resources();
    }
}

/// The short commit of `HEAD`, and a rerun when it moves. A commit changes the branch's ref (a
/// loose file, or a line of `packed-refs` after `git pack-refs`) and appends to the reflog, and a
/// switch of branch changes `HEAD`. Cargo treats a path that does not exist as always changed, so
/// only the files that exist are named: a packed ref has no loose file, and a repository may keep
/// no reflog.
fn git_commit() -> Option<String> {
    let commit = git(&["rev-parse", COMMIT_DIGITS, "HEAD"])?;
    let mut watched = vec![
        git_path("HEAD"),
        git_path("logs/HEAD"),
        git_path("packed-refs"),
    ];
    if let Some(branch) = git(&["symbolic-ref", "--quiet", "HEAD"]) {
        watched.push(git_path(&branch));
    }
    for path in watched.into_iter().flatten() {
        if Path::new(&path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    Some(commit)
}

fn git_path(name: &str) -> Option<String> {
    git(&["rev-parse", "--git-path", name])
}

fn git(arguments: &[&str]) -> Option<String> {
    // No git, or no repository, is the ordinary case of a build from an archive.
    let Ok(output) = Command::new("git").args(arguments).output() else {
        return None;
    };
    if !output.status.success() {
        return None;
    }
    let Ok(text) = String::from_utf8(output.stdout) else {
        return None;
    };
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn windows_resources() {
    let folder = "../../packaging/windows";
    println!("cargo:rerun-if-changed={folder}/soyeht.rc");
    println!("cargo:rerun-if-changed={folder}/soyeht.ico");
    let version = ["MAJOR", "MINOR", "PATCH"].map(|part| {
        let number = env::var(format!("CARGO_PKG_VERSION_{part}")).unwrap_or_default();
        format!("VERSION_{part}={number}")
    });
    let parameters = embed_resource::ParamsMacrosAndIncludeDirs(version, [folder]);
    if let Err(failure) =
        embed_resource::compile(format!("{folder}/soyeht.rc"), parameters).manifest_required()
    {
        println!("cargo:warning={failure}");
        std::process::exit(1);
    }
}
