//! What a running process is: its executable and its arguments, and which of a terminal's
//! processes is in its foreground.
//!
//! The supervisor uses it to say what a pane runs (ADR 0165). On macOS and Linux the foreground of
//! a terminal is the process group `tcgetpgrp` names, and [`foreground_of_terminal`] reads the
//! group's leader. On Windows a pseudo console has no foreground group; the job object that holds
//! the pane's processes lists them instead (`windows::Spawned::programs`), in tree order. Both
//! give [`Program`]s, so everything above reads one shape.

#[cfg(unix)]
mod unix;

#[cfg(unix)]
pub use unix::foreground_of_terminal;

/// The most arguments of a program that are kept: a harness is told by its first few.
pub const MAX_ARGUMENTS: usize = 32;
/// The most bytes of one argument that are kept. A longer one is cut at a character boundary.
pub const MAX_ARGUMENT_BYTES: usize = 1000;
/// The most processes one report holds. A job with more is cut, shallowest first.
pub const MAX_PROGRAMS: usize = 64;

/// A process, as far as the system lets this one read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub pid: u32,
    /// The parent's pid when it is one of the same report (a Windows job holds a tree); `None`
    /// for the first process of a report and always for a process group's leader.
    pub parent: Option<u32>,
    /// The file the process runs, when the system tells.
    pub executable: Option<String>,
    /// The program's own `argv`, `argv[0]` first, at most [`MAX_ARGUMENTS`].
    pub arguments: Vec<String>,
}

/// Keeps at most [`MAX_ARGUMENTS`] arguments of at most [`MAX_ARGUMENT_BYTES`] bytes each.
pub fn bounded(arguments: impl IntoIterator<Item = String>) -> Vec<String> {
    arguments
        .into_iter()
        .take(MAX_ARGUMENTS)
        .map(|mut argument| {
            if argument.len() > MAX_ARGUMENT_BYTES {
                let mut end = MAX_ARGUMENT_BYTES;
                while !argument.is_char_boundary(end) {
                    end -= 1;
                }
                argument.truncate(end);
            }
            argument
        })
        .collect()
}

/// A process as the system lists a job's members, before the tree is built.
#[cfg(any(windows, test))]
#[derive(Clone, Debug)]
pub(crate) struct Listed {
    pub(crate) program: Program,
    pub(crate) parent_pid: u32,
    /// When the process was created, in the system's own unit (only the order matters).
    pub(crate) created: u64,
}

/// Puts the members of a job in tree order: a process before its children, the oldest first among
/// siblings. A parent counts only when it is in the list and was created no later than the
/// child: a process id is reused, so a number that now names a younger process is not the
/// parent. Without a parent that counts, a process is a root.
#[cfg(any(windows, test))]
pub(crate) fn tree_order(mut listed: Vec<Listed>) -> Vec<Program> {
    listed.sort_by_key(|member| (member.created, member.program.pid));
    let created_of = |pid: u32| {
        listed
            .iter()
            .find(|member| member.program.pid == pid)
            .map(|member| member.created)
    };
    let parents: Vec<Option<u32>> = listed
        .iter()
        .map(|member| {
            created_of(member.parent_pid)
                .filter(|parent_created| *parent_created <= member.created)
                .map(|_| member.parent_pid)
                .filter(|parent| *parent != member.program.pid)
        })
        .collect();
    let mut ordered: Vec<Program> = Vec::with_capacity(listed.len());
    let mut placed = vec![false; listed.len()];
    // Level by level: the roots, then everything whose parent is placed, and so on. A cycle
    // cannot form (a parent is never younger), so the loop ends when a level adds nothing.
    loop {
        let level: Vec<usize> = (0..listed.len())
            .filter(|index| !placed[*index])
            .filter(|index| match parents[*index] {
                None => true,
                Some(parent) => ordered.iter().any(|done| done.pid == parent),
            })
            .collect();
        if level.is_empty() {
            break;
        }
        for index in level {
            placed[index] = true;
            let mut program = listed[index].program.clone();
            program.parent = parents[index];
            ordered.push(program);
        }
        if ordered.len() >= MAX_PROGRAMS {
            ordered.truncate(MAX_PROGRAMS);
            break;
        }
    }
    ordered
}

/// Splits a Windows command line into arguments by the rules of the C runtime, which every
/// ordinary program follows: a quote groups, `2n` backslashes before a quote are `n`, and `2n+1`
/// are `n` and a literal quote.
#[cfg(any(windows, test))]
pub(crate) fn split_command_line(line: &[u16]) -> Vec<String> {
    const SPACE: u16 = b' ' as u16;
    const TAB: u16 = b'\t' as u16;
    const QUOTE: u16 = b'"' as u16;
    const BACKSLASH: u16 = b'\\' as u16;
    let mut arguments = Vec::new();
    let mut current: Vec<u16> = Vec::new();
    let mut started = false;
    let mut quoted = false;
    let mut index = 0;
    while index < line.len() {
        let unit = line[index];
        match unit {
            BACKSLASH => {
                let mut run = 0;
                while line.get(index + run) == Some(&BACKSLASH) {
                    run += 1;
                }
                started = true;
                if line.get(index + run) == Some(&QUOTE) {
                    current.extend(std::iter::repeat_n(BACKSLASH, run / 2));
                    if run % 2 == 1 {
                        current.push(QUOTE);
                        index += run + 1;
                    } else {
                        index += run;
                    }
                } else {
                    current.extend(std::iter::repeat_n(BACKSLASH, run));
                    index += run;
                }
            }
            QUOTE => {
                quoted = !quoted;
                started = true;
                index += 1;
            }
            SPACE | TAB if !quoted => {
                if started {
                    arguments.push(String::from_utf16_lossy(&current));
                    current.clear();
                    started = false;
                }
                index += 1;
            }
            _ => {
                current.push(unit);
                started = true;
                index += 1;
            }
        }
    }
    if started {
        arguments.push(String::from_utf16_lossy(&current));
    }
    arguments
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(pid: u32, parent_pid: u32, created: u64, name: &str) -> Listed {
        Listed {
            program: Program {
                pid,
                parent: None,
                executable: Some(name.into()),
                arguments: vec![name.into()],
            },
            parent_pid,
            created,
        }
    }

    fn order_of(programs: &[Program]) -> Vec<(u32, Option<u32>)> {
        programs
            .iter()
            .map(|program| (program.pid, program.parent))
            .collect()
    }

    #[test]
    fn a_job_is_listed_parents_first_and_the_oldest_first_among_siblings() {
        // The shell (10) started claude (30) and, before it, a helper (20); claude started a
        // server (40). The system listed them in no useful order.
        let tree = tree_order(vec![
            listed(40, 30, 400, "server"),
            listed(30, 10, 300, "claude"),
            listed(10, 1, 100, "shell"),
            listed(20, 10, 200, "helper"),
        ]);
        assert_eq!(
            order_of(&tree),
            [(10, None), (20, Some(10)), (30, Some(10)), (40, Some(30))]
        );
    }

    #[test]
    fn a_parent_id_that_names_a_younger_process_is_not_the_parent() {
        // The process 50 was started by a parent that has since ended; its number now names 60,
        // which was created later and so cannot have started it.
        let tree = tree_order(vec![
            listed(60, 1, 600, "other"),
            listed(50, 60, 500, "orphan"),
        ]);
        assert_eq!(order_of(&tree), [(50, None), (60, None)]);
    }

    #[test]
    fn a_process_that_names_itself_as_its_parent_is_a_root() {
        let tree = tree_order(vec![listed(7, 7, 10, "odd")]);
        assert_eq!(order_of(&tree), [(7, None)]);
    }

    #[test]
    fn a_big_job_is_cut_at_the_report_limit_with_the_shallowest_first() {
        let many = (1..=(MAX_PROGRAMS as u32 + 20))
            .map(|pid| listed(pid, pid - 1, u64::from(pid), "p"))
            .collect();
        let tree = tree_order(many);
        assert_eq!(tree.len(), MAX_PROGRAMS);
        assert_eq!(tree[0].pid, 1);
    }

    #[test]
    fn arguments_are_bounded_in_number_and_in_length_at_a_character_boundary() {
        let long = "é".repeat(MAX_ARGUMENT_BYTES);
        let kept = bounded(std::iter::once(long).chain((0..100).map(|number| number.to_string())));
        assert_eq!(kept.len(), MAX_ARGUMENTS);
        assert!(kept[0].len() <= MAX_ARGUMENT_BYTES);
        assert!(kept[0].chars().all(|character| character == 'é'));
    }

    fn split(line: &str) -> Vec<String> {
        split_command_line(&line.encode_utf16().collect::<Vec<_>>())
    }

    #[test]
    fn a_windows_command_line_splits_like_the_c_runtime() {
        assert_eq!(
            split(r#""C:\Program Files\nodejs\node.exe" C:\npm\codex.js --flag"#),
            [
                r"C:\Program Files\nodejs\node.exe",
                r"C:\npm\codex.js",
                "--flag"
            ]
        );
        assert_eq!(split(r#"a "b c" d\ e"#), ["a", "b c", r"d\", "e"]);
        assert_eq!(split(r#"x "say \"hi\"" y"#), ["x", r#"say "hi""#, "y"]);
        assert_eq!(split(r#"x "dir\\" y"#), ["x", r"dir\", "y"]);
        assert_eq!(split(r#"x "" y"#), ["x", "", "y"]);
        assert!(split("   ").is_empty());
    }
}
