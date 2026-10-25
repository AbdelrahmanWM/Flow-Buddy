//! Desktop notifications, used when the GNOME extension isn't connected
//! and for the end-of-day summary.

pub trait Notifier {
    fn notify(&self, title: &str, body: &str, ttl_ms: u64);
}

/// `notify-send`, talking to the desktop's notification service over D-Bus.
pub struct NotifySend;

impl Notifier for NotifySend {
    fn notify(&self, title: &str, body: &str, ttl_ms: u64) {
        let mut cmd = std::process::Command::new("notify-send");
        cmd.args([
            "--app-name=Flow Buddy",
            "--urgency=low",
            "--icon=input-keyboard-symbolic",
            "--expire-time",
            &ttl_ms.to_string(),
            title,
            body,
        ]);
        // Reap the child off the main loop so it never becomes a zombie.
        std::thread::spawn(move || {
            if let Err(e) = cmd.status() {
                tracing::warn!(error = %e, "notify-send failed");
            }
        });
    }
}

#[cfg(test)]
pub mod fake {
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Clone, Default)]
    pub struct Recorder(pub Rc<RefCell<Vec<(String, String)>>>);

    impl super::Notifier for Recorder {
        fn notify(&self, title: &str, body: &str, _ttl_ms: u64) {
            self.0.borrow_mut().push((title.into(), body.into()));
        }
    }
}
