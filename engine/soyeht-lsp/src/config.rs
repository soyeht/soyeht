use serde_json::{Value, json};
use soyeht_protocol::EditorLanguage;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageConfig {
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub language_id: &'static str,
}

pub fn config(language: &EditorLanguage) -> Option<LanguageConfig> {
    match language {
        EditorLanguage::Rust => Some(LanguageConfig {
            command: "rust-analyzer",
            args: &[],
            language_id: "rust",
        }),
        EditorLanguage::Swift => Some(LanguageConfig {
            command: "sourcekit-lsp",
            args: &[],
            language_id: "swift",
        }),
        EditorLanguage::Typescript => Some(LanguageConfig {
            command: "typescript-language-server",
            args: &["--stdio"],
            language_id: "typescript",
        }),
        EditorLanguage::Python => Some(LanguageConfig {
            command: "pyright-langserver",
            args: &["--stdio"],
            language_id: "python",
        }),
        EditorLanguage::PlainText
        | EditorLanguage::Json
        | EditorLanguage::Markdown
        | EditorLanguage::Shell => None,
    }
}

/// Why a language server does not start while the person has not trusted the folder, or `None`
/// when it starts in a safe mode (ADR 0150). A server is here only when nothing in the folder can
/// make it run a program of the folder, with the options of [`initialization_options`]; the
/// evidence for each is in contract 18.
pub fn refusal(language: &EditorLanguage, trusted: bool) -> Option<&'static str> {
    if trusted {
        return None;
    }
    match language {
        EditorLanguage::Rust => Some(
            "rust-analyzer is off in a restricted folder: cargo and rustup run programs that the folder's own files name. Trust the folder to turn it on.",
        ),
        EditorLanguage::Swift => Some(
            "sourcekit-lsp is off in a restricted folder: it builds Package.swift and starts the build servers the folder names. Trust the folder to turn it on.",
        ),
        EditorLanguage::Python if cfg!(windows) => Some(
            "pyright is off in a restricted folder on Windows: the system looks for python3 in the folder first. Trust the folder to turn it on.",
        ),
        _ => None,
    }
}

/// Why the options of a server cannot be made safe on this computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionsError {
    /// No TypeScript that was installed with the language server was found.
    NoTrustedTypescript,
}

/// The `initializationOptions` the server gets, or `None` for the server's own defaults. A
/// trusted folder gets the defaults. Without trust, TypeScript runs the `tsserver` that was
/// installed with the language server, never one from the folder's `node_modules`, and installs
/// no type packages (the installer runs npm); pyright runs no program of the folder and needs none.
pub fn initialization_options(
    language: &EditorLanguage,
    trusted: bool,
    search_path: Option<&std::ffi::OsStr>,
) -> Result<Option<Value>, OptionsError> {
    if trusted || *language != EditorLanguage::Typescript {
        return Ok(None);
    }
    let tsserver = search_path
        .and_then(installed_tsserver)
        .ok_or(OptionsError::NoTrustedTypescript)?;
    Ok(Some(json!({
        "tsserver":{"path":tsserver.to_string_lossy(),"logVerbosity":"off"},
        "disableAutomaticTypingAcquisition":true,
        "plugins":[]
    })))
}

/// The absolute path of `command` found in the absolute entries of `search_path`. A relative or
/// empty entry names a folder of the working directory, which for a language server is the
/// project folder, so it never counts.
pub fn absolute_command(command: &str, search_path: &std::ffi::OsStr) -> Option<PathBuf> {
    std::env::split_paths(search_path)
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join(command))
        .find(|candidate| candidate.is_file())
}

const TYPESCRIPT_SERVER: &str = "typescript-language-server";

/// The `tsserver.js` that sits with the `typescript-language-server` found on `search_path`: in
/// the `node_modules` that holds the server (where npm puts its dependencies) or in its own. The
/// `typescript` package must be whole (a readable `package.json` with a version): a server that
/// cannot read its version falls back to the folder's own TypeScript.
fn installed_tsserver(search_path: &std::ffi::OsStr) -> Option<PathBuf> {
    let program = absolute_command(TYPESCRIPT_SERVER, search_path)?;
    let program = soyeht_platform::fs::canonicalize(&program).ok()?;
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
                if tsserver.is_file() && has_version(&package.join("package.json")) {
                    return Some(tsserver);
                }
            }
        }
    }
    None
}

fn has_version(package_json: &Path) -> bool {
    std::fs::read_to_string(package_json)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .is_some_and(|package| package["version"].as_str().is_some_and(|v| !v.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_closed_to_the_four_contracted_servers() {
        let supported = [
            EditorLanguage::Rust,
            EditorLanguage::Swift,
            EditorLanguage::Typescript,
            EditorLanguage::Python,
        ];
        assert_eq!(supported.iter().filter_map(config).count(), 4);
        for unsupported in [
            EditorLanguage::PlainText,
            EditorLanguage::Json,
            EditorLanguage::Markdown,
            EditorLanguage::Shell,
        ] {
            assert!(config(&unsupported).is_none());
        }
    }

    #[test]
    fn a_trusted_folder_gets_every_server_with_its_own_defaults() {
        for language in [
            EditorLanguage::Rust,
            EditorLanguage::Swift,
            EditorLanguage::Typescript,
            EditorLanguage::Python,
        ] {
            assert_eq!(refusal(&language, true), None);
            assert_eq!(initialization_options(&language, true, None), Ok(None));
        }
    }

    #[test]
    fn rust_and_swift_have_no_safe_mode_and_python_needs_none() {
        for language in [EditorLanguage::Rust, EditorLanguage::Swift] {
            let reason = refusal(&language, false).unwrap();
            assert!(reason.contains("Trust the folder"), "{reason}");
            assert!(reason.len() <= 1024);
        }
        assert_eq!(refusal(&EditorLanguage::Typescript, false), None);
        assert_eq!(
            refusal(&EditorLanguage::Python, false).is_some(),
            cfg!(windows)
        );
        assert_eq!(
            initialization_options(&EditorLanguage::Python, false, None),
            Ok(None)
        );
    }

    #[cfg(unix)]
    #[test]
    fn typescript_without_trust_runs_the_tsserver_installed_beside_the_server() {
        let root = std::env::temp_dir().join(format!("sn-tsserver-{}", std::process::id()));
        let modules = root.join("lib/node_modules");
        let server = modules.join("typescript-language-server/lib");
        std::fs::create_dir_all(&server).unwrap();
        std::fs::create_dir_all(modules.join("typescript/lib")).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(server.join("cli.mjs"), "").unwrap();
        std::os::unix::fs::symlink(
            server.join("cli.mjs"),
            root.join("bin/typescript-language-server"),
        )
        .unwrap();
        let search = root.join("bin");
        let missing =
            initialization_options(&EditorLanguage::Typescript, false, Some(search.as_os_str()));
        assert_eq!(missing, Err(OptionsError::NoTrustedTypescript));

        std::fs::write(modules.join("typescript/lib/tsserver.js"), "").unwrap();
        // A partial install (no readable package.json) is not trusted: the server would fall back
        // to the folder's own TypeScript.
        assert_eq!(
            initialization_options(&EditorLanguage::Typescript, false, Some(search.as_os_str())),
            Err(OptionsError::NoTrustedTypescript)
        );
        std::fs::write(
            modules.join("typescript/package.json"),
            "{\"version\":\"5.9.2\"}",
        )
        .unwrap();
        let options =
            initialization_options(&EditorLanguage::Typescript, false, Some(search.as_os_str()))
                .unwrap()
                .unwrap();
        let expected =
            soyeht_platform::fs::canonicalize(&modules.join("typescript/lib/tsserver.js")).unwrap();
        assert_eq!(options["tsserver"]["path"], expected.to_str().unwrap());
        assert_eq!(options["disableAutomaticTypingAcquisition"], true);
        assert_eq!(options["plugins"], json!([]));
        assert_eq!(
            initialization_options(&EditorLanguage::Typescript, false, None),
            Err(OptionsError::NoTrustedTypescript)
        );
        // A relative or empty entry of the search path is the project folder, never a program.
        let relative =
            std::env::join_paths([std::path::Path::new(""), std::path::Path::new("bin")]).unwrap();
        assert_eq!(
            absolute_command("typescript-language-server", &relative),
            None
        );
        assert_eq!(
            initialization_options(&EditorLanguage::Typescript, false, Some(&relative)),
            Err(OptionsError::NoTrustedTypescript)
        );
        assert!(absolute_command("typescript-language-server", search.as_os_str()).is_some());
        let _ = std::fs::remove_dir_all(root);
    }
}
