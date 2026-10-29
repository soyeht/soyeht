use super::*;

fn redactor() -> Redactor {
    Redactor::new(
        Some(Path::new("/Users/dana.example")),
        Some("dana.example"),
        Some("Danas-Studio.local"),
    )
}

/// Every `leaked` must be gone from the redaction of `line`.
fn assert_gone(redactor: &Redactor, line: &str, leaked: &[&str]) -> String {
    let clean = redactor.redact(line);
    for text in leaked {
        assert!(!clean.contains(text), "{text} leaked from {line}: {clean}");
    }
    clean
}

#[test]
fn a_log_line_loses_its_home_path_and_its_token() {
    let line = r#"{"event":"store.open_failed","path":"/Users/dana.example/Library/Work Files/db.sqlite","token":"s3cr3tValue","note":"sk-9fA2kLm4Qz7Xw1Rt8Yb3Nc5Vd6"}"#;
    let clean = assert_gone(
        &redactor(),
        line,
        &[
            "dana.example",
            "/Users",
            "Work Files",
            "s3cr3tValue",
            "sk-9fA2kLm4Qz7Xw1Rt8Yb3Nc5Vd6",
        ],
    );
    assert!(clean.contains(r#""event":"store.open_failed""#), "{clean}");
}

#[test]
fn the_name_of_an_event_and_of_a_failure_kind_stay() {
    for line in [
        r#"{"event":"engine.reconnect_attempts_exhausted","kind":"ConnectionRefused","n":"3"}"#,
        r#"{"event":"x","kind":"SomeVeryLongFailureKindNameOfTheEngine"}"#,
        "12:34:56 the engine started in 3.2 s",
        "version 0.1.0 of the engine",
    ] {
        assert_eq!(redactor().redact(line), line);
    }
}

#[test]
fn paths_of_every_system_are_removed() {
    let redactor = Redactor::new(Some(Path::new(r"C:\Users\dana.example")), None, None);
    for line in [
        r"opened C:\Users\dana.example\AppData\Local\Soyeht.next\db",
        r#"{"p":"C:\\Users\\dana.example\\AppData"}"#,
        r"share \\server\share\folder\file",
        "start /usr/local/bin/soyeht-engine now",
        "cwd=/home/dana/work/repo",
        "see ~/work/repo please",
        "file:///home/dana/work/file.txt",
        "unit at .config/systemd/user/unit.service",
    ] {
        assert_gone(
            &redactor,
            line,
            &["dana", "AppData", "usr/local", "systemd", "server", "work"],
        );
    }
}

#[test]
fn a_path_with_blanks_in_its_folders_is_removed_whole() {
    for line in [
        "opened /Users/other/My Projects/Secret Client/db.sqlite failed",
        r"opened D:\Work Stuff\Client Name\repo failed",
        "opened ~/My Projects/Secret Client/db failed",
        "opened /Users/other/Library/Application Support/Some App/db.sqlite",
        r#"{"p":"/Users/other/My Projects/Secret Client/db"}"#,
    ] {
        assert_gone(
            &redactor(),
            line,
            &[
                "Projects",
                "Secret",
                "Client",
                "Stuff",
                "Application",
                "Support",
                "Some App",
            ],
        );
    }
    // The text after the path's end stays: a quote, `: ` and a word, or a new option.
    assert_eq!(
        redactor().redact("cannot open /a/My Projects/b: permission denied"),
        "cannot open <path>: permission denied"
    );
    assert_eq!(
        redactor().redact("run /a/My Projects/b --flag"),
        "run <path> --flag"
    );
}

#[test]
fn addresses_and_machine_names_are_removed() {
    for (line, leaked) in [
        ("connect Caios-Mac-Studio.local failed", "Caios"),
        ("peer 100.64.12.7:4242 up", "100.64"),
        ("peer 192.168.1.20 up", "192.168"),
        ("peer fe80::1c2b:3d4e:5f60:7182%en0 up", "fe80"),
        ("peer 2001:db8::1 up", "2001"),
        ("peer [::1]:4242 up", "::1"),
        ("peer 2001:0db8:85a3:0000:0000:8a2e:0370:7334 up", "8a2e"),
        ("nic 02:42:ac:11:00:02 up", "02:42"),
        ("nic 9C-3D-5E-7F-10-21 up", "9C-3D"),
        ("via laptop.tail1234.ts.net ok", "tail1234"),
        ("via printer.lan ok", "printer"),
        ("the machine Danas-Studio answered", "Danas"),
    ] {
        assert_gone(&redactor(), line, &[leaked]);
    }
}

#[test]
fn the_user_name_goes_in_any_case_and_with_a_plural_or_possessive() {
    let redactor = Redactor::new(None, Some("caio"), None);
    for line in [
        "Caios-Mac-Studio ready",
        "CAIO logged in",
        "caio's folder",
        "hello Caio",
        "the caios",
    ] {
        assert_gone(&redactor, line, &["aio", "AIO"]);
    }
    // Inside a longer word it stays; so does a name under the minimum.
    assert_eq!(redactor.redact("caiofoo and acaio"), "caiofoo and acaio");
    let short = Redactor::new(None, Some("al"), None);
    assert_eq!(short.redact("alpha and al"), "alpha and al");
    let alex = Redactor::new(None, Some("alex"), None);
    assert_eq!(alex.redact("alexander and alex"), "alexander and <user>");
}

#[test]
fn links_addresses_and_secret_fields_are_removed() {
    assert_gone(
        &redactor(),
        "GET https://example.test/hook?key=abc123 from dana@example.com \
         Authorization: Bearer abcDEF123456 password=hunter2 api_key: zzz",
        &[
            "example.test",
            "abc123",
            "dana@example.com",
            "abcDEF123456",
            "hunter2",
            "zzz",
        ],
    );
}

#[test]
fn pairing_codes_pins_and_session_keys_are_removed() {
    for (line, leaked) in [
        (
            r#"{"event":"device.pair_started","pairingCode":"482913"}"#,
            "482913",
        ),
        (r#"{"event":"device.pair","code":"AB-12"}"#, "AB-12"),
        ("pin=1234 given", "1234"),
        ("pin: 4455 given", "4455"),
        ("otp: 556677 sent", "556677"),
        ("x-api-key: k9z8 sent", "k9z8"),
        (r#"{"apiKey":"short1"}"#, "short1"),
        ("session=abcd1234 open", "abcd1234"),
        ("passphrase: correct horse", "correct"),
        (r#"{"privateKey":"xyz"}"#, "xyz"),
    ] {
        assert_gone(&redactor(), line, &[leaked]);
    }
}

#[test]
fn ordinary_words_that_name_a_secret_stay() {
    for line in [
        "the token was refused and the password prompt closed",
        "the pin was wrong",
        "mapping: ready, monkey: here, spinning: yes",
        "a private matter, one session ended",
    ] {
        assert_eq!(redactor().redact(line), line);
    }
}

#[test]
fn the_ids_of_panes_stay_and_a_token_beside_them_goes() {
    let uuid = "3f2a9c1e-7b4d-4e8a-9c3b-1a2b3c4d5e6f";
    let ulid = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    let line = format!(r#"{{"event":"pane.closed","paneId":"{uuid}","workspace":"{ulid}"}}"#);
    assert_eq!(redactor().redact(&line), line);
    let clean = redactor().redact(&format!("pane {uuid} key sk-9fA2kLm4Qz7Xw1Rt8Yb3Nc5Vd6"));
    assert!(clean.contains(uuid), "{clean}");
    assert!(!clean.contains("9fA2"), "{clean}");
    // A field named like a secret loses its value even when the value is an id.
    let clean = redactor().redact(&format!(r#"{{"sessionToken":"{uuid}"}}"#));
    assert!(!clean.contains(uuid), "{clean}");
}

#[test]
fn a_long_line_with_many_secret_words_is_handled_in_one_pass() {
    let line = "token=a ".repeat(20_000);
    let clean = redactor().redact(&line);
    assert_eq!(clean, "token=<redacted> ".repeat(20_000));
}
