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
}
