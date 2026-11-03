use super::*;
use std::cell::RefCell;

const OFF: &str = r#"{"services":[{"component":"ptyd","name":"a","registered":false,"running":false},
    {"component":"engine","name":"b","registered":false,"running":false}]}"#;
const ON: &str = r#"{"services":[{"component":"ptyd","name":"a","registered":true,"running":true},
    {"component":"engine","name":"b","registered":true,"running":true}]}"#;

/// Answers each call with the next of `answers` and records the arguments.
struct Script {
    answers: RefCell<Vec<Result<String, Failure>>>,
    calls: RefCell<Vec<Vec<String>>>,
}

impl Script {
    fn new(answers: Vec<Result<String, Failure>>) -> Self {
        Self {
            answers: RefCell::new(answers),
            calls: RefCell::new(Vec::new()),
        }
    }

    fn ask(&self, arguments: &[&str]) -> Result<String, Failure> {
        self.calls
            .borrow_mut()
            .push(arguments.iter().map(|word| (*word).to_owned()).collect());
        self.answers.borrow_mut().remove(0)
    }

    fn calls(&self) -> Vec<String> {
        self.calls
            .borrow()
            .iter()
            .map(|call| call.join(" "))
            .collect()
    }
}

fn run_failure() -> Failure {
    Failure::Run("status:1".into())
}

const HALF: &str = r#"{"services":[{"component":"ptyd","name":"a","registered":false,"running":false},
    {"component":"engine","name":"b","registered":true,"running":false}]}"#;

#[test]
fn the_registration_is_all_none_or_part_of_the_services() {
    assert_eq!(registration(ON), Ok(Registration::Full));
    assert_eq!(registration(OFF), Ok(Registration::None));
    assert_eq!(registration(HALF), Ok(Registration::Partial));
    // A report with no service is not a report; neither is text that is not one.
    assert_eq!(registration(r#"{"services":[]}"#), Err(Failure::Unreadable));
    assert_eq!(registration("not json"), Err(Failure::Unreadable));
}

#[test]
fn what_the_command_line_prints_is_what_the_window_reads() {
    // The command line serializes `ServiceReport`; the window parses the same type, so a field
    // renamed on one side fails to compile on the other. This pins the JSON between them too.
    let printed = serde_json::to_string(&ServiceReport {
        services: vec![soyeht_protocol::service_report::ServiceState {
            component: "engine".into(),
            name: "soyeht-next-engine.service".into(),
            registered: true,
            running: false,
            current: None,
        }],
    })
    .unwrap();
    assert_eq!(registration(&printed), Ok(Registration::Full));
}

#[test]
fn the_status_asks_the_services_of_the_profile_and_changes_nothing() {
    let script = Script::new(vec![Ok(ON.into())]);
    assert_eq!(
        is_registered(&|call| script.ask(call)),
        Ok(Registration::Full)
    );
    assert_eq!(script.calls(), ["service status"]);
}

#[test]
fn turning_on_installs_and_wants_the_services_registered_afterwards() {
    let script = Script::new(vec![Ok(ON.into()), Ok(OFF.into()), Err(run_failure())]);
    let ask = |call: &[&str]| script.ask(call);
    assert_eq!(turn_on(&ask), Ok(()));
    assert_eq!(
        turn_on(&ask),
        Err(Failure::Run("not_registered".into())),
        "a command that exits well and registers nothing is not a success"
    );
    assert_eq!(turn_on(&ask), Err(run_failure()));
    assert_eq!(script.calls(), ["service install"; 3]);
}

#[test]
fn turning_off_uninstalls_and_wants_nothing_registered_afterwards() {
    let script = Script::new(vec![Ok(OFF.into()), Ok(ON.into()), Err(run_failure())]);
    let ask = |call: &[&str]| script.ask(call);
    assert_eq!(turn_off(&ask), Ok(()));
    assert_eq!(turn_off(&ask), Err(Failure::Run("still_registered".into())));
    assert_eq!(turn_off(&ask), Err(run_failure()));
    assert_eq!(script.calls(), ["service uninstall"; 3]);
}

#[test]
fn a_start_is_the_service_start_command_and_its_failure_is_the_commands() {
    let script = Script::new(vec![Ok(ON.into()), Err(run_failure())]);
    let ask = |call: &[&str]| script.ask(call);
    assert_eq!(start(&ask), Ok(()));
    assert_eq!(start(&ask), Err(run_failure()));
    assert_eq!(script.calls(), ["service start"; 2]);
}

#[test]
fn a_window_that_has_not_asked_shows_nothing_and_asks_nothing() {
    let setting = LoginStart::new(false);
    assert!(!setting.is_available());
    assert!(!setting.should_ask());
    assert!(!setting.can_change());
    assert_eq!(setting.message(), None);
}

#[test]
fn the_first_question_is_asked_once() {
    let mut setting = LoginStart::new(false);
    assert!(setting.begin_check());
    assert!(!setting.begin_check(), "one question at a time");
    assert_eq!(setting.message(), Some((text::WORKING, false)));
    setting.checked(Ok(Registration::None));
    assert!(
        !setting.begin_check(),
        "and not again once it has an answer"
    );
}

#[test]
fn a_copy_that_is_not_registered_asks_the_person_until_they_decline() {
    let mut setting = LoginStart::new(false);
    assert!(setting.begin_check());
    setting.checked(Ok(Registration::None));
    assert!(setting.should_ask());
    assert!(!setting.is_on());
    setting.decline();
    assert!(!setting.should_ask(), "a no is asked once");
    assert!(setting.declined());
    assert!(
        setting.can_change(),
        "and the setting still answers a click"
    );
}

#[test]
fn a_person_who_declined_before_is_not_asked_and_one_who_registered_is_not_asked() {
    let mut declined = LoginStart::new(true);
    declined.begin_check();
    declined.checked(Ok(Registration::None));
    assert!(!declined.should_ask());
    let mut registered = LoginStart::new(false);
    registered.begin_check();
    registered.checked(Ok(Registration::Full));
    assert!(registered.is_on());
    assert!(!registered.should_ask());
}

#[test]
fn a_copy_with_no_soyeht_program_beside_it_has_no_setting() {
    let mut setting = LoginStart::new(false);
    setting.begin_check();
    setting.checked(Err(Failure::Unavailable));
    assert!(!setting.is_available());
    assert!(!setting.should_ask());
    assert!(!setting.can_change());
}

#[test]
fn a_question_that_fails_leaves_the_setting_off_with_the_reason_said() {
    let mut setting = LoginStart::new(false);
    setting.begin_check();
    setting.checked(Err(run_failure()));
    assert!(!setting.is_on());
    assert!(
        !setting.should_ask(),
        "a window that cannot tell does not ask"
    );
    assert_eq!(setting.message(), Some((text::FAILED, true)));
    assert!(setting.can_change(), "the person can try the switch");
}

#[test]
fn turning_the_setting_on_registers_and_then_shows_it_on() {
    let mut setting = LoginStart::new(true);
    setting.begin_check();
    setting.checked(Ok(Registration::None));
    assert!(setting.begin_change(true));
    assert!(setting.is_on(), "the switch moves at once");
    assert!(
        !setting.can_change(),
        "and answers no click until it is done"
    );
    assert_eq!(setting.message(), Some((text::WORKING, false)));
    assert!(!setting.begin_change(false), "one change at a time");
    setting.changed(true, Ok(()));
    assert!(setting.is_on());
    assert_eq!(setting.message(), None);
    assert!(!setting.declined(), "a yes undoes an earlier no");
}

#[test]
fn turning_the_setting_off_unregisters_and_then_shows_it_off() {
    let mut setting = LoginStart::new(false);
    setting.begin_check();
    setting.checked(Ok(Registration::Full));
    assert!(!setting.begin_change(true), "it is on already");
    assert!(setting.begin_change(false));
    assert!(!setting.is_on());
    setting.changed(false, Ok(()));
    assert!(!setting.is_on());
    assert!(setting.declined(), "turning it off is a no");
    assert!(
        !setting.should_ask(),
        "so the window does not ask the person who just chose"
    );
}

#[test]
fn a_change_that_fails_puts_the_switch_back_and_can_be_tried_again() {
    let mut setting = LoginStart::new(false);
    setting.begin_check();
    setting.checked(Ok(Registration::None));
    setting.begin_change(true);
    setting.changed(true, Err(run_failure()));
    assert!(!setting.is_on());
    assert_eq!(setting.message(), Some((text::FAILED, true)));
    assert!(!setting.should_ask(), "a failure is not a question");
    assert!(setting.begin_change(true));
    setting.changed(true, Ok(()));
    assert!(setting.is_on());

    setting.begin_change(false);
    setting.changed(false, Err(run_failure()));
    assert!(setting.is_on(), "a failed turn-off leaves it on");
}

#[test]
fn nothing_changes_before_the_setting_has_an_answer() {
    let mut setting = LoginStart::new(false);
    assert!(!setting.begin_change(true));
    setting.begin_check();
    assert!(!setting.begin_change(true));
}

#[test]
fn a_half_registered_login_start_is_said_and_turning_it_on_repairs_it() {
    let mut setting = LoginStart::new(false);
    setting.begin_check();
    setting.checked(Ok(Registration::Partial));
    assert!(!setting.is_on(), "the switch does not claim it is on");
    assert!(
        !setting.should_ask(),
        "and the window does not ask over a registration"
    );
    assert_eq!(setting.message(), Some((text::PARTIAL, true)));
    assert!(setting.can_change());
    assert!(setting.begin_change(true), "turning it on is the repair");
    setting.changed(true, Ok(()));
    assert!(setting.is_on());
    assert_eq!(setting.message(), None);
}

#[test]
fn turning_on_a_half_registration_that_stays_half_is_a_failure() {
    let script = Script::new(vec![Ok(HALF.into())]);
    assert_eq!(
        turn_on(&|call| script.ask(call)),
        Err(Failure::Run("not_registered".into()))
    );
}
