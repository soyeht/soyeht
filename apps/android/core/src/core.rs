//! The object the Kotlin app holds: it pairs, connects as a device, keeps the replica the desktop
//! client keeps, and answers the views' questions from it.

use crate::{
    CoreError,
    conversation::{self, Conversation},
    overview::{self, Overview},
    terminal::{self, KeyPress, TerminalFrame},
};
use soyeht_client::{
    Command, Engine, Error, Event, State,
    device::{self, Cancel, Credential, PairingFailure},
    keys,
};
use soyeht_protocol::{
    client_policy::{CLIENT_NAME_ANDROID, CLIENT_NAME_ANDROID_DEV},
    limits,
};
use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicU64, Ordering},
};

/// The client name in `hello`: `android-dev` in a debug build, so the engine's log tells a person's
/// phone from a developer's.
const CLIENT: &str = if cfg!(debug_assertions) {
    CLIENT_NAME_ANDROID_DEV
} else {
    CLIENT_NAME_ANDROID
};

/// What the views are told as it happens.
#[uniffi::export(with_foreign)]
pub trait Listener: Send + Sync {
    /// The replica changed: ask for what is on screen again.
    fn changed(&self);
    /// The fingerprint of this device's key, to compare with the one the computer shows.
    fn fingerprint(&self, fingerprint: String);
    /// The computer has the proof and waits for the person to allow access.
    fn waiting_for_approval(&self);
}

/// The name this phone gives itself when it pairs, which the person reads on the computer: its maker
/// and model, cut to the longest name the engine takes (`limits::DEVICE_NAME_BYTES`) at a character.
#[uniffi::export]
pub fn device_name(maker: String, model: String) -> String {
    let mut name = format!("{maker} {model}").trim().to_owned();
    while name.len() > limits::DEVICE_NAME_BYTES {
        name.pop();
    }
    name
}

#[derive(Default)]
struct Replica {
    state: State,
    /// The computer refused this device's access: the person has to pair again.
    refused: Option<String>,
}

#[derive(uniffi::Object)]
pub struct Core {
    listener: Arc<dyn Listener>,
    replica: Arc<Mutex<Replica>>,
    engine: Mutex<Option<Arc<Engine>>>,
    /// Ends the pairing that is waiting, if there is one.
    pairing: Mutex<Arc<Cancel>>,
    /// Which connection the replica belongs to: the events of an earlier one are dropped, not applied.
    epoch: Arc<AtomicU64>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[uniffi::export]
impl Core {
    #[uniffi::constructor]
    pub fn new(listener: Arc<dyn Listener>) -> Arc<Self> {
        Arc::new(Self {
            listener,
            replica: Arc::default(),
            engine: Mutex::default(),
            pairing: Mutex::default(),
            epoch: Arc::default(),
        })
    }

    /// Pairs with the computer a pairing link names, calling this device `device_name`. Blocks until
    /// the person allows access there; returns the access to keep, which `connect` takes back.
    pub fn pair(&self, link: String, device_name: String) -> Result<String, CoreError> {
        let cancel = Arc::new(Cancel::default());
        *lock(&self.pairing) = Arc::clone(&cancel);
        let listener = &self.listener;
        let credential = device::pair(
            &link,
            &device_name,
            &mut |progress| match progress {
                device::Progress::Fingerprint(fingerprint) => listener.fingerprint(fingerprint),
                device::Progress::WaitingForApproval => listener.waiting_for_approval(),
            },
            &cancel,
        )?;
        Ok(credential.to_json()?)
    }

    /// Gives up a pairing that is waiting for the person: it ends at once, and never returns access.
    pub fn cancel_pairing(&self) {
        lock(&self.pairing).stop();
    }

    /// Connects with saved access and keeps connecting until `disconnect`.
    pub fn connect(&self, credential: String) -> Result<(), CoreError> {
        let credential = Credential::from_json(&credential).map_err(|error| match error {
            Error::Json(_) => CoreError::PairAgain {
                reason: PairingFailure::PairAgain.to_string(),
            },
            other => other.into(),
        })?;
        self.disconnect();
        let mine = self.epoch.fetch_add(1, Ordering::AcqRel) + 1;
        *lock(&self.replica) = Replica::default();
        let (engine, inbox) = Engine::start_device(credential, CLIENT);
        let engine = Arc::new(engine);
        // The thread below holds the engine weakly: the engine's workers hold the other end of its
        // queue, so a strong hold would keep both alive for ever.
        let weak = Arc::downgrade(&engine);
        *lock(&self.engine) = Some(engine);
        let (replica, listener) = (Arc::clone(&self.replica), Arc::clone(&self.listener));
        let epoch = Arc::clone(&self.epoch);
        std::thread::spawn(move || {
            while let Ok(event) = inbox.recv_blocking() {
                let effects = {
                    let mut replica = lock(&replica);
                    if epoch.load(Ordering::Acquire) != mine {
                        break;
                    }
                    // A server that is gone, or that refused this device, leaves nothing to show.
                    let refusal = match &event {
                        Event::Problem { cause, message } if Credential::refused_cause(cause) => {
                            Some(message.clone())
                        }
                        _ => None,
                    };
                    if refusal.is_some() || matches!(event, Event::Disconnected(_)) {
                        replica.state = State::default();
                    }
                    if refusal.is_some() {
                        replica.refused = refusal;
                    }
                    replica.state.apply(event)
                };
                let Some(engine) = weak.upgrade() else {
                    break;
                };
                for effect in effects {
                    for command in effect.commands() {
                        // A command the queue refuses is a connection that stopped answering; the
                        // engine reconnects and the replica starts again from a snapshot.
                        if engine.send(command).is_err() {
                            engine.resync();
                        }
                    }
                    if matches!(effect, soyeht_client::Effect::Resync) {
                        engine.resync();
                    }
                }
                listener.changed();
            }
        });
        Ok(())
    }

    /// Ends the connection and forgets what it held.
    pub fn disconnect(&self) {
        // The epoch moves on, so what the old connection still says is not applied.
        self.epoch.fetch_add(1, Ordering::AcqRel);
        if let Some(engine) = lock(&self.engine).take() {
            engine.stop();
        }
    }

    pub fn overview(&self) -> Overview {
        let replica = lock(&self.replica);
        overview::overview(&replica.state, replica.refused.as_deref())
    }

    /// The screen of a terminal pane, at the size the pane has on the computer. The phone never
    /// asks the computer to resize it (ADR 0013).
    pub fn terminal(&self, pane_id: String) -> Option<TerminalFrame> {
        lock(&self.replica)
            .state
            .screen(&pane_id)
            .map(terminal::frame)
    }

    /// Text the person typed into a terminal pane.
    pub fn type_text(&self, pane_id: String, text: String) {
        self.send(Command::Input {
            pane_id,
            bytes: text.into_bytes(),
        });
    }

    /// A key that is not text: Enter, an arrow, Escape, a Control combination.
    pub fn press_key(&self, pane_id: String, press: KeyPress) {
        let bytes = lock(&self.replica)
            .state
            .screen(&pane_id)
            .and_then(|screen| terminal::encode(&press, screen));
        if let Some(bytes) = bytes {
            self.send(Command::Input { pane_id, bytes });
        }
    }

    /// Pasted text, wrapped for programs that asked for bracketed paste.
    pub fn paste(&self, pane_id: String, text: String) {
        let bracketed = lock(&self.replica)
            .state
            .screen(&pane_id)
            .is_some_and(|screen| screen.bracketed_paste());
        self.send(Command::Input {
            pane_id,
            bytes: keys::paste(&text, bracketed),
        });
    }

    pub fn conversation(&self, pane_id: String) -> Option<Conversation> {
        conversation::conversation(&lock(&self.replica).state, &pane_id)
    }

    pub fn send_prompt(&self, pane_id: String, text: String) {
        self.send(Command::Prompt {
            pane_id,
            text,
            attachments: Vec::new(),
        });
    }

    pub fn answer_permission(&self, pane_id: String, request_id: String, option_id: String) {
        self.send(Command::AnswerPermission {
            pane_id,
            request_id,
            option_id,
        });
    }

    /// The pane the person looks at takes the keyboard on the computer too.
    pub fn focus(&self, pane_id: String) {
        lock(&self.replica).state.focus_pane(&pane_id);
        self.send(Command::Focus { pane_id });
    }
}

impl Core {
    fn send(&self, command: Command) {
        let engine = lock(&self.engine).clone();
        let sent = engine.as_ref().map(|engine| engine.send(command));
        if !matches!(sent, Some(Ok(()))) {
            // A command nobody could take is a connection that is not there: the views show the
            // link, and the person tries again.
            soyeht_platform::log::event("android.command_dropped", &[]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overview::Link;

    struct Silent;

    impl Listener for Silent {
        fn changed(&self) {}
        fn fingerprint(&self, _fingerprint: String) {}
        fn waiting_for_approval(&self) {}
    }

    #[test]
    fn a_new_core_is_connecting_and_has_nothing_to_show() {
        let core = Core::new(Arc::new(Silent));
        let overview = core.overview();
        assert!(matches!(overview.link, Link::Connecting));
        assert!(overview.workspaces.is_empty());
        assert!(core.terminal("pane".to_owned()).is_none());
        assert!(core.conversation("pane".to_owned()).is_none());
    }

    #[test]
    fn a_device_name_is_the_maker_and_model_cut_to_what_the_engine_takes() {
        assert_eq!(
            device_name("Google".to_owned(), "Pixel 8".to_owned()),
            "Google Pixel 8"
        );
        assert_eq!(device_name(String::new(), "Pixel".to_owned()), "Pixel");
        let long = device_name("é".repeat(100), "é".repeat(100));
        assert!(
            long.len() <= limits::DEVICE_NAME_BYTES && long.len() > limits::DEVICE_NAME_BYTES - 2
        );
        assert!(long.is_char_boundary(long.len()));
    }

    /// Access for an engine that nothing listens for: the engine keeps trying to connect.
    const UNREACHABLE: &str = r#"{"engine_id":"6f1e2d3c-4b5a-4978-8a6b-5c4d3e2f1a0b",
        "device_id":"0c9b6a52-1b2f-4f6e-9d3a-7a1e5f4b8c10","address":"127.0.0.1:1","token":"t",
        "key":"BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=",
        "engine_key":"0EqyMnQrtKs6E2i9RhXk5tAiSrcaAWuvhSCjMsl3hzc="}"#;

    fn until(what: &str, ready: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !ready() {
            assert!(std::time::Instant::now() < deadline, "{what}");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn a_connection_that_ends_leaves_no_thread_and_no_listener_behind_and_shows_nothing() {
        let listener = Arc::new(Silent);
        let core = Core::new(Arc::clone(&listener) as Arc<dyn Listener>);
        // The test, the core, and nothing else.
        assert_eq!(Arc::strong_count(&listener), 2);
        for _ in 0..3 {
            core.connect(UNREACHABLE.to_owned()).unwrap();
            until("the core never said the engine was unreachable", || {
                matches!(core.overview().link, Link::Disconnected { .. })
            });
            assert!(core.overview().workspaces.is_empty());
            core.disconnect();
            // The event thread, which holds a clone of the listener, has ended and let go of the engine.
            until(
                "the event thread of the connection is still running",
                || Arc::strong_count(&listener) == 2,
            );
        }
        // A core that is dropped with a connection open lets go of everything too.
        core.connect(UNREACHABLE.to_owned()).unwrap();
        drop(core);
        until("a dropped core left its event thread running", || {
            Arc::strong_count(&listener) == 1
        });
    }

    #[test]
    fn a_cancelled_pairing_is_told_as_cancelled() {
        let core = Core::new(Arc::new(Silent));
        core.cancel_pairing();
        // Nothing waits: the next pairing starts afresh and a bad link is still a bad link.
        assert!(matches!(
            core.pair("not a link".to_owned(), "Phone".to_owned()),
            Err(CoreError::Failed { .. })
        ));
        assert_eq!(
            CoreError::from(Error::Pairing(PairingFailure::Cancelled)).to_string(),
            "Pairing was cancelled."
        );
        assert!(matches!(
            CoreError::from(Error::Pairing(PairingFailure::Cancelled)),
            CoreError::Cancelled
        ));
    }

    #[test]
    fn a_text_that_is_not_a_pairing_link_is_refused_in_words() {
        let core = Core::new(Arc::new(Silent));
        let refused = core.pair("https://example.com".to_owned(), "Phone".to_owned());
        assert!(matches!(
            refused,
            Err(CoreError::Failed { reason }) if reason.contains("pairing link")
        ));
    }

    #[test]
    fn saved_access_that_is_not_access_asks_to_pair_again() {
        let core = Core::new(Arc::new(Silent));
        assert!(matches!(
            core.connect("not json".to_owned()),
            Err(CoreError::PairAgain { .. })
        ));
    }
}
