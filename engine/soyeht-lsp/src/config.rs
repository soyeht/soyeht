use soyeht_protocol::EditorLanguage;

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
        EditorLanguage::Python => Some(
            "pyright is off in a restricted folder: it runs python3 in the project folder with the folder first on the import path, so a file of the folder such as json.py would run. Trust the folder to turn it on.",
        ),
        _ => None,
    }
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
    fn rust_swift_and_python_have_no_safe_mode_and_typescript_has_one() {
        for language in [
            EditorLanguage::Rust,
            EditorLanguage::Swift,
            EditorLanguage::Python,
        ] {
            let reason = refusal(&language, false).unwrap();
            assert!(reason.contains("Trust the folder"), "{reason}");
            assert!(reason.len() <= 1024);
            assert_eq!(refusal(&language, true), None);
        }
        assert_eq!(refusal(&EditorLanguage::Typescript, false), None);
    }
}
