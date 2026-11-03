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
    device::{self, Credential},
    keys,
};
use soyeht_protocol::client_policy::{CLIENT_NAME_ANDROID, CLIENT_NAME_ANDROID_DEV};
use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicBool, Ordering},
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
    pairing_stopped: Arc<AtomicBool>,
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
            pairing_stopped: Arc::default(),
        })
    }

    /// Pairs with the computer a pairing link names, calling this device `device_name`. Blocks until
    /// the person allows access there; returns the access to keep, which `connect` takes back.
    pub fn pair(&self, link: String, device_name: String) -> Result<String, CoreError> {
        self.pairing_stopped.store(false, Ordering::Relaxed);
        let listener = &self.listener;
        let credential = device::pair(
            &link,
            &device_name,
            &mut |progress| match progress {
                device::Progress::Fingerprint(fingerprint) => listener.fingerprint(fingerprint),
                device::Progress::WaitingForApproval => listener.waiting_for_approval(),
            },
            &|| self.pairing_stopped.load(Ordering::Relaxed),
        )?;
        Ok(credential.to_json()?)
    }

    /// Gives up a pairing that is waiting for the person.
    pub fn cancel_pairing(&self) {
        self.pairing_stopped.store(true, Ordering::Relaxed);
    }

    /// Connects with saved access and keeps connecting until `disconnect`.
    pub fn connect(&self, credential: String) -> Result<(), CoreError> {
        let credential = Credential::from_json(&credential).map_err(|error| match error {
            Error::Json(_) => CoreError::PairAgain {
                reason: "This device cannot check your computer. Pair again.".to_owned(),
            },
            other => other.into(),
        })?;
        self.disconnect();
        *lock(&self.replica) = Replica::default();
        let (engine, inbox) = Engine::start_device(credential, CLIENT);
        let engine = Arc::new(engine);
        *lock(&self.engine) = Some(Arc::clone(&engine));
        let (replica, listener) = (Arc::clone(&self.replica), Arc::clone(&self.listener));
        std::thread::spawn(move || {
            while let Ok(event) = inbox.recv_blocking() {
                let effects = {
                    let mut replica = lock(&replica);
                    if let Event::Problem { cause, message } = &event
                        && matches!(cause.as_str(), "unauthorized" | "engine_identity_changed")
                    {
                        replica.refused = Some(message.clone());
                    }
                    replica.state.apply(event)
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
