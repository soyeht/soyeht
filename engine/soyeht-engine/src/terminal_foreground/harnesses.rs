//! The terminal harnesses the engine knows by the program a terminal runs (ADR 0165).
//!
//! A harness is told by data, not by a branch per product: the table lists, for each one, the
//! command names it runs under and the folders its files live in. The shapes were measured on the real binaries: Claude Code runs a file named
//! after its version with `argv[0]` `claude`, Codex and OpenCode run a native file under the
//! package manager's folder with their own name as `argv[0]`, and Cursor Agent runs its bundled
//! `node` with `argv[0]` the path it was started by and its own script as the first argument.

use soyeht_ptyd::wire::Program;

/// What a terminal harness is called, and how the engine tells that a program is it.
pub(super) struct Known {
    /// The harness id of the registry, which the apps already know it by.
    pub(super) id: &'static str,
    pub(super) name: &'static str,
    /// The command names, lowercase and with no extension, that are this harness whether they
    /// are `argv[0]` or the name of the file it runs.
    commands: &'static [&'static str],
    /// Folder names, lowercase and joined by `/`, that the path of this harness's file or script
    /// holds. They tell a name another program also has (Cursor's short name `agent`) and a
    /// file that is not named after the harness (Cursor's bundled `node`, Claude's versions).
    markers: &'static [&'static str],
    /// Whether `--version` is asked of the program as the person started it (`argv[0]`) before
    /// the file it runs: Cursor's file is the `node` that hosts it.
    pub(super) version_from_launch: bool,
}

pub(super) const KNOWN: [Known; 4] = [
    Known {
        id: "registry.claude-acp",
        name: "Claude Code",
        commands: &["claude"],
        markers: &["claude/versions", "@anthropic-ai/claude-code"],
        version_from_launch: false,
    },
    Known {
        id: "registry.codex-acp",
        name: "Codex",
        commands: &["codex"],
        markers: &["@openai/codex"],
        version_from_launch: false,
    },
    Known {
        id: "registry.opencode",
        name: "OpenCode",
        commands: &["opencode"],
        markers: &["opencode-ai"],
        version_from_launch: false,
    },
    Known {
        id: "registry.cursor",
        name: "Cursor Agent",
        commands: &["cursor-agent"],
        markers: &["cursor-agent"],
        version_from_launch: true,
    },
];

/// Programs a harness may be started through, or hosted by: they are looked through, never
/// taken for the harness. A shell is what a terminal pane runs; the others start a program for
/// it. The console host of a Windows pane is among them.
const LAUNCHERS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "fish",
    "dash",
    "ksh",
    "csh",
    "tcsh",
    "nu",
    "pwsh",
    "powershell",
    "cmd",
    "env",
    "sudo",
    "npx",
    "npm",
    "pnpm",
    "yarn",
    "bunx",
    "winpty",
    "conhost",
    "openconsole",
];

/// Programs that run a script file, whose first argument is then what is running.
const INTERPRETERS: &[&str] = &["node", "bun", "deno"];

/// How a program was told to be a harness, which decides what `--version` is asked of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Via<'a> {
    /// By its own command name or its own folder: the file it runs is the harness.
    Program,
    /// By the script a host program (`node`) runs.
    Script(&'a str),
}

/// A program that is a known harness.
pub(super) struct Matched<'a> {
    pub(super) known: &'static Known,
    pub(super) program: &'a Program,
    pub(super) via: Via<'a>,
}

/// `C:\a\Codex.EXE` and `/a/codex` both read `codex`: the file name, in lowercase, without the
/// extension that names how the system runs it.
fn command_name(path: &str) -> String {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_lowercase();
    for extension in [".exe", ".cmd", ".bat", ".ps1", ".js", ".mjs", ".cjs"] {
        if let Some(stem) = name.strip_suffix(extension) {
            return stem.to_owned();
        }
    }
    name
}

/// The folder names of a path in lowercase, joined by `/` and with a `/` at each end, so that a
/// marker is found whole and never inside a longer name.
fn folders(path: &str) -> String {
    let normal = path.replace('\\', "/").to_lowercase();
    let names: Vec<&str> = normal.split('/').filter(|name| !name.is_empty()).collect();
    format!("/{}/", names.join("/"))
}

fn has_marker(known: &Known, path: &str) -> bool {
    let folders = folders(path);
    known
        .markers
        .iter()
        .any(|marker| folders.contains(&format!("/{marker}/")))
}

/// The first argument that is not an option: the script an interpreter runs.
fn script_of(program: &Program) -> Option<&str> {
    program
        .arguments
        .iter()
        .skip(1)
        .map(String::as_str)
        .find(|argument| !argument.starts_with('-'))
}

/// The known harness that `program` is, if it is one.
pub(super) fn identify(program: &Program) -> Option<Matched<'_>> {
    let launch = program.arguments.first().map(String::as_str);
    let places: Vec<&str> = [launch, program.executable.as_deref()]
        .into_iter()
        .flatten()
        .collect();
    let by_program = KNOWN.iter().find(|known| {
        places.iter().any(|place| {
            known.commands.contains(&command_name(place).as_str()) || has_marker(known, place)
        })
    });
    if let Some(known) = by_program {
        return Some(Matched {
            known,
            program,
            via: Via::Program,
        });
    }
    // A host program runs the harness's script: `node` and the first argument that is a file.
    let host = program.executable.as_deref().map(command_name)?;
    if !INTERPRETERS.contains(&host.as_str()) {
        return None;
    }
    let script = script_of(program)?;
    KNOWN
        .iter()
        .find(|known| has_marker(known, script))
        .map(|known| Matched {
            known,
            program,
            via: Via::Script(script),
        })
}

/// Whether `program` only starts other programs or hosts them (a shell, a package runner, a
/// console host), so the harness may be among what it started.
fn is_launcher(program: &Program) -> bool {
    [
        program.executable.as_deref(),
        program.arguments.first().map(String::as_str),
    ]
    .into_iter()
    .flatten()
    .map(command_name)
    .any(|name| LAUNCHERS.contains(&name.as_str()))
}

/// The harness among the programs of a terminal, taken in tree order (a program before what it
/// started). It is the shallowest known harness reached from the first program only through
/// launchers: a harness's own children (its tool servers, its shell commands) are deeper and are
/// never taken, and a program that is neither a launcher nor a harness hides what it started.
pub(super) fn pick(programs: &[Program]) -> Option<Matched<'_>> {
    let mut open: Vec<u32> = Vec::new();
    for program in programs {
        let reachable = program
            .parent_pid
            .is_none_or(|parent| open.contains(&parent));
        if !reachable {
            continue;
        }
        if let Some(matched) = identify(program) {
            return Some(matched);
        }
        if is_launcher(program) {
            open.push(program.pid);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(pid: u32, parent: Option<u32>, exe: &str, args: &[&str]) -> Program {
        Program {
            pid,
            parent_pid: parent,
            executable: Some(exe.into()),
            arguments: args.iter().map(|argument| (*argument).into()).collect(),
        }
    }

    fn named(programs: &[Program]) -> Option<&'static str> {
        pick(programs).map(|matched| matched.known.name)
    }

    const CURSOR_NODE: &str = "/Users/x/.local/share/cursor-agent/versions/2026.09.02-c22c1a3/node";
    const CURSOR_SCRIPT: &str =
        "/Users/x/.local/share/cursor-agent/versions/2026.09.02-c22c1a3/index.js";

    /// The foreground programs of the four harnesses as `ps` showed them on the real binaries.
    fn measured() -> Vec<(&'static str, Program)> {
        vec![
            (
                "Claude Code",
                program(
                    10,
                    None,
                    "/Users/x/.local/share/claude/versions/2.1.325",
                    &["claude", "--strict-mcp-config"],
                ),
            ),
            (
                "Codex",
                program(
                    11,
                    None,
                    "/opt/homebrew/Caskroom/codex/0.169.0/bin/codex",
                    &["codex", "--disable", "hooks"],
                ),
            ),
            (
                "OpenCode",
                program(
                    12,
                    None,
                    "/opt/homebrew/Cellar/opencode-v2/2.2.5/bin/opencode",
                    &["opencode", "--standalone"],
                ),
            ),
            (
                "Cursor Agent",
                program(
                    13,
                    None,
                    CURSOR_NODE,
                    &[
                        "/Users/x/.local/bin/cursor-agent",
                        "--use-system-ca",
                        CURSOR_SCRIPT,
                        "--approve-mcps",
                        "--trust",
                    ],
                ),
            ),
        ]
    }

    #[test]
    fn each_harness_is_known_by_the_shape_its_real_binary_has() {
        for (name, program) in measured() {
            assert_eq!(named(&[program]), Some(name), "{name}");
        }
    }

    #[test]
    fn cursor_is_known_under_its_short_name_and_by_its_script_alone() {
        let short = program(
            1,
            None,
            CURSOR_NODE,
            &["agent", "--use-system-ca", CURSOR_SCRIPT],
        );
        assert_eq!(named(&[short]), Some("Cursor Agent"));
        // Started by a launcher that rewrote `argv[0]`, the bundled file and script still say it.
        let rewritten = program(1, None, CURSOR_NODE, &["node", CURSOR_SCRIPT]);
        assert_eq!(named(&[rewritten]), Some("Cursor Agent"));
    }

    #[test]
    fn a_short_name_alone_is_not_a_harness() {
        let other = program(1, None, "/usr/local/bin/agent", &["agent", "--serve"]);
        assert_eq!(named(&[other]), None);
    }

    #[test]
    fn the_programs_of_an_ordinary_terminal_are_no_harness() {
        for (exe, args) in [
            ("/bin/zsh", &["-zsh"][..]),
            ("/bin/sleep", &["sleep", "30"]),
            ("/usr/bin/vim", &["vim", "claude.md"]),
            ("/usr/bin/grep", &["grep", "codex", "notes.txt"]),
            (
                "/usr/bin/python3",
                &["python3", "probe-mcp.py", "claude-probe"],
            ),
            ("/usr/local/bin/node", &["node", "server.js"]),
            ("/usr/local/bin/node", &["node"]),
            (
                "/usr/local/bin/node",
                &["node", "--inspect", "/work/claude-notes/run.js"],
            ),
        ] {
            assert_eq!(
                named(&[program(1, None, exe, args)]),
                None,
                "{exe} {args:?}"
            );
        }
    }

    #[test]
    fn windows_programs_are_known_by_their_files_and_by_the_script_node_runs() {
        let native = program(
            1,
            None,
            r"C:\Users\x\AppData\Local\Programs\opencode\OpenCode.EXE",
            &[r"C:\Users\x\AppData\Local\Programs\opencode\opencode.exe"],
        );
        assert_eq!(named(&[native]), Some("OpenCode"));
        let shim = program(
            1,
            None,
            r"C:\Program Files\nodejs\node.exe",
            &[
                r"C:\Program Files\nodejs\node.exe",
                r"C:\Users\x\AppData\Roaming\npm\node_modules\@openai\codex\bin\codex.js",
                "--model",
                "x",
            ],
        );
        let matched = pick(std::slice::from_ref(&shim)).unwrap();
        assert_eq!(matched.known.name, "Codex");
        assert!(matches!(matched.via, Via::Script(script) if script.ends_with("codex.js")));
    }

    #[test]
    fn a_harness_is_found_through_shells_and_never_among_its_own_children() {
        let tree = [
            program(
                1,
                None,
                r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
                &["powershell.exe"],
            ),
            program(
                2,
                Some(1),
                r"C:\Windows\System32\cmd.exe",
                &["cmd.exe", "/c", "codex"],
            ),
            program(
                3,
                Some(2),
                r"C:\Program Files\nodejs\node.exe",
                &["node", r"C:\n\node_modules\@openai\codex\bin\codex.js"],
            ),
            // The harness's tool server runs a script of the same product, deeper.
            program(
                4,
                Some(3),
                r"C:\Program Files\nodejs\node.exe",
                &[
                    "node",
                    r"C:\n\node_modules\@anthropic-ai\claude-code\cli.js",
                ],
            ),
        ];
        let matched = pick(&tree).unwrap();
        assert_eq!((matched.known.name, matched.program.pid), ("Codex", 3));
    }

    #[test]
    fn a_program_between_the_shell_and_the_harness_hides_it() {
        let tree = [
            program(1, None, "/bin/zsh", &["zsh"]),
            program(2, Some(1), "/usr/bin/make", &["make", "serve"]),
            program(3, Some(2), "/x/bin/claude", &["claude"]),
        ];
        assert!(pick(&tree).is_none());
    }

    #[test]
    fn a_launcher_that_starts_a_harness_is_looked_through() {
        let tree = [
            program(1, None, "/bin/bash", &["bash"]),
            program(
                2,
                Some(1),
                "/usr/local/bin/node",
                &["node", "/usr/local/bin/npx", "opencode"],
            ),
            program(3, Some(1), "/usr/bin/env", &["env", "opencode"]),
            program(4, Some(3), "/opt/x/bin/opencode", &["opencode"]),
        ];
        let matched = pick(&tree).unwrap();
        assert_eq!((matched.known.name, matched.program.pid), ("OpenCode", 4));
    }

    #[test]
    fn nothing_in_a_terminal_is_no_harness() {
        assert!(pick(&[]).is_none());
    }
}
