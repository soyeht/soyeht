//! The terminal harnesses the engine knows by the program a terminal runs (ADR 0165).
//!
//! A harness is told by data, not by a branch per product. The table lists, for each one, the
//! command names it runs under and the install layouts its files live in; the rules below read
//! the program's executable path, `argv[0]` and, for a host program such as `node`, its script.
//! The shapes were measured on the real binaries (the spike): Claude Code runs a file named after
//! its version with `argv[0]` `claude`, Codex and OpenCode run a native file under the package
//! manager's folder with their own name as `argv[0]`, and Cursor Agent runs its bundled `node`
//! with `argv[0]` the path it was started by and its own script as the first argument.
//!
//! A program is a harness when one of these holds, and only these:
//!
//! 1. a file it runs (executable, or the script of a host program) sits in an install layout of
//!    the harness (`.local/share/cursor-agent/versions/`, `node_modules/@openai/codex/`, ...);
//! 2. `argv[0]` is the bare command name (the shell found it on the person's `PATH`);
//! 3. a file it runs (executable, `argv[0]`, or the script of a host program) is named like the
//!    command and the folder it is in is on the person's `PATH`. A harness installed with npm runs
//!    as `node <the link npm made in a PATH folder>`: the interpreter and the link as it was
//!    invoked, which is this rule applied to the script (measured on macOS and Linux).
//!
//! A binary named like a harness in a project folder is none of them. A wrong match only
//! mislabels an `agent.list` entry; it grants nothing.

use soyeht_platform::process::inspect::MAX_ARGUMENT_BYTES;
use soyeht_protocol::readiness::HARNESSES;
use soyeht_ptyd::wire::Program;

/// What a terminal harness is called, and how the engine tells that a program is it.
pub(super) struct Known {
    /// The harness id of the registry, which the apps already know it by.
    pub(super) id: &'static str,
    pub(super) name: &'static str,
    /// The command names, lowercase and with no extension, that are this harness.
    commands: &'static [&'static str],
    /// The install layouts: runs of folder names, lowercase and joined by `/`, that the path of
    /// this harness's file or script holds whole and in order.
    layouts: &'static [&'static str],
}

/// The registry's own id of the harness at `position` (the order of `readiness::HARNESSES`).
const fn registry_id(position: usize) -> &'static str {
    HARNESSES[position].id
}

pub(super) const KNOWN: [Known; 4] = [
    Known {
        id: registry_id(0),
        name: "Claude Code",
        commands: &["claude"],
        layouts: &[
            ".local/share/claude/versions",
            "node_modules/@anthropic-ai/claude-code",
            "caskroom/claude-code",
        ],
    },
    Known {
        id: registry_id(1),
        name: "Codex",
        commands: &["codex"],
        layouts: &[
            "caskroom/codex",
            "cellar/codex",
            "node_modules/@openai/codex",
        ],
    },
    Known {
        id: registry_id(2),
        name: "Cursor Agent",
        commands: &["cursor-agent"],
        layouts: &[
            ".local/share/cursor-agent/versions",
            "appdata/local/cursor-agent",
        ],
    },
    Known {
        id: registry_id(3),
        name: "OpenCode",
        commands: &["opencode"],
        layouts: &[
            ".opencode/bin",
            "cellar/opencode",
            "cellar/opencode-v2",
            "caskroom/opencode",
            "node_modules/opencode-ai",
        ],
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
    "winpty",
    "conhost",
    "openconsole",
];

/// Programs that run a script file, whose first argument is then what is running.
const INTERPRETERS: &[&str] = &["node", "bun", "deno"];

/// A program that is a known harness.
pub(super) struct Matched<'a> {
    pub(super) known: &'static Known,
    pub(super) program: &'a Program,
    /// The file that is the harness: the script a host program runs, or the executable.
    file: &'a str,
}

impl Matched<'_> {
    /// The version the install path carries (`.../versions/2.1.325`, `Caskroom/codex/0.169.0/`),
    /// read from the path alone: no program is run to ask.
    pub(super) fn version(&self) -> Option<String> {
        let launched = self.program.arguments.first().map(String::as_str);
        [Some(self.file), launched]
            .into_iter()
            .flatten()
            .find_map(|path| version_after_layout(self.known, path))
    }
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
/// layout is found whole and never inside a longer name.
pub(super) fn folders(path: &str) -> String {
    let normal = path.replace('\\', "/").to_lowercase();
    let names: Vec<&str> = normal.split('/').filter(|name| !name.is_empty()).collect();
    format!("/{}/", names.join("/"))
}

/// The folder a file is in, as `folders` writes it, or `None` for a bare name.
fn folder_of(path: &str) -> Option<String> {
    let (folder, _) = path.rsplit_once(['/', '\\'])?;
    Some(folders(folder))
}

fn is_bare(path: &str) -> bool {
    !path.contains(['/', '\\'])
}

fn in_layout(known: &Known, path: &str) -> bool {
    let folders = folders(path);
    known
        .layouts
        .iter()
        .any(|layout| folders.contains(&format!("/{layout}/")))
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

/// The known harness that `program` is, if it is one. `on_path` says whether a folder (as
/// `folders` writes it) is on the person's `PATH`; it is asked only when the other rules did
/// not decide.
pub(super) fn identify<'a>(
    program: &'a Program,
    on_path: &dyn Fn(&str) -> bool,
) -> Option<Matched<'a>> {
    let launch = program.arguments.first().map(String::as_str);
    let executable = program.executable.as_deref();
    let places: Vec<&str> = [launch, executable].into_iter().flatten().collect();
    let matched = |known: &'static Known, file: &'a str| Matched {
        known,
        program,
        file,
    };
    let file = executable.or(launch)?;
    if let Some(known) = KNOWN
        .iter()
        .find(|known| places.iter().any(|place| in_layout(known, place)))
    {
        return Some(matched(known, file));
    }
    let named = |known: &Known, place: &str| known.commands.contains(&command_name(place).as_str());
    // The shell found the command on the person's `PATH` and ran it under its bare name.
    if let Some(known) = launch.and_then(|launch| {
        KNOWN
            .iter()
            .find(|known| is_bare(launch) && named(known, launch))
    }) {
        return Some(matched(known, file));
    }
    // A file named like the command, in a folder of the person's `PATH`.
    if let Some(known) = KNOWN.iter().find(|known| {
        places.iter().any(|place| {
            named(known, place) && folder_of(place).is_some_and(|folder| on_path(&folder))
        })
    }) {
        return Some(matched(known, file));
    }
    // A host program runs the harness's script: `node` and the first argument that is a file.
    let host = command_name(executable?);
    if !INTERPRETERS.contains(&host.as_str()) {
        return None;
    }
    let script = script_of(program)?;
    KNOWN
        .iter()
        .find(|known| {
            in_layout(known, script)
                || (named(known, script)
                    && folder_of(script).is_some_and(|folder| on_path(&folder)))
        })
        .map(|known| matched(known, script))
}

/// Whether `program` only starts other programs or hosts them (a shell, a console host), so the
/// harness may be among what it started.
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
pub(super) fn pick<'a>(
    programs: &'a [Program],
    on_path: &dyn Fn(&str) -> bool,
) -> Option<Matched<'a>> {
    let mut open: Vec<u32> = Vec::new();
    for program in programs {
        let reachable = program
            .parent_pid
            .is_none_or(|parent| open.contains(&parent));
        if !reachable {
            continue;
        }
        if let Some(matched) = identify(program, on_path) {
            return Some(matched);
        }
        if is_launcher(program) {
            open.push(program.pid);
        }
    }
    None
}

/// The longest a version is, as the contract says (`TerminalHarness.version`).
const LONGEST_VERSION: usize = 64;

/// How many parts after the install layout may hold the version (`.../versions/<version>`).
const PARTS_AFTER_LAYOUT: usize = 2;

/// The version an install layout carries: the part after the layout (or the one after that) that
/// reads `1.2.3`, `0.169.0` or `2026.09.02-c22c1a3` (three numbers joined by dots, then an
/// optional suffix after `-` or `+`). Only what follows the harness's own layout is read, so a
/// node manager's or another folder's version in the path is never taken for the harness's. A path
/// that was cut short (it is as long as the longest the supervisor keeps) and ends in the part is
/// not read: the part may be cut.
fn version_after_layout(known: &Known, path: &str) -> Option<String> {
    let parts: Vec<&str> = path
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect();
    let lowered: Vec<String> = parts.iter().map(|part| part.to_lowercase()).collect();
    let cut = path.len() >= MAX_ARGUMENT_BYTES;
    known.layouts.iter().find_map(|layout| {
        let wanted: Vec<&str> = layout.split('/').collect();
        let at = lowered
            .windows(wanted.len())
            .position(|window| window.iter().zip(&wanted).all(|(part, want)| part == want))?;
        let after = at + wanted.len();
        parts
            .iter()
            .enumerate()
            .skip(after)
            .take(PARTS_AFTER_LAYOUT)
            .find(|(index, part)| is_version(part) && !(cut && index + 1 == parts.len()))
            .map(|(_, part)| (*part).to_owned())
    })
}

fn is_version(part: &str) -> bool {
    let Some(number) = part.split(['-', '+']).next() else {
        return false;
    };
    let numbers: Vec<&str> = number.split('.').collect();
    numbers.len() == 3
        && numbers
            .iter()
            .all(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        && part.len() <= LONGEST_VERSION
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'))
}

/// A program as the tests of this module and of its users spell one.
#[cfg(test)]
pub(super) fn sample(pid: u32, parent: Option<u32>, exe: &str, args: &[&str]) -> Program {
    Program {
        pid,
        parent_pid: parent,
        executable: Some(exe.into()),
        arguments: args.iter().map(|argument| (*argument).into()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::sample as program;
    use super::*;

    /// A `PATH` of this person: the folders `which` would search.
    fn on_path(folder: &str) -> bool {
        ["/users/x/.local/bin/", "/opt/homebrew/bin/", "/c/tools/"]
            .iter()
            .any(|path| folder.ends_with(path) || folder == *path)
    }

    fn named(programs: &[Program]) -> Option<&'static str> {
        pick(programs, &on_path).map(|matched| matched.known.name)
    }

    const CURSOR_NODE: &str = "/Users/x/.local/share/cursor-agent/versions/2026.09.02-c22c1a3/node";
    const CURSOR_SCRIPT: &str =
        "/Users/x/.local/share/cursor-agent/versions/2026.09.02-c22c1a3/index.js";

    /// The foreground programs of the four harnesses as `ps` showed them on the real binaries.
    fn measured() -> Vec<(&'static str, &'static str, Program)> {
        vec![
            (
                "Claude Code",
                "2.1.325",
                program(
                    10,
                    None,
                    "/Users/x/.local/share/claude/versions/2.1.325",
                    &["claude", "--strict-mcp-config"],
                ),
            ),
            (
                "Codex",
                "0.169.0",
                program(
                    11,
                    None,
                    "/opt/homebrew/Caskroom/codex/0.169.0/bin/codex",
                    &["codex", "--disable", "hooks"],
                ),
            ),
            (
                "OpenCode",
                "2.2.5",
                program(
                    12,
                    None,
                    "/opt/homebrew/Cellar/opencode-v2/2.2.5/bin/opencode",
                    &["opencode", "--standalone"],
                ),
            ),
            (
                "Cursor Agent",
                "2026.09.02-c22c1a3",
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
    fn each_harness_is_known_by_the_shape_its_real_binary_has_and_its_version_by_the_path() {
        for (name, version, program) in measured() {
            let programs = [program];
            let matched = pick(&programs, &on_path).unwrap();
            assert_eq!(matched.known.name, name);
            assert_eq!(matched.version().as_deref(), Some(version), "{name}");
        }
    }

    #[test]
    fn the_registry_ids_are_the_registrys_own() {
        assert_eq!(KNOWN.len(), HARNESSES.len());
        for known in &KNOWN {
            let stem = known.commands[0].split('-').next().unwrap();
            assert!(known.id.contains(stem), "{} / {stem}", known.id);
        }
    }

    #[test]
    fn a_harness_found_through_the_path_is_known_by_its_bare_name_or_its_folder() {
        let bare = program(1, None, "/somewhere/odd/opencode", &["opencode"]);
        assert_eq!(named(&[bare]), Some("OpenCode"));
        let in_a_path_folder = program(
            1,
            None,
            "/Users/x/.local/bin/codex",
            &["/Users/x/.local/bin/codex"],
        );
        assert_eq!(named(&[in_a_path_folder]), Some("Codex"));
    }

    #[test]
    fn a_program_named_like_a_harness_in_a_project_folder_is_not_one() {
        for (exe, args) in [
            // A binary of a project, run by path: not an install layout, not on the `PATH`.
            ("/work/repo/bin/opencode", &["/work/repo/bin/opencode"][..]),
            ("/work/repo/claude", &["./claude", "--serve"]),
            ("/work/codex", &["/work/codex"]),
            // A folder that only carries the product's name, away from an install layout.
            ("/Users/x/code/opencode-ai/node_modules/.bin/tsc", &["tsc"]),
            (
                "/usr/local/bin/node",
                &["node", "/Users/x/code/opencode-ai/scripts/dev.js"],
            ),
            (
                "/usr/local/bin/node",
                &["node", "/Users/x/code/cursor-agent/index.js"],
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
    fn a_host_program_runs_a_harness_only_through_an_install_layout() {
        let installed = program(
            1,
            None,
            "/usr/local/bin/node",
            &[
                "node",
                "/usr/local/lib/node_modules/@openai/codex/bin/codex.js",
            ],
        );
        assert_eq!(named(&[installed]), Some("Codex"));
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
    fn windows_programs_are_known_by_their_layouts_and_by_the_script_node_runs() {
        let native = program(
            1,
            None,
            r"C:\Users\x\.opencode\bin\opencode.exe",
            &[r"C:\Users\x\.opencode\bin\opencode.exe"],
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
        let programs = [shim];
        let matched = pick(&programs, &on_path).unwrap();
        assert_eq!(matched.known.name, "Codex");
        assert!(matched.file.ends_with("codex.js"));
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
        let matched = pick(&tree, &on_path).unwrap();
        assert_eq!((matched.known.name, matched.program.pid), ("Codex", 3));
    }

    #[test]
    fn a_program_between_the_shell_and_the_harness_hides_it() {
        let tree = [
            program(1, None, "/bin/zsh", &["zsh"]),
            program(2, Some(1), "/usr/bin/make", &["make", "serve"]),
            program(3, Some(2), "/x/bin/claude", &["claude"]),
        ];
        assert!(pick(&tree, &on_path).is_none());
    }

    #[test]
    fn a_launcher_that_starts_a_harness_is_looked_through() {
        let tree = [
            program(1, None, "/bin/bash", &["bash"]),
            program(2, Some(1), "/usr/bin/env", &["env", "opencode"]),
            program(3, Some(2), "/opt/x/bin/opencode", &["opencode"]),
        ];
        let matched = pick(&tree, &on_path).unwrap();
        assert_eq!((matched.known.name, matched.program.pid), ("OpenCode", 3));
    }

    #[test]
    fn nothing_in_a_terminal_is_no_harness() {
        assert!(pick(&[], &on_path).is_none());
    }

    #[test]
    fn a_version_is_read_after_the_install_layout_and_nowhere_else() {
        let claude = &KNOWN[0];
        let codex = &KNOWN[1];
        let cursor = &KNOWN[2];
        for (known, path, version) in [
            (
                claude,
                "/h/.local/share/claude/versions/2.1.325",
                Some("2.1.325"),
            ),
            (
                codex,
                "/opt/homebrew/Caskroom/codex/0.169.0/bin/codex",
                Some("0.169.0"),
            ),
            (
                cursor,
                "C:\\Users\\x\\AppData\\Local\\cursor-agent\\versions\\1.2.3-beta.1\\index.js",
                Some("1.2.3-beta.1"),
            ),
            // A node manager's folder is not the harness's version.
            (
                codex,
                "/h/.nvm/versions/node/v22.20.0/lib/node_modules/@openai/codex/bin/codex.js",
                None,
            ),
            (
                codex,
                "/h/.nvm/versions/22.20.0/lib/node_modules/@openai/codex/bin/codex.js",
                None,
            ),
            // No layout, no version.
            (codex, "/h/opencode-v2/1.2.3/bin/opencode", None),
            (codex, "/opt/homebrew/Caskroom/codex/v1.2/bin/codex", None),
        ] {
            assert_eq!(
                version_after_layout(known, path).as_deref(),
                version,
                "{path}"
            );
        }
        // A path cut where the version ends it may have lost part of it.
        let long = format!(
            "{}/.local/share/claude/versions/2.1.3",
            "d".repeat(MAX_ARGUMENT_BYTES)
        );
        assert_eq!(version_after_layout(claude, &long), None);
    }

    #[test]
    fn an_npm_installed_harness_runs_as_node_and_the_link_it_made_in_a_path_folder() {
        // Measured on macOS and Linux: `node <the link as invoked> args`.
        let installed = program(
            1,
            None,
            "/usr/local/bin/node",
            &["node", "/Users/x/.local/bin/opencode", "--flag"],
        );
        assert_eq!(named(&[installed]), Some("OpenCode"));
        // The same name in a project folder away from the `PATH` is no harness.
        let project = program(
            1,
            None,
            "/usr/local/bin/node",
            &["node", "/work/repo/bin/opencode", "--flag"],
        );
        assert_eq!(named(&[project]), None);
        let project_js = program(
            1,
            None,
            "/usr/local/bin/node",
            &["node", "/work/repo/bin/opencode.js"],
        );
        assert_eq!(named(&[project_js]), None);
    }
}
