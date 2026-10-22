// Generated from contracts/protocol.json. Do not edit.
use crate::ReadinessSystem;

/// The commands that install one program, by system.
pub struct Hints(&'static [(ReadinessSystem, &'static str)]);

impl Hints {
    /// The command that installs it on `system`, when there is one.
    pub fn for_system(&self, system: &ReadinessSystem) -> Option<&'static str> {
        self.0
            .iter()
            .find(|(candidate, _)| candidate == system)
            .map(|(_, command)| *command)
    }
}

/// What to say with a command that starts with `prefix`: what it needs first.
pub struct Note {
    pub prefix: &'static str,
    pub text: &'static str,
}

/// The note that goes with an install `hint`, if its command has a prerequisite.
pub fn note_for(hint: &str) -> Option<&'static str> {
    NOTES
        .iter()
        .find(|note| hint.starts_with(note.prefix))
        .map(|note| note.text)
}

/// A program the engine runs.
pub struct Tool {
    pub id: &'static str,
    pub name: &'static str,
    /// Without it part of the engine does not work, not only a convenience.
    pub required: bool,
    /// The systems the engine runs it on.
    pub systems: &'static [ReadinessSystem],
    pub hints: Hints,
}

/// The CLI of a harness of the registry.
pub struct Harness {
    pub id: &'static str,
    pub hints: Hints,
}

/// The systems the checklist knows commands for.
const EVERYWHERE: &[ReadinessSystem] = &[
    ReadinessSystem::Macos,
    ReadinessSystem::Debian,
    ReadinessSystem::Fedora,
    ReadinessSystem::Windows,
];

pub const NOTES: &[Note] = &[Note {
    prefix: "brew ",
    text: "Needs Homebrew: https://brew.sh",
}];

pub const TOOLS: &[Tool] = &[
    Tool {
        id: "git",
        name: "Git",
        required: true,
        systems: EVERYWHERE,
        hints: Hints(&[
            (ReadinessSystem::Macos, "xcode-select --install"),
            (ReadinessSystem::Debian, "sudo apt install git"),
            (ReadinessSystem::Fedora, "sudo dnf install git"),
            (ReadinessSystem::Windows, "winget install --id Git.Git -e"),
        ]),
    },
    Tool {
        id: "node",
        name: "Node.js",
        required: true,
        systems: EVERYWHERE,
        hints: Hints(&[
            (ReadinessSystem::Macos, "brew install node"),
            (ReadinessSystem::Debian, "sudo apt install nodejs npm"),
            (ReadinessSystem::Fedora, "sudo dnf install nodejs npm"),
            (
                ReadinessSystem::Windows,
                "winget install --id OpenJS.NodeJS.LTS -e",
            ),
        ]),
    },
    Tool {
        id: "wt",
        name: "Worktrunk",
        required: false,
        systems: EVERYWHERE,
        hints: Hints(&[
            (ReadinessSystem::Macos, "brew install worktrunk"),
            (ReadinessSystem::Debian, "brew install worktrunk"),
            (ReadinessSystem::Fedora, "brew install worktrunk"),
            (
                ReadinessSystem::Windows,
                "winget install max-sixty.worktrunk",
            ),
        ]),
    },
    Tool {
        id: "lsof",
        name: "lsof",
        required: false,
        systems: &[
            ReadinessSystem::Macos,
            ReadinessSystem::Debian,
            ReadinessSystem::Fedora,
        ],
        hints: Hints(&[
            (ReadinessSystem::Macos, "brew install lsof"),
            (ReadinessSystem::Debian, "sudo apt install lsof"),
            (ReadinessSystem::Fedora, "sudo dnf install lsof"),
        ]),
    },
];

pub const HARNESSES: &[Harness] = &[
    Harness {
        id: "registry.claude-acp",
        hints: Hints(&[
            (ReadinessSystem::Macos, "brew install --cask claude-code"),
            (
                ReadinessSystem::Debian,
                "curl -fsSL https://claude.ai/install.sh | bash",
            ),
            (
                ReadinessSystem::Fedora,
                "curl -fsSL https://claude.ai/install.sh | bash",
            ),
            (
                ReadinessSystem::Windows,
                "irm https://claude.ai/install.ps1 | iex",
            ),
        ]),
    },
    Harness {
        id: "registry.codex-acp",
        hints: Hints(&[
            (ReadinessSystem::Macos, "brew install --cask codex"),
            (
                ReadinessSystem::Debian,
                "curl -fsSL https://chatgpt.com/codex/install.sh | sh",
            ),
            (
                ReadinessSystem::Fedora,
                "curl -fsSL https://chatgpt.com/codex/install.sh | sh",
            ),
            (ReadinessSystem::Windows, "npm install -g @openai/codex"),
        ]),
    },
    Harness {
        id: "registry.cursor",
        hints: Hints(&[
            (
                ReadinessSystem::Macos,
                "curl https://cursor.com/install -fsS | bash",
            ),
            (
                ReadinessSystem::Debian,
                "curl https://cursor.com/install -fsS | bash",
            ),
            (
                ReadinessSystem::Fedora,
                "curl https://cursor.com/install -fsS | bash",
            ),
            (
                ReadinessSystem::Windows,
                "irm 'https://cursor.com/install?win32=true' | iex",
            ),
        ]),
    },
    Harness {
        id: "registry.opencode",
        hints: Hints(&[
            (
                ReadinessSystem::Macos,
                "curl -fsSL https://opencode.ai/install | bash",
            ),
            (
                ReadinessSystem::Debian,
                "curl -fsSL https://opencode.ai/install | bash",
            ),
            (
                ReadinessSystem::Fedora,
                "curl -fsSL https://opencode.ai/install | bash",
            ),
            (ReadinessSystem::Windows, "npm install -g opencode-ai"),
        ]),
    },
];
