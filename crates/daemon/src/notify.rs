//! Desktop notifications, used when the GNOME extension isn't connected
//! and for the end-of-day summary.

pub trait Notifier {
    fn notify(&self, title: &str, body: &str, ttl_ms: u64);

    /// Hand the weekly note to the local model: start
    /// `flowbuddy-weekly.service`, which may reach the model over the
    /// network stack this daemon is denied. False if it couldn't start.
    fn start_weekly_note(&self) -> bool;
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

    fn start_weekly_note(&self) -> bool {
        // --no-block: queue the job and return; systemd runs it.
        let status = std::process::Command::new("systemctl")
            .args(["--user", "start", "--no-block", "flowbuddy-weekly.service"])
            .status();
        match status {
            Ok(s) if s.success() => true,
            other => {
                tracing::warn!(?other, "could not start flowbuddy-weekly.service");
                false
            }
        }
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

        /// Recorded as a notification titled "weekly service".
        fn start_weekly_note(&self) -> bool {
            self.0
                .borrow_mut()
                .push(("weekly service".into(), String::new()));
            true
        }
    }
}
