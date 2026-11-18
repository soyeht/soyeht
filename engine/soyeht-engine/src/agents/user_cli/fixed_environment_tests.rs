//! The variables the engine fixes for the second adapter.

use super::tests::{adapter, sent_to_a_supervisor_that_refuses};
use super::*;

#[test]
fn the_second_adapter_is_given_its_fixed_variable_and_a_supervisor_that_refuses_it_gets_none() {
    let profile = soyeht_platform::testing::scratch_dir("user-cli-fixed");
    let index = super::super::adapter_install::LOCKED
        .iter()
        .position(|locked| !locked.environment.is_empty())
        .unwrap();
    super::super::adapter_install::seed_for_test(&profile, index);
    let package = super::super::adapter_install::launch_argv_for_test(index)[2].clone();
    let (name, value) = soyeht_ptyd::CODEX_PROMPT_SUGGESTIONS_OFF;

    let given = prepare(adapter(&package, ProcessKind::PipesAcp), &profile).unwrap();
    assert_eq!(given.config.env.get(name).map(String::as_str), Some(value));
    assert_eq!(given.plain.as_ref().map(|plain| plain.env.len()), Some(0));
    // A supervisor that does not know the variable starts it without.
    let (sent, _) = sent_to_a_supervisor_that_refuses(given, name);
    assert_eq!(sent, [true, false]);

    // A sign-in terminal is no adapter process, and a launch that set the variable keeps it.
    let terminal = prepare(adapter(&package, ProcessKind::Pty), &profile).unwrap();
    assert!(terminal.config.env.is_empty() && terminal.plain.is_none());
    let mut own = adapter(&package, ProcessKind::PipesAcp);
    own.env.insert(name.into(), "true".into());
    let kept = prepare(own.clone(), &profile).unwrap();
    assert_eq!((kept.config.env, kept.plain), (own.env, None));
    std::fs::remove_dir_all(profile).unwrap();
}
