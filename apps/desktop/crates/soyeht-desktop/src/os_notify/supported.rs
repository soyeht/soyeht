//! Linux (the freedesktop notification service) and Windows (a toast), through `notify-rust`.

use super::{APP_NAME, Error, Outcome, Toast};
use notify_rust::{Notification, NotificationHandle, NotificationResponse, Timeout};

pub(super) struct Shown(NotificationHandle);

pub(super) fn show(toast: &Toast) -> Result<Shown, Error> {
    let mut notification = Notification::new();
    notification
        .appname(APP_NAME)
        .summary(toast.title)
        .body(toast.body)
        .timeout(Timeout::Default);
    // The desktop entry ties the banner to the application's window, so that a shell can group it
    // with the window and show its icon. The `default` action is what a click on the banner's
    // body invokes; without it a server delivers no click at all.
    #[cfg(target_os = "linux")]
    notification
        .hint(notify_rust::Hint::DesktopEntry(APP_NAME.to_lowercase()))
        .action("default", "Open");
    notification
        .show()
        .map(Shown)
        .map_err(|_| Error::Unavailable)
}

impl Shown {
    pub(super) fn wait(self) -> Outcome {
        let mut outcome = Outcome::Closed;
        let answered = self.0.wait_for_response(|response: &NotificationResponse| {
            if matches!(
                response,
                NotificationResponse::Default | NotificationResponse::Action(_)
            ) {
                outcome = Outcome::Activated;
            }
        });
        // A wait that failed (the connection that carried the click is gone) is a notification
        // that is over.
        answered.map_or(Outcome::Closed, |()| outcome)
    }
}
