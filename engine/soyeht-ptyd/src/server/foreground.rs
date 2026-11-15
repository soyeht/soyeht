//! `Foreground`: what each terminal the supervisor holds runs in its foreground (ADR 0165).
//!
//! The question is answered in two steps so that the reading never holds the supervisor's state:
//! [`collect`] copies the terminals that are asked about while the state is held, and [`answer`]
//! reads them after it was released.

use super::{Live, State, error, lock};
use crate::{
    wire::{Foreground, MAX_FOREGROUND, MAX_FOREGROUND_BYTES, Program, Response},
    wire_kind,
};
use soyeht_platform::log;
use std::sync::{Arc, atomic::Ordering};

/// The log event of a terminal whose foreground could not be read.
const UNREADABLE: &str = "ptyd.foreground_unreadable";
/// What each program of an answer costs besides its strings, for the budget.
const PROGRAM_OVERHEAD: usize = 64;

/// The live terminals among `instance_ids`, or the refusal of a question that names too many. An
/// id the supervisor does not know, or that is not a terminal, is left out.
pub(super) fn collect(
    state: &State,
    instance_ids: &[String],
) -> std::result::Result<Vec<(String, Arc<Live>)>, Response> {
    if instance_ids.len() > MAX_FOREGROUND {
        return Err(error(wire_kind::INVALID_PARAMS, "too many terminals"));
    }
    Ok(instance_ids
        .iter()
        .filter_map(|instance_id| {
            let live = state.live.get(instance_id)?;
            live.resizer.as_ref()?;
            Some((instance_id.clone(), Arc::clone(live)))
        })
        .collect())
}

/// The bytes a program takes in an answer, for the budget.
fn cost(program: &Program) -> usize {
    PROGRAM_OVERHEAD
        + program.executable.as_ref().map_or(0, String::len)
        + program.arguments.iter().map(String::len).sum::<usize>()
}

/// The programs that fit in `room` bytes, in order, and whether some were left out.
fn fit(programs: Vec<Program>, room: &mut usize) -> (Vec<Program>, bool) {
    let mut kept = Vec::with_capacity(programs.len());
    for program in programs {
        let needed = cost(&program);
        if needed > *room {
            return (kept, true);
        }
        *room -= needed;
        kept.push(program);
    }
    (kept, false)
}

/// The programs of each terminal `asked`, within [`MAX_FOREGROUND_BYTES`]: a terminal whose
/// programs do not all fit comes with those that do and `truncated`, and once the budget is spent
/// the terminals after it are left out (the engine keeps what it knew of them). A terminal whose
/// process ended answers with no programs.
pub(super) fn answer(asked: std::result::Result<Vec<(String, Arc<Live>)>, Response>) -> Response {
    let asked = match asked {
        Ok(asked) => asked,
        Err(refusal) => return refusal,
    };
    let mut answers = Vec::with_capacity(asked.len());
    let mut room = MAX_FOREGROUND_BYTES;
    for (instance_id, live) in asked {
        if room == 0 {
            break;
        }
        let programs: Vec<Program> = if live.exited.load(Ordering::Acquire) {
            Vec::new()
        } else {
            let read = match lock(&live.child, "child") {
                Ok(child) => live
                    .resizer
                    .as_ref()
                    .map(|terminal| terminal.foreground(&child)),
                Err(failure) => {
                    return error(wire_kind::OPERATION_FAILED, &failure.to_string());
                }
            };
            match read {
                Some(Ok(programs)) => {
                    live.foreground_unreadable.store(false, Ordering::Relaxed);
                    programs.into_iter().map(Into::into).collect()
                }
                Some(Err(failure)) => {
                    // The first failure of a terminal is logged; the ones that follow are not
                    // until a read succeeds again.
                    if !live.foreground_unreadable.swap(true, Ordering::Relaxed) {
                        log::failure(UNREADABLE, &failure);
                    }
                    continue;
                }
                None => continue,
            }
        };
        let (kept, truncated) = fit(programs, &mut room);
        answers.push(Foreground {
            instance_id,
            programs: kept,
            truncated,
        });
    }
    Response::Foregrounds(answers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{MAX_FOREGROUND, MAX_FOREGROUND_BYTES};
    use soyeht_platform::process::inspect::{
        MAX_ARGUMENT_BYTES, MAX_ARGUMENTS, MAX_EXECUTABLE_BYTES, MAX_PROGRAMS,
    };

    /// The largest program the platform reports, every character one that JSON writes as six.
    fn largest() -> Program {
        Program {
            pid: u32::MAX,
            parent_pid: Some(u32::MAX),
            executable: Some("\u{1}".repeat(MAX_EXECUTABLE_BYTES)),
            arguments: vec!["\u{1}".repeat(MAX_ARGUMENT_BYTES); MAX_ARGUMENTS],
        }
    }

    #[test]
    fn a_program_costs_its_strings_and_a_fixed_part() {
        let program = Program {
            pid: 1,
            parent_pid: None,
            executable: Some("x".repeat(10)),
            arguments: vec!["y".repeat(10); 2],
        };
        assert_eq!(cost(&program), PROGRAM_OVERHEAD + 30);
    }

    #[test]
    fn programs_that_do_not_fit_are_left_out_and_the_answer_says_so() {
        let one = cost(&largest());
        let mut room = one * 2 + one / 2;
        let (kept, truncated) = fit(vec![largest(); 5], &mut room);
        assert_eq!(kept.len(), 2);
        assert!(truncated);
        assert_eq!(room, one / 2);
        let (all, truncated) = fit(vec![largest(); 1], &mut (one * 2));
        assert_eq!((all.len(), truncated), (1, false));
    }

    #[test]
    fn the_largest_possible_answer_stays_inside_a_frame() {
        // Every terminal asked about runs the most programs of the largest size.
        let mut room = MAX_FOREGROUND_BYTES;
        let mut answers = Vec::new();
        for number in 0..MAX_FOREGROUND {
            if room == 0 {
                break;
            }
            let (programs, truncated) = fit(vec![largest(); MAX_PROGRAMS], &mut room);
            answers.push(Foreground {
                instance_id: format!("instance-{number}"),
                programs,
                truncated,
            });
        }
        let written = serde_json::to_vec(&Response::Foregrounds(answers)).unwrap();
        assert!(
            written.len() < soyeht_protocol::limits::JSON_BYTES,
            "{} bytes",
            written.len()
        );
    }
}
