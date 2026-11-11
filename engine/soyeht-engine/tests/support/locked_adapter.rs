//! The programs a locked adapter runs through (ADR 0143), for the rigs that start the Claude
//! adapter.

use super::*;

impl Rig {
    /// The programs a locked adapter runs through: an `npm` that lays out the tree its lockfile
    /// names, as `npm ci` would, and a `node` that starts the fake agent in place of the adapter's
    /// script after `before` (a shell fragment run first). No package is downloaded.
    #[cfg(unix)]
    pub fn install_fake_locked_adapter_runner(&self, before: &str) {
        // The registry still names `npx` as the adapter's program, which must be there to be
        // found; a locked adapter never runs it.
        self.install_stub_executable("npx");
        write_script(
            &self.path.join("bin/npm"),
            "#!/bin/sh\n\
             name=$(sed -n 's/^ *\"\\(@agentclientprotocol[^\"]*\\)\": *\"[^\"]*\".*/\\1/p' package.json)\n\
             version=$(sed -n 's/^ *\"@agentclientprotocol[^\"]*\": *\"\\([^\"]*\\)\".*/\\1/p' package.json)\n\
             mkdir -p \"node_modules/$name/dist\"\n\
             printf '{\"version\":\"%s\"}' \"$version\" > \"node_modules/$name/package.json\"\n\
             echo '// adapter' > \"node_modules/$name/dist/index.js\"\n",
        );
        write_script(
            &self.path.join("bin/node"),
            &format!(
                "#!/bin/sh\n{before}\nexec '{}' --state-dir '{}'\n",
                sibling_binary("soyeht-fake-acp").display(),
                self.path.join("claude-native").display()
            ),
        );
    }
    /// The fake runner of the locked Claude adapter, which starts the fake agent.
    #[cfg(unix)]
    pub fn install_fake_claude_runner(&self) {
        self.install_fake_locked_adapter_runner("");
    }
}
