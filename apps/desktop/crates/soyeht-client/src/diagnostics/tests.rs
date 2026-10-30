use super::*;
use soyeht_platform::log::Failure as _;
use std::path::PathBuf;

fn texts(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

#[test]
fn the_program_is_asked_for_the_profile_and_directory_the_window_resolved() {
    assert_eq!(
        arguments("prod", Path::new("/data/Soyeht.next")),
        texts(&[
            "--profile",
            "prod",
            "--data-dir",
            "/data/Soyeht.next",
            "diagnostics"
        ])
    );
}

#[test]
fn the_program_is_the_soyeht_file_in_the_folder_of_the_window() {
    let folder =
        std::env::temp_dir().join(format!("next-diagnostics-beside-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let window = folder.join("soyeht-desktop");
    assert_eq!(program_beside(&window), Err(Failure::Unavailable));
    let program = program_file(&folder, PROGRAM);
    std::fs::write(&program, "").unwrap();
    assert_eq!(program_beside(&window), Ok(program));
    std::fs::remove_dir_all(&folder).unwrap();
}

#[cfg(unix)]
#[test]
fn the_report_is_what_the_soyeht_program_beside_the_window_prints() {
    // A script that prints its arguments stands in for the program. A link to `echo` would not
    // do: on some systems `echo` is a link to one multi-call program that reads its own name.
    let folder =
        std::env::temp_dir().join(format!("next-diagnostics-collect-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    write_printing_script(&program_file(&folder, PROGRAM));
    let report = collect_with(
        &folder.join("soyeht-desktop"),
        "prod",
        Path::new("/data/Soyeht.next"),
    );
    std::fs::remove_dir_all(&folder).unwrap();
    assert_eq!(
        report,
        Ok("--profile prod --data-dir /data/Soyeht.next diagnostics\n".to_owned())
    );
}

/// Writes an executable `/bin/sh` script at `path` that prints its arguments, as `echo` does.
/// Every Unix has `/bin/sh`; not every one has `/bin/echo` (NixOS).
#[cfg(unix)]
fn write_printing_script(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, "#!/bin/sh\necho \"$@\"\n").unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn a_window_with_no_soyeht_program_beside_it_has_no_report() {
    let folder = std::env::temp_dir().join(format!("next-diagnostics-none-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let report = collect_with(&folder.join("soyeht-desktop"), "prod", Path::new("/data"));
    std::fs::remove_dir_all(&folder).unwrap();
    assert_eq!(report, Err(Failure::Unavailable));
}

#[cfg(unix)]
mod running {
    use super::*;

    fn shell(script: &str) -> Result<String, super::super::Failure> {
        run(Path::new("/bin/sh"), &texts(&["-c", script]))
    }

    #[test]
    fn what_the_program_prints_is_the_report_unchanged() {
        let report = "## Soyeht diagnostics\n\n- engine: not running\n";
        assert_eq!(
            shell(&format!("printf '%s' '{report}'")),
            Ok(report.to_owned())
        );
    }

    #[test]
    fn the_arguments_reach_the_program_in_order() {
        let given = arguments("dev", Path::new("/data/Soyeht.dev"));
        let mut all = texts(&["-c", "echo \"$@\"", "sh"]);
        all.extend(given);
        assert_eq!(
            run(Path::new("/bin/sh"), &all),
            Ok("--profile dev --data-dir /data/Soyeht.dev diagnostics\n".to_owned())
        );
    }

    #[test]
    fn a_program_that_fails_is_a_failure_with_its_status_and_no_text() {
        let failure = shell("echo 'oops /Users/someone/secret' >&2; exit 3").unwrap_err();
        assert_eq!(failure, super::super::Failure::Run("status:3".into()));
        assert!(!failure.failure_kind().contains("someone"));
    }

    #[test]
    fn a_program_that_prints_no_text_is_a_failure() {
        assert_eq!(
            shell("printf '\\377\\376'"),
            Err(super::super::Failure::NotText)
        );
    }

    #[test]
    fn a_program_that_cannot_start_is_a_failure() {
        let missing = PathBuf::from("/nonexistent/soyeht");
        assert!(matches!(
            run(&missing, &[]),
            Err(super::super::Failure::Run(kind)) if kind.starts_with("spawn:")
        ));
    }
}

#[test]
fn the_action_goes_from_collecting_to_copied() {
    let mut phase = Phase::default();
    assert_eq!(phase.message(), None);
    assert!(phase.begin());
    assert!(phase.working());
    assert_eq!(phase.message(), Some(DIAGNOSTICS_WORKING));
    assert!(!phase.begin(), "one collection at a time");
    phase.finish(Ok(()));
    assert_eq!(phase.message(), Some(DIAGNOSTICS_COPIED));
    assert!(!phase.is_failure());
    assert!(phase.begin(), "the person may copy again");
}

#[test]
fn a_failure_is_said_and_never_looks_like_a_copy() {
    let mut phase = Phase::default();
    assert!(phase.begin());
    phase.finish(Err(Failure::Run("timed_out".into())));
    assert!(phase.is_failure());
    assert_eq!(phase.message(), Some(DIAGNOSTICS_FAILED));
    phase.finish(Err(Failure::NotText));
    assert_eq!(phase.message(), Some(DIAGNOSTICS_FAILED));
    phase.finish(Err(Failure::Unavailable));
    assert_eq!(phase.message(), Some(DIAGNOSTICS_UNAVAILABLE));
}

#[test]
fn a_failure_logs_a_kind_and_never_a_message() {
    assert_eq!(Failure::Unavailable.failure_kind(), "unavailable");
    assert_eq!(Failure::Profile.failure_kind(), "profile");
    assert_eq!(
        Failure::Run("timed_out".into()).failure_kind(),
        "run:timed_out"
    );
    assert_eq!(Failure::NotText.failure_kind(), "not_text");
}
