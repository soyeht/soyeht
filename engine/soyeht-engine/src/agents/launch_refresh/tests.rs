use super::*;
use crate::agents::registry::builtin_entries;
use std::fs;

/// A pane saved under an older pin of a locked adapter starts the catalogue's adapter, from the
/// lock, and not the saved package through `npx`.
#[test]
fn a_pane_saved_under_an_older_pin_follows_the_catalogue_to_the_installed_adapter() {
    let profile = soyeht_platform::testing::scratch_dir("registry-older-pin");
    let entries = builtin_entries().unwrap();
    for (index, locked) in adapter_install::LOCKED.iter().enumerate() {
        let package = format!("{}@{}", locked.package, locked.version);
        let current = entries
            .iter()
            .map(|entry| &entry.launch)
            .find(|launch| launch.args == ["--yes", package.as_str()])
            .unwrap();
        let mut saved = current.clone();
        saved.args = vec!["--yes".into(), format!("{}@0.0.1", locked.package)];
        saved.version = "0.0.1".into();
        let spawn = |launch: &HarnessLaunch| soyeht_ptyd::wire::Spawn {
            instance_key: "key".into(),
            generation: 1,
            kind: soyeht_ptyd::wire::ProcessKind::PipesAcp,
            cwd: "/".into(),
            argv: std::iter::once(launch.command.clone())
                .chain(launch.args.iter().cloned())
                .collect(),
            env: Default::default(),
            cols: 80,
            rows: 24,
        };
        // The saved launch of a built-in pane is not started unlocked.
        adapter_install::seed_for_test(&profile, index);
        assert!(adapter_install::refuse_stale_builtin(&saved).is_err());
        // The catalogue's entry is read as written, whether or not `npx` is on this machine's path.
        let read = refreshed_launch(&profile, &saved).unwrap();
        assert_eq!(read.args, current.args);
        let followed = followed(&saved, current).unwrap();
        assert_eq!(
            (&followed.args, &followed.version),
            (&current.args, &current.version)
        );
        assert_eq!(
            (&followed.id, &followed.command),
            (&saved.id, &saved.command)
        );
        let started = adapter_install::installed_spawn(&profile, spawn(&followed)).unwrap();
        assert_eq!(started.argv[0], "node", "{package}");
        // Nothing to follow when it is already the catalogue's, or for a custom entry.
        assert_eq!(super::followed(current, current), None);
        let mut custom = saved.clone();
        custom.id = "custom.agent".into();
        assert_eq!(super::followed(&custom, current), None);
    }
    let gemini = entries
        .iter()
        .map(|entry| entry.launch.clone())
        .find(|launch| launch.id == "registry.gemini")
        .unwrap();
    let mut older = gemini.clone();
    older.version = "0.0.1".into();
    older.args = vec!["--yes".into(), "@google/gemini-cli@0.0.1".into()];
    // An adapter that is not locked keeps the acknowledgement the person gave it.
    assert_eq!(super::followed(&older, &gemini), None);
    fs::remove_dir_all(profile).unwrap();
}
