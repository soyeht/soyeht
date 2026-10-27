use super::*;
use serde_json::json;
use soyeht_protocol::WebCommandKind;
use std::sync::Mutex;

/// A page that answers from a table and remembers what it was asked.
struct FakePage {
    incognito: bool,
    cookies: bool,
    info: Mutex<PageInfo>,
    answers: Vec<(&'static str, String)>,
    asked: Mutex<Vec<String>>,
    navigated: Mutex<Vec<Target>>,
    handoff: Handoff,
}

impl FakePage {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            incognito: false,
            cookies: false,
            info: Mutex::new(PageInfo {
                url: "https://example.com/".into(),
                title: "Example".into(),
                loading: false,
            }),
            answers: Vec::new(),
            asked: Mutex::new(Vec::new()),
            navigated: Mutex::new(Vec::new()),
            handoff: Handoff::default(),
        })
    }

    fn with(answers: Vec<(&'static str, String)>, cookies: bool, incognito: bool) -> Arc<Self> {
        let mut page = Arc::into_inner(Self::new()).unwrap();
        page.answers = answers;
        page.cookies = cookies;
        page.incognito = incognito;
        Arc::new(page)
    }
}

impl Page for FakePage {
    fn incognito(&self) -> bool {
        self.incognito
    }
    fn navigate(&self, target: &Target) -> Result<(), HostError> {
        self.navigated.lock().unwrap().push(target.clone());
        Ok(())
    }
    fn info(&self) -> PageInfo {
        self.info.lock().unwrap().clone()
    }
    fn generation(&self) -> u64 {
        7
    }
    fn run_script(&self, source: &str) -> Result<String, HostError> {
        self.asked.lock().unwrap().push(source.to_owned());
        self.answers
            .iter()
            .find(|(prefix, _)| source.contains(prefix))
            .map(|(_, answer)| answer.clone())
            .ok_or(HostError::Internal("no answer"))
    }
    fn picture(&self) -> Result<Picture, HostError> {
        Ok(Picture {
            width: 50,
            height: 40,
            stride: 200,
            bytes: vec![200; 200 * 40],
        })
    }
    fn wheel(&self, _: f64, _: f64, _: f64) {}
    fn handoff(&self) -> &Handoff {
        &self.handoff
    }
    fn has_cookies(&self, _: &str) -> Result<bool, HostError> {
        Ok(self.cookies)
    }
}

struct Registry(Arc<FakePage>);

impl Pages for Registry {
    fn page(&self, pane_id: &str) -> Option<Arc<dyn Page>> {
        (pane_id == "pane-1").then(|| self.0.clone() as Arc<dyn Page>)
    }
}

fn command(kind: WebCommandKind, session: &str, args: Value) -> WebCommandParams {
    let mut args = args;
    args["_soyehtAccess"] = json!({"session": session, "evaluation": false});
    WebCommandParams {
        pane_id: "pane-1".into(),
        command: kind,
        args,
        timeout_ms: 1000,
        operation_key: "op".into(),
    }
}

fn host(page: &Arc<FakePage>) -> Host {
    Host::new(Arc::new(Registry(page.clone())))
}

fn value(outcome: &Outcome) -> &Value {
    outcome.result.as_ref().unwrap().value.as_ref().unwrap()
}

fn error_kind(outcome: &Outcome) -> ErrorKind {
    outcome.result.as_ref().unwrap_err().data.kind.clone()
}

const READ: &str = r#"{"nodes":[{"ref":"e1","role":"heading","name":"News","state":"","value":null,"rect":{"x":1,"y":2,"width":30,"height":10},"parentRef":null},{"ref":"e2","role":"link","name":"A story","state":"","value":"https://example.com/a","rect":{"x":1,"y":20,"width":30,"height":10},"parentRef":"e1"}],"truncated":false}"#;

fn snapshot_of(page: &Arc<FakePage>, args: Value) -> Outcome {
    host(page).execute(&command(WebCommandKind::Snapshot, "human", args))
}

#[test]
fn a_snapshot_is_the_page_read_in_the_world_the_page_cannot_see() {
    let page = FakePage::with(vec![("maxNodes", READ.into())], false, false);
    let outcome = snapshot_of(&page, json!({}));
    let snapshot = value(&outcome);
    assert_eq!(snapshot["navigationGeneration"], "7");
    assert_eq!(snapshot["title"], "Example");
    assert_eq!(snapshot["url"], "https://example.com/");
    assert_eq!(snapshot["truncated"], false);
    assert_eq!(snapshot["nodes"][1]["value"], "https://example.com/a");
    assert_eq!(snapshot["nodes"][1]["parentRef"], "e1");
    assert!(outcome.attachment.is_none());
    assert_eq!(page.asked.lock().unwrap().len(), 1);
}

#[test]
fn a_snapshot_that_asks_for_a_screenshot_carries_its_picture_before_the_result() {
    let page = FakePage::with(vec![("maxNodes", READ.into())], false, false);
    let outcome = snapshot_of(&page, json!({"screenshot": true}));
    let (reference, bytes) = outcome.attachment.as_ref().unwrap();
    assert_eq!(
        value(&outcome)["screenshot"]["attachmentId"],
        reference.attachment_id
    );
    assert_eq!(&bytes[..4], b"\x89PNG");
    assert_eq!(reference.bytes, bytes.len().to_string());
}

#[test]
fn a_screenshot_is_the_picture_of_what_the_pane_shows() {
    let page = FakePage::new();
    let visible = host(&page).execute(&command(WebCommandKind::Screenshot, "human", json!({})));
    let bytes = &visible.attachment.as_ref().unwrap().1;
    assert_eq!(::image::load_from_memory(bytes).unwrap().height(), 40);
    assert_eq!(value(&visible)["image"]["name"], "screenshot.png");
}

#[test]
fn an_agent_is_refused_while_a_person_has_the_pane_and_a_person_is_not() {
    let page = FakePage::with(vec![("maxNodes", READ.into())], false, false);
    page.handoff.note_person();
    let agent = host(&page).execute(&command(WebCommandKind::Snapshot, "granted", json!({})));
    assert_eq!(error_kind(&agent), ErrorKind::HumanTookOver);
    let person = host(&page).execute(&command(WebCommandKind::Snapshot, "human", json!({})));
    assert!(person.result.is_ok());
    page.handoff.hand_back();
    let agent = host(&page).execute(&command(WebCommandKind::Snapshot, "granted", json!({})));
    assert!(agent.result.is_ok());
}

#[test]
fn a_navigation_goes_to_an_address_or_back_forward_or_again() {
    let page = FakePage::new();
    let host = host(&page);
    for args in [
        json!({"url": "https://example.com/next"}),
        json!({"action": "back"}),
        json!({"action": "forward"}),
        json!({"action": "reload"}),
        json!({"action": "reload_ignoring_cache"}),
    ] {
        let outcome = host.execute(&command(WebCommandKind::Navigate, "human", args));
        assert_eq!(value(&outcome)["accepted"], true);
    }
    assert_eq!(
        *page.navigated.lock().unwrap(),
        [
            Target::Url("https://example.com/next".into()),
            Target::Back,
            Target::Forward,
            Target::Reload,
            Target::ReloadIgnoringCache
        ]
    );
    let refused = host.execute(&command(WebCommandKind::Navigate, "human", json!({})));
    assert_eq!(error_kind(&refused), ErrorKind::NavigationRefused);
}

#[test]
fn an_agent_that_has_not_been_allowed_is_told_when_the_site_has_cookies_and_nothing_runs() {
    let page = FakePage::with(vec![("maxNodes", READ.into())], true, false);
    let outcome = host(&page).execute(&command(WebCommandKind::Snapshot, "probe", json!({})));
    let result = outcome.result.unwrap();
    assert_eq!(result.cookies_present, Some(true));
    assert!(result.value.is_none());
    assert!(page.asked.lock().unwrap().is_empty());
}

#[test]
fn a_site_without_cookies_is_read_at_once_and_says_so() {
    let page = FakePage::with(vec![("maxNodes", READ.into())], false, false);
    let outcome = host(&page).execute(&command(WebCommandKind::Snapshot, "probe", json!({})));
    let result = outcome.result.unwrap();
    assert_eq!(result.cookies_present, Some(false));
    assert!(result.value.is_some());
}

#[test]
fn the_session_of_the_command_must_fit_the_pane() {
    let private = FakePage::with(vec![], false, true);
    let ordinary = FakePage::new();
    for (page, session) in [
        (&private, "granted"),
        (&private, "probe"),
        (&ordinary, "incognito"),
        (&ordinary, "root"),
    ] {
        let outcome = host(page).execute(&command(WebCommandKind::Snapshot, session, json!({})));
        assert_eq!(error_kind(&outcome), ErrorKind::InternalError, "{session}");
    }
    let none = WebCommandParams {
        args: json!({}),
        ..command(WebCommandKind::Snapshot, "human", json!({}))
    };
    assert_eq!(
        error_kind(&host(&ordinary).execute(&none)),
        ErrorKind::InternalError
    );
}

#[test]
fn a_pane_without_a_page_and_a_command_not_served_yet_are_internal_errors_the_engine_accepts() {
    let page = FakePage::new();
    let host = host(&page);
    let mut elsewhere = command(WebCommandKind::Snapshot, "human", json!({}));
    elsewhere.pane_id = "pane-2".into();
    assert_eq!(
        error_kind(&host.execute(&elsewhere)),
        ErrorKind::InternalError
    );
    for kind in [
        WebCommandKind::Click,
        WebCommandKind::Type,
        WebCommandKind::Evaluate,
        WebCommandKind::Console,
        WebCommandKind::Network,
        WebCommandKind::Resize,
    ] {
        let outcome = host.execute(&command(kind, "human", json!({})));
        let error = outcome.result.unwrap_err();
        assert_eq!(error.data.kind, ErrorKind::InternalError);
        assert_eq!(error.code, error_code(&ErrorKind::InternalError));
    }
}

#[test]
fn a_scroll_of_a_stale_reference_asks_for_a_new_snapshot() {
    let page = FakePage::with(vec![("scrollBy", "stale".into())], false, false);
    let outcome = host(&page).execute(&command(
        WebCommandKind::Scroll,
        "human",
        json!({"ref": "e4", "dy": 100}),
    ));
    assert_eq!(error_kind(&outcome), ErrorKind::StaleReference);
    let page = FakePage::with(vec![("scrollBy", "ok".into())], false, false);
    let outcome = host(&page).execute(&command(
        WebCommandKind::Scroll,
        "human",
        json!({"dy": 100}),
    ));
    assert_eq!(value(&outcome)["completed"], true);
}

#[test]
fn a_wait_matches_when_its_text_is_on_the_page_and_gives_up_at_its_time() {
    let page = FakePage::with(vec![("includes", "true".into())], false, false);
    let outcome = host(&page).execute(&command(
        WebCommandKind::Wait,
        "human",
        json!({"text": "ready"}),
    ));
    assert_eq!(value(&outcome)["matched"], true);
    let page = FakePage::with(vec![("includes", "false".into())], false, false);
    let mut wait = command(WebCommandKind::Wait, "human", json!({"text": "ready"}));
    wait.timeout_ms = 300;
    assert_eq!(value(&host(&page).execute(&wait))["matched"], false);
}

#[test]
fn a_wait_for_network_idle_waits_for_the_page_to_stop_loading() {
    let page = FakePage::new();
    page.info.lock().unwrap().loading = true;
    let mut wait = command(WebCommandKind::Wait, "human", json!({"networkIdle": true}));
    wait.timeout_ms = 300;
    assert_eq!(value(&host(&page).execute(&wait))["matched"], false);
    page.info.lock().unwrap().loading = false;
    wait.timeout_ms = 3000;
    assert_eq!(value(&host(&page).execute(&wait))["matched"], true);
}

#[test]
fn the_text_of_a_response_is_clipped_on_a_character_boundary() {
    assert_eq!(clip("ação", 3), "aç");
    assert_eq!(clip("ação", 2), "a");
    assert_eq!(clip("abc", 10), "abc");
}
