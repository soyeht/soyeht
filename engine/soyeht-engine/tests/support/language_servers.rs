//! The fake language server, installed under the names of the four real ones. The fake is the
//! Rust server; for Python it is the same program, and for TypeScript it sits in the layout npm
//! gives the real one (its `typescript` package beside it), so the tests of a restricted folder
//! can see which `tsserver` the engine chose.
use super::install_fake;
use std::{fs, path::Path};

pub fn install(root: &Path) {
    install_fake(root, "soyeht-fake-lsp", "rust-analyzer");
    install_fake(root, "soyeht-fake-lsp", "pyright-langserver");
    let modules = root.join("npm/node_modules");
    let server = modules.join("typescript-language-server/lib");
    fs::create_dir_all(&server).unwrap();
    fs::create_dir_all(modules.join("typescript/lib")).unwrap();
    fs::write(modules.join("typescript/lib/tsserver.js"), "").unwrap();
    fs::write(
        modules.join("typescript/package.json"),
        "{\"version\":\"5.9.2\"}",
    )
    .unwrap();
    #[cfg(unix)]
    {
        let program = server.join("cli");
        // A real file, as npm installs the server: a link here would resolve to the fake itself.
        fs::copy(super::sibling_binary("soyeht-fake-lsp"), &program).unwrap();
        std::os::unix::fs::symlink(program, root.join("bin/typescript-language-server")).unwrap();
    }
}
