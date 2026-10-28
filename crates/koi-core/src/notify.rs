//! Change notification used to wake the UI (e.g. `egui::Context::request_repaint`).

use std::fmt;
use std::sync::Arc;

#[derive(Clone)]
pub struct Notifier(Arc<dyn Fn() + Send + Sync>);

impl Notifier {
    pub fn new(f: impl Fn() + Send + Sync + 'static) -> Self {
        Self(Arc::new(f))
    }

    pub fn noop() -> Self {
        Self::new(|| {})
    }

    pub fn notify(&self) {
        (self.0)()
    }
}

impl Default for Notifier {
    fn default() -> Self {
        Self::noop()
    }
}

impl fmt::Debug for Notifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Notifier")
    }
}
