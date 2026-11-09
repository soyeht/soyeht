//! How a language server starts in a folder the person has not trusted (ADR 0150). The server
//! runs with the project folder as its working directory, and what it starts by name (`env node`,
//! `python3`) is found on the search path it is given. So an untrusted server gets a search path
//! of its own: only absolute entries, none inside the folder, each as the system resolves it. The
//! server, and the TypeScript it runs, must also resolve outside the folder.
use crate::config::config;
use serde_json::{Value, json};
use soyeht_protocol::EditorLanguage;
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

const TYPESCRIPT_SERVER: &str = "typescript-language-server";

/// Why a server cannot be started safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    /// The program is on no entry of the search path that is outside the folder.
    NoProgram,
    /// No whole TypeScript outside the folder was installed with the language server.
    NoTrustedTypescript,
}

/// What an untrusted server is started with.
#[derive(Debug, Clone, PartialEq)]
pub struct Restricted {
    /// The absolute path of the program.
    pub program: PathBuf,
    /// The search path the program gets, in place of the one it would inherit.
    pub path: OsString,
    /// The `initializationOptions` of its `initialize`.
    pub options: Option<Value>,
}

/// The start of the server for `language` in the untrusted folder `root`, from `search_path`.
/// TypeScript runs the `tsserver` that was installed with the language server, never one from
/// the folder, and installs no type packages (the installer runs npm); pyright runs no program of
/// the folder and needs no options.
pub fn restricted_launch(
    language: &EditorLanguage,
    search_path: &OsStr,
    root: &Path,
) -> Result<Restricted, LaunchError> {
    let command = config(language).ok_or(LaunchError::NoProgram)?.command;
    let root = resolved(root);
    let path = outside_search_path(search_path, &root).ok_or(LaunchError::NoProgram)?;
    let (program, _) = find(command, &path, &root).ok_or(LaunchError::NoProgram)?;
    let options = if *language == EditorLanguage::Typescript {
        Some(typescript_options(&path, &root)?)
    } else {
        None
    };
    Ok(Restricted {
        program,
        path,
        options,
    })
}

fn resolved(path: &Path) -> PathBuf {
    if let Ok(real) = soyeht_platform::fs::canonicalize(path) {
        real
    } else {
        path.to_path_buf()
    }
}

/// The absolute entries of `search_path` that resolve outside `root`, resolved. A relative or
/// empty entry is the working directory, which is the project folder.
fn outside_search_path(search_path: &OsStr, root: &Path) -> Option<OsString> {
    let mut kept: Vec<PathBuf> = Vec::new();
    for directory in std::env::split_paths(search_path) {
        if !directory.is_absolute() {
            continue;
        }
        let Ok(real) = soyeht_platform::fs::canonicalize(&directory) else {
            continue;
        };
        if !real.starts_with(root) && !kept.contains(&real) {
            kept.push(real);
        }
    }
    if kept.is_empty() {
        return None;
    }
    let Ok(joined) = std::env::join_paths(kept) else {
        return None;
    };
    Some(joined)
}

/// The first `command` on `path` that is a file and resolves outside `root`: where it is found,
/// and what it resolves to.
fn find(command: &str, path: &OsStr, root: &Path) -> Option<(PathBuf, PathBuf)> {
    for directory in std::env::split_paths(path) {
        let candidate = directory.join(command);
        if !candidate.is_file() {
            continue;
        }
        let Ok(real) = soyeht_platform::fs::canonicalize(&candidate) else {
            continue;
        };
        if !real.starts_with(root) {
            return Some((candidate, real));
        }
    }
    None
}

fn typescript_options(path: &OsStr, root: &Path) -> Result<Value, LaunchError> {
    let tsserver = installed_tsserver(path, root).ok_or(LaunchError::NoTrustedTypescript)?;
    Ok(json!({
        "tsserver":{"path":tsserver.to_string_lossy(),"logVerbosity":"off"},
        "disableAutomaticTypingAcquisition":true,
        "plugins":[]
    }))
}

/// The `tsserver.js` that sits with the `typescript-language-server` found on `path`: in the
/// `node_modules` that holds the server (where npm puts its dependencies) or in its own. The
/// `typescript` package must be whole (a readable `package.json` with a version), because a server
/// that cannot read its version falls back to the folder's own TypeScript, and must resolve
/// outside the folder.
fn installed_tsserver(path: &OsStr, root: &Path) -> Option<PathBuf> {
    let (_, program) = find(TYPESCRIPT_SERVER, path, root)?;
    for ancestor in program.ancestors().skip(1) {
        if ancestor
            .file_name()
            .is_some_and(|name| name == TYPESCRIPT_SERVER)
        {
            let own = ancestor.join("node_modules");
            let shared = ancestor.parent().map(Path::to_path_buf);
            for modules in std::iter::once(own).chain(shared) {
                let package = modules.join("typescript");
                let tsserver = package.join("lib/tsserver.js");
                let Ok(real) = soyeht_platform::fs::canonicalize(&tsserver) else {
                    continue;
                };
                if !real.starts_with(root) && has_version(&package.join("package.json")) {
                    return Some(real);
                }
            }
        }
    }
    None
}

fn has_version(package_json: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(package_json) else {
        return false;
    };
    let Ok(package) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    package["version"].as_str().is_some_and(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    mod unix {
        use super::super::*;
        use std::{fs, os::unix::fs::PermissionsExt};

        struct Tree(PathBuf);

        impl Tree {
            fn new(name: &str) -> Self {
                let root = std::env::temp_dir()
                    .join(format!("sn-restricted-{name}-{}", std::process::id()));
                let _ = fs::remove_dir_all(&root);
                fs::create_dir_all(&root).unwrap();
                Self(soyeht_platform::fs::canonicalize(&root).unwrap())
            }

            fn dir(&self, relative: &str) -> PathBuf {
                let path = self.0.join(relative);
                fs::create_dir_all(&path).unwrap();
                path
            }

            fn script(&self, relative: &str, body: &str) -> PathBuf {
                let path = self.0.join(relative);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
                path
            }
        }

        impl Drop for Tree {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        fn joined(entries: &[&Path]) -> OsString {
            std::env::join_paths(entries).unwrap()
        }

        #[test]
        fn the_search_path_keeps_only_absolute_entries_outside_the_folder() {
            let tree = Tree::new("path");
            let project = tree.dir("project");
            let inside = tree.dir("project/node_modules/.bin");
            let outside = tree.dir("tools");
            std::os::unix::fs::symlink(&inside, tree.0.join("linked")).unwrap();
            let entries = [
                Path::new(""),
                Path::new("bin"),
                Path::new("."),
                inside.as_path(),
                &tree.0.join("linked"),
                &tree.0.join("missing"),
                outside.as_path(),
            ];
            let kept = outside_search_path(&joined(&entries), &project).unwrap();
            assert_eq!(kept, joined(&[outside.as_path()]));
            assert_eq!(
                outside_search_path(&joined(&[inside.as_path()]), &project),
                None
            );
        }

        #[test]
        fn a_program_of_the_folder_is_never_the_server_even_through_a_link() {
            let tree = Tree::new("program");
            let project = tree.dir("project");
            let inside = tree.dir("project/node_modules/.bin");
            tree.script("project/node_modules/.bin/pyright-langserver", "exit 0");
            let outside = tree.dir("tools");
            std::os::unix::fs::symlink(
                inside.join("pyright-langserver"),
                outside.join("pyright-langserver"),
            )
            .unwrap();
            let path = joined(&[inside.as_path(), outside.as_path()]);
            assert_eq!(
                restricted_launch(&EditorLanguage::Python, &path, &project),
                Err(LaunchError::NoProgram)
            );

            tree.script("tools2/pyright-langserver", "exit 0");
            let path = joined(&[inside.as_path(), &tree.0.join("tools2")]);
            let launch = restricted_launch(&EditorLanguage::Python, &path, &project).unwrap();
            assert_eq!(launch.program, tree.0.join("tools2/pyright-langserver"));
            assert_eq!(launch.path, joined(&[&tree.0.join("tools2")]));
            assert_eq!(launch.options, None);
        }

        fn typescript_install(tree: &Tree, base: &str) -> PathBuf {
            let modules = tree.dir(&format!("{base}/lib/node_modules"));
            tree.script(
                &format!("{base}/lib/node_modules/typescript-language-server/lib/cli.mjs"),
                "exit 0",
            );
            tree.dir(&format!("{base}/lib/node_modules/typescript/lib"));
            fs::write(modules.join("typescript/lib/tsserver.js"), "").unwrap();
            tree.dir(&format!("{base}/bin"));
            std::os::unix::fs::symlink(
                modules.join("typescript-language-server/lib/cli.mjs"),
                tree.0
                    .join(format!("{base}/bin/typescript-language-server")),
            )
            .unwrap();
            tree.0.join(format!("{base}/bin"))
        }

        #[test]
        fn typescript_runs_the_installed_tsserver_when_the_package_is_whole() {
            let tree = Tree::new("typescript");
            let project = tree.dir("project");
            let bin = typescript_install(&tree, "npm");
            let path = joined(&[bin.as_path()]);
            // A partial install has no readable package.json: the server would fall back to the
            // folder's own TypeScript.
            assert_eq!(
                restricted_launch(&EditorLanguage::Typescript, &path, &project),
                Err(LaunchError::NoTrustedTypescript)
            );
            fs::write(
                tree.0.join("npm/lib/node_modules/typescript/package.json"),
                "{\"version\":\"5.9.2\"}",
            )
            .unwrap();
            let launch = restricted_launch(&EditorLanguage::Typescript, &path, &project).unwrap();
            let tsserver = soyeht_platform::fs::canonicalize(
                &tree
                    .0
                    .join("npm/lib/node_modules/typescript/lib/tsserver.js"),
            )
            .unwrap();
            let options = launch.options.unwrap();
            assert_eq!(options["tsserver"]["path"], tsserver.to_str().unwrap());
            assert_eq!(options["disableAutomaticTypingAcquisition"], true);
            assert_eq!(options["plugins"], json!([]));
        }

        #[test]
        fn a_typescript_install_inside_the_folder_is_not_trusted() {
            let tree = Tree::new("repository");
            let project = tree.dir("project");
            // The folder's own server and TypeScript, on an absolute entry such as direnv adds.
            let bin = typescript_install(&tree, "project/vendored");
            fs::write(
                tree.0
                    .join("project/vendored/lib/node_modules/typescript/package.json"),
                "{\"version\":\"5.9.2\"}",
            )
            .unwrap();
            let path = joined(&[bin.as_path()]);
            assert_eq!(
                restricted_launch(&EditorLanguage::Typescript, &path, &project),
                Err(LaunchError::NoProgram)
            );
            // Outside the folder, the server is fine but its TypeScript is the folder's: refused.
            let outside = typescript_install(&tree, "npm");
            fs::remove_dir_all(tree.0.join("npm/lib/node_modules/typescript")).unwrap();
            std::os::unix::fs::symlink(
                tree.0.join("project/vendored/lib/node_modules/typescript"),
                tree.0.join("npm/lib/node_modules/typescript"),
            )
            .unwrap();
            assert_eq!(
                restricted_launch(
                    &EditorLanguage::Typescript,
                    &joined(&[outside.as_path()]),
                    &project
                ),
                Err(LaunchError::NoTrustedTypescript)
            );
        }

        #[test]
        fn a_script_started_with_the_restricted_path_never_finds_an_interpreter_in_the_folder() {
            let tree = Tree::new("shebang");
            let project = tree.dir("project");
            // The folder's own interpreter and the program that needs one, as `env node` does.
            let in_folder = tree.0.join("project-ran");
            let elsewhere = tree.0.join("trusted-ran");
            tree.script(
                "project/fakeinterp",
                &format!("/usr/bin/touch '{}'", in_folder.display()),
            );
            tree.script(
                "tools/fakeinterp",
                &format!("/usr/bin/touch '{}'", elsewhere.display()),
            );
            let launcher = tree.script("bin/launcher", "exit 0");
            fs::write(&launcher, "#!/usr/bin/env fakeinterp\n").unwrap();
            // An empty entry in the middle: the working directory, which is the folder.
            let raw = joined(&[
                tree.0.join("bin").as_path(),
                Path::new(""),
                tree.0.join("tools").as_path(),
            ]);

            let run = |path: &OsStr| {
                let status = std::process::Command::new(&launcher)
                    .current_dir(&project)
                    .env_clear()
                    .env("PATH", path)
                    .status()
                    .unwrap();
                assert!(status.success());
            };
            // The hole: with the inherited path the folder's interpreter runs.
            run(&raw);
            assert!(in_folder.exists() && !elsewhere.exists());
            fs::remove_file(&in_folder).unwrap();

            // The restricted path does not have it.
            fs::write(tree.0.join("bin/pyright-langserver"), "").unwrap();
            let launch = restricted_launch(&EditorLanguage::Python, &raw, &project).unwrap();
            run(&launch.path);
            assert!(!in_folder.exists() && elsewhere.exists());
        }
    }
}
