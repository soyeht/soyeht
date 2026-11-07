use super::*;
use std::{
    env, fs,
    path::{Path, PathBuf},
    time::Duration,
};

/// A machine whose `git` is a launcher that records that it ran. The launcher stands for
/// the macOS stub that opens the installer.
struct Launcher {
    root: PathBuf,
}

impl Launcher {
    fn new(label: &str) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let root = env::temp_dir().join(format!("soyeht-run-git-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("usr/bin")).unwrap();
        let script = format!("#!/bin/sh\ntouch '{}'\n", root.join("ran").display());
        let launcher = root.join("usr/bin/git");
        fs::write(&launcher, script).unwrap();
        fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).unwrap();
        Self { root }
    }

    fn probe(&self) -> Probe {
        Probe {
            launcher: Some(self.root.join("usr/bin/git")),
            path: self.root.join("usr/bin").into_os_string(),
            developer_dir: None,
            select_link: self.root.join("xcode_select_link"),
            default_tools: self.root.join("Library/CommandLineTools"),
        }
    }

    fn ran(&self) -> bool {
        self.root.join("ran").exists()
    }
}

impl Drop for Launcher {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn git_is_not_started_when_the_developer_tools_are_missing() {
    let machine = Launcher::new("missing");
    let limits = Limits::new(Duration::from_secs(10), 8192);
    let error = run_git_with(&machine.probe(), Command::new("git"), limits).unwrap_err();
    assert!(matches!(error, Error::GitUnavailable), "{error}");
    assert!(matches!(
        require_git_with(&machine.probe()),
        Err(Error::GitUnavailable)
    ));
    assert!(!machine.ran(), "the launcher must never start");
}

#[test]
fn git_starts_when_the_developer_tools_are_present() {
    use std::os::unix::fs::PermissionsExt;
    let machine = Launcher::new("present");
    let real = machine.root.join("Library/CommandLineTools/usr/bin/git");
    fs::create_dir_all(real.parent().unwrap()).unwrap();
    fs::write(&real, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&real, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = Command::new(machine.root.join("usr/bin/git"));
    command.arg("--version");
    let limits = Limits::new(Duration::from_secs(10), 8192);
    run_git_with(&machine.probe(), command, limits).unwrap();
    assert!(machine.ran());
}

#[test]
fn only_the_repositorys_own_entries_name_drivers_and_includes() {
    let listing = b"global\0file:/home/p/.gitconfig\0filter.lfs.clean\ngit-lfs clean\0\
        local\0file:.git/config\0filter.a.b.process\ncat\0\
        local\0file:.git/config\0filter.x\0\
        local\0file:.git/config\0include.path\n../extra.cfg\0\
        local\0file:.git/config\0includeif.gitdir:/r/**.path\n~/more.cfg\0";
    let entries = config_entries(listing).unwrap();
    assert_eq!(entries.len(), 5);
    let own: Vec<_> = entries
        .iter()
        .filter(|entry| entry.scope == "local")
        .collect();
    let names: Vec<_> = own
        .iter()
        .filter_map(|entry| driver_name(&entry.key))
        .collect();
    assert_eq!(names, ["a.b"], "a key with no subsection names none");
    let repository = Path::new("/work/repository");
    let targets: Vec<_> = own
        .iter()
        .map(|entry| include_target(repository, entry).unwrap())
        .collect();
    let targets: Vec<_> = targets.into_iter().flatten().collect();
    assert_eq!(targets[0], Path::new("/work/repository/.git/../extra.cfg"));
    assert_eq!(targets[1], std::env::home_dir().unwrap().join("more.cfg"));
    // Output that is not text, or that stops in the middle of an entry, is refused.
    assert!(config_entries(b"local\0file:x\0\xff\0").is_none());
    assert!(config_entries(b"local\0file:x").is_none());
    assert_eq!(config_entries(b"").unwrap(), []);
}

#[test]
fn the_version_of_git_is_read_from_its_first_line() {
    assert_eq!(
        git_version("git version 2.50.1 (Apple Git-155)\n"),
        Some((2, 50))
    );
    assert_eq!(git_version("git version 2.25.1"), Some((2, 25)));
    assert_eq!(git_version("git version 2.31.0.windows.1"), Some((2, 31)));
    assert_eq!(git_version("something else"), None);
}

/// `git` in `root` with an identity, for a test's setup. Its standard input is closed: a
/// program a configuration names reads it.
fn git_in(root: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "{arguments:?}");
}

/// Adds the worktree `name` to the repository at `root`, with the settings of
/// [`ReadOnlyGit::harden_environment`] when `hardened`.
fn add_worktree(root: &Path, name: &str, hardened: bool) {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(root)
        .args(["worktree", "add", "-q", "-b", name])
        .arg(root.join(name));
    if hardened {
        let limits = Limits::new(Duration::from_secs(30), 1 << 20);
        ReadOnlyGit::prepare(root, limits)
            .unwrap()
            .harden_environment(&mut command)
            .unwrap();
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A repository whose own configuration names a program for each way git starts one while it
/// reads: the file system monitor and the three programs of a filter driver. Each program
/// leaves a file named for it in `root`.
fn repository_with_programs(label: &str) -> PathBuf {
    let root = env::temp_dir().join(format!("soyeht-read-only-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let git = |arguments: &[&str]| git_in(&root, arguments);
    git(&["init", "-q"]);
    fs::write(root.join(".gitattributes"), "*.bin filter=probe\n").unwrap();
    fs::write(root.join("a.bin"), "one\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "baseline"]);
    let program = |name: &str| format!("sh -c 'touch \"{}/ran-{name}\"; cat' --", root.display());
    for (key, name) in [
        ("filter.probe.clean", "clean"),
        ("filter.probe.smudge", "smudge"),
        ("core.fsmonitor", "fsmonitor"),
    ] {
        git(&["config", key, &program(name)]);
    }
    git(&["config", "filter.probe.process", &program("process")]);
    // A driver name with dots is listed whole.
    git(&["config", "filter.dotted.name.clean", &program("dotted")]);
    fs::write(root.join("a.bin"), "two\n").unwrap();
    fs::write(root.join("untracked.txt"), "new\n").unwrap();
    root
}

fn programs_that_ran(root: &Path) -> Vec<String> {
    let mut ran: Vec<String> = fs::read_dir(root)
        .unwrap()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with("ran-"))
        .collect();
    ran.sort();
    ran
}

#[test]
fn a_read_only_command_starts_none_of_the_programs_a_repository_names() {
    let limits = Limits::new(Duration::from_secs(30), 1 << 20);
    let root = repository_with_programs("commands");
    let commands: [&[&str]; 4] = [
        &[
            "diff",
            "--no-ext-diff",
            "--name-only",
            "-z",
            "HEAD",
            "--",
            ".",
        ],
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--unified=200",
            "HEAD",
            "--",
            "a.bin",
        ],
        &["ls-files", "--others", "--exclude-standard", "-z"],
        &["status", "--porcelain"],
    ];

    // The same commands without the hardening do start them, so the check below can fail.
    for arguments in commands {
        let status = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(arguments)
            .output()
            .unwrap();
        assert!(status.status.success(), "{arguments:?}");
    }
    assert!(
        !programs_that_ran(&root).is_empty(),
        "the control started nothing"
    );
    for entry in programs_that_ran(&root) {
        fs::remove_file(root.join(entry)).unwrap();
    }

    for arguments in commands {
        let mut command = ReadOnlyGit::prepare(&root, limits).unwrap().command();
        command.args(arguments);
        let output = run_git(command, limits).unwrap();
        assert!(output.success(), "{arguments:?}: {}", output.stderr_text());
    }
    let ran = programs_that_ran(&root);
    fs::remove_dir_all(&root).unwrap();
    assert!(ran.is_empty(), "{ran:?}");
}

/// Worktrunk runs git itself, so the settings must reach the git it starts through the
/// environment. The same checkout without them runs the repository's hook and programs.
#[test]
fn the_environment_carries_the_settings_to_a_git_another_program_starts() {
    let limits = Limits::new(Duration::from_secs(30), 1 << 20);
    let root = repository_with_programs("environment");
    let hook = root.join(".git/hooks/post-checkout");
    fs::write(
        &hook,
        format!("#!/bin/sh\ntouch \"{}/ran-hook\"\n", root.display()),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }
    add_worktree(&root, "control", false);
    assert!(
        programs_that_ran(&root).contains(&"ran-hook".to_owned()),
        "the control ran no hook"
    );
    for entry in programs_that_ran(&root) {
        fs::remove_file(root.join(entry)).unwrap();
    }
    add_worktree(&root, "hardened", true);
    let mut status = Command::new("git");
    status.arg("-C").arg(&root).args(["status", "--porcelain"]);
    ReadOnlyGit::prepare(&root, limits)
        .unwrap()
        .harden_environment(&mut status)
        .unwrap();
    assert!(status.output().unwrap().status.success());
    let ran = programs_that_ran(&root);
    fs::remove_dir_all(&root).unwrap();
    assert!(ran.is_empty(), "{ran:?}");
}

/// A driver defined in a file the configuration includes, under a condition that holds only
/// inside a worktree that does not exist yet, is cleared for that worktree too. The control
/// shows the driver does run in the worktree without the settings.
#[test]
fn a_driver_of_an_included_file_is_cleared_whatever_the_condition_of_the_include() {
    let root = fs::canonicalize(repository_with_programs("included")).unwrap();
    let git = |arguments: &[&str]| git_in(&root, arguments);
    // Each driver is a script beside the repository's files: a program with a `;` in a
    // config file would be read as a comment.
    for name in ["late", "deep"] {
        let script = root.join(format!("{name}.sh"));
        fs::write(
            &script,
            format!("#!/bin/sh\ntouch '{}/ran-{name}'\ncat\n", root.display()),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    fs::write(
        root.join("extra.cfg"),
        format!(
            "[filter \"late\"]\n\tsmudge = {}/late.sh\n[include]\n\tpath = deeper.cfg\n",
            root.display()
        ),
    )
    .unwrap();
    fs::write(
        root.join("deeper.cfg"),
        format!("[filter \"deep\"]\n\tsmudge = {}/deep.sh\n", root.display()),
    )
    .unwrap();
    git(&[
        "config",
        &format!("includeIf.gitdir:{}/.git/worktrees/**.path", root.display()),
        "../extra.cfg",
    ]);
    fs::write(
        root.join(".gitattributes"),
        "*.late filter=late\n*.deep filter=deep\n",
    )
    .unwrap();
    fs::write(root.join("a.late"), "one\n").unwrap();
    fs::write(root.join("a.deep"), "one\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "included"]);
    for entry in programs_that_ran(&root) {
        fs::remove_file(root.join(entry)).unwrap();
    }
    add_worktree(&root, "control", false);
    let started = programs_that_ran(&root);
    for name in ["ran-late", "ran-deep"] {
        assert!(
            started.contains(&name.to_owned()),
            "the control did not run {name}: {started:?}"
        );
    }
    for entry in started {
        fs::remove_file(root.join(entry)).unwrap();
    }
    add_worktree(&root, "hardened", true);
    let ran = programs_that_ran(&root);
    fs::remove_dir_all(&root).unwrap();
    assert!(ran.is_empty(), "{ran:?}");
}

/// The person's own drivers (git-lfs) are not the repository's: only the repository's are
/// cleared.
#[test]
fn a_repository_without_drivers_of_its_own_clears_none() {
    let limits = Limits::new(Duration::from_secs(30), 1 << 20);
    let root = env::temp_dir().join(format!("soyeht-read-only-plain-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let status = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["init", "-q"])
        .status()
        .unwrap();
    assert!(status.success());
    let git = ReadOnlyGit::prepare(&root, limits).unwrap();
    fs::remove_dir_all(&root).unwrap();
    assert_eq!(
        git.settings
            .iter()
            .filter(|setting| setting.starts_with("filter."))
            .count(),
        0,
        "{:?}",
        git.settings
    );
}

#[test]
fn the_settings_come_after_the_ones_the_environment_carries() {
    let git = ReadOnlyGit {
        repository: PathBuf::from("/r"),
        settings: vec!["core.fsmonitor=false".to_owned()],
    };
    let mut command = Command::new("true");
    git.harden_environment(&mut command).unwrap();
    // What the process itself carries comes first; the settings are numbered after it.
    let value = |name: &str| {
        command
            .get_envs()
            .find(|(key, _)| *key == name)
            .and_then(|(_, value)| value.map(|value| value.to_string_lossy().into_owned()))
    };
    let inherited: usize = std::env::var("GIT_CONFIG_COUNT")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    assert_eq!(value("GIT_CONFIG_COUNT"), Some((inherited + 1).to_string()));
    assert_eq!(
        value(&format!("GIT_CONFIG_KEY_{inherited}")).as_deref(),
        Some("core.fsmonitor")
    );
}

/// A repository with one commit and no drivers, for the tests of what the configuration lists.
fn plain_repository(label: &str) -> PathBuf {
    let root = env::temp_dir().join(format!("soyeht-read-only-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(&root).unwrap();
    git_in(&root, &["init", "-q"]);
    git_in(&root, &["commit", "-q", "--allow-empty", "-m", "baseline"]);
    root
}

fn append(file: &Path, text: &str) {
    use std::io::Write;
    let mut file = fs::OpenOptions::new().append(true).open(file).unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

fn cleared(git: &ReadOnlyGit, driver: &str) -> bool {
    git.settings.contains(&format!("filter.{driver}.clean="))
}

fn lists() -> Limits {
    Limits::new(Duration::from_secs(30), 1 << 20)
}

fn includes(count: usize) -> String {
    (0..count)
        .map(|number| format!("[include]\n\tpath = inc{number}.cfg\n"))
        .collect()
}

/// Past the cap the includes that are left would go unread, and a driver in one of them
/// would run: preparing fails, and a configuration at the cap still works.
#[test]
fn more_includes_than_the_cap_fail_instead_of_being_dropped() {
    let root = plain_repository("include-cap");
    let config = root.join(".git/config");
    // The driver is in the first file the walk would drop: it pops the last entry first.
    fs::write(
        root.join(".git/inc1.cfg"),
        "[filter \"late\"]\n\tclean = touch /nonexistent\n",
    )
    .unwrap();
    append(&config, &includes(INCLUDE_FILES));
    let at_the_cap = ReadOnlyGit::prepare(&root, lists()).unwrap();
    assert!(cleared(&at_the_cap, "late"));
    append(&config, "[include]\n\tpath = one-too-many.cfg\n");
    let over = ReadOnlyGit::prepare(&root, lists());
    fs::remove_dir_all(&root).unwrap();
    assert!(over.is_err(), "65 includes were prepared");
}

/// With `extensions.worktreeConfig`, the `config.worktree` of the main worktree and of every
/// linked one is read by the git that `wt` starts in it, so its drivers are cleared whichever
/// worktree the repository is prepared from, and so are the ones in files it includes.
#[test]
fn the_worktree_config_of_every_worktree_is_read() {
    let root = plain_repository("worktree-config");
    git_in(&root, &["config", "extensions.worktreeConfig", "true"]);
    git_in(
        &root,
        &["worktree", "add", "-q", "-b", "first", "../first-worktree"],
    );
    git_in(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "second",
            "../second-worktree",
        ],
    );
    let common = root.join(".git");
    fs::write(
        common.join("worktrees/first-worktree/config.worktree"),
        "[filter \"linked\"]\n\tclean = x\n[include]\n\tpath = more.cfg\n",
    )
    .unwrap();
    fs::write(
        common.join("worktrees/first-worktree/more.cfg"),
        "[filter \"included\"]\n\tclean = x\n",
    )
    .unwrap();
    fs::write(
        common.join("config.worktree"),
        "[filter \"main\"]\n\tclean = x\n",
    )
    .unwrap();
    let second = root.with_file_name("second-worktree");
    let from_main = ReadOnlyGit::prepare(&root, lists()).unwrap();
    let from_other = ReadOnlyGit::prepare(&second, lists()).unwrap();
    fs::remove_dir_all(&root).unwrap();
    fs::remove_dir_all(&second).unwrap();
    let _ = fs::remove_dir_all(root.with_file_name("first-worktree"));
    for git in [&from_main, &from_other] {
        for driver in ["linked", "included", "main"] {
            assert!(cleared(git, driver), "{driver}: {:?}", git.settings);
        }
    }
}

/// Prepares a repository whose `file` (inside `.git`) holds `text`, and removes it.
fn prepare_with_file(label: &str, file: &str, text: &str) -> Result<ReadOnlyGit> {
    let root = plain_repository(label);
    fs::write(root.join(".git").join(file), text).unwrap();
    let prepared = ReadOnlyGit::prepare(&root, lists());
    fs::remove_dir_all(&root).unwrap();
    prepared
}

/// A `config.worktree` that git cannot read is not skipped.
#[test]
fn an_unreadable_worktree_config_fails() {
    assert!(prepare_with_file("worktree-config-bad", "config.worktree", "[filter\n").is_err());
}

/// Git expands `~user/` to that user's home; this cannot, so an include that starts with a
/// `~` that is not `~/` fails instead of being read as a relative path.
#[test]
fn an_include_git_would_expand_differently_fails() {
    let entry = |value: &str| ConfigEntry {
        scope: "local".into(),
        origin: "file:.git/config".into(),
        key: "include.path".into(),
        value: value.into(),
    };
    let repository = Path::new("/r");
    for value in ["~someone/x.cfg", "~", "%(prefix)/x.cfg"] {
        assert!(
            include_target(repository, &entry(value)).is_err(),
            "{value}"
        );
    }
    assert!(include_target(repository, &entry("~/x.cfg")).is_ok());
    assert!(include_target(repository, &entry("x.cfg")).is_ok());

    let text = "[include]\n\tpath = ~someone/x.cfg\n";
    assert!(prepare_with_file("tilde-user", "config", text).is_err());
}

#[test]
fn a_count_in_the_environment_that_cannot_be_added_to_fails() {
    let git = ReadOnlyGit {
        repository: PathBuf::from("/r"),
        settings: vec!["a=b".to_owned(), "c=d".to_owned()],
    };
    let hardened = |inherited: Option<&str>| {
        let mut command = Command::new("true");
        let result = git.harden_over(&mut command, inherited);
        (result, command.get_envs().count())
    };
    for bad in [
        "many",
        "",
        "-1",
        &usize::MAX.to_string(),
        &(usize::MAX - 1).to_string(),
    ] {
        let (result, touched) = hardened(Some(bad));
        assert!(result.is_err(), "{bad}");
        assert_eq!(touched, 0, "{bad}: the command was changed");
    }
    let (result, _) = hardened(Some("3"));
    assert!(result.is_ok());
    assert!(hardened(None).0.is_ok());
}
