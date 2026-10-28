//! Running core futures from the UI and polling their results each frame (design D3).

use std::future::Future;

use eframe::egui;
use koi_core::{Error, Result};
use tokio::sync::oneshot;

/// Spawns futures on the tokio runtime and repaints the UI when they finish.
#[derive(Clone)]
pub struct Spawner {
    pub rt: tokio::runtime::Handle,
    pub ctx: egui::Context,
}

impl Spawner {
    pub fn load<T: Send + 'static>(&self, fut: impl Future<Output = Result<T>> + Send + 'static) -> Load<T> {
        let (tx, rx) = oneshot::channel();
        let ctx = self.ctx.clone();
        self.rt.spawn(async move {
            let _ = tx.send(fut.await);
            ctx.request_repaint();
        });
        Load::Loading(rx)
    }

    /// Run a future whose result is reported through `on_err` only.
    pub fn fire(&self, fut: impl Future<Output = Result<()>> + Send + 'static, on_err: impl FnOnce(Error) + Send + 'static) {
        self.rt.spawn(async move {
            if let Err(e) = fut.await {
                on_err(e);
            }
        });
    }
}

/// State of a request.
#[derive(Default)]
pub enum Load<T> {
    #[default]
    Idle,
    Loading(oneshot::Receiver<Result<T>>),
    Ready(T),
    Failed(Error),
}

impl<T> Load<T> {
    /// Check for a finished result. Returns true when the state changed.
    pub fn poll(&mut self) -> bool {
        let Self::Loading(rx) = self else { return false };
        match rx.try_recv() {
            Ok(Ok(v)) => *self = Self::Ready(v),
            Ok(Err(e)) => *self = Self::Failed(e),
            Err(oneshot::error::TryRecvError::Empty) => return false,
            Err(oneshot::error::TryRecvError::Closed) => *self = Self::Failed(Error::cancelled()),
        }
        true
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, Self::Loading(_))
    }

    /// Take a finished result (checking for one first), leaving `Idle`.
    pub fn take_done(&mut self) -> Option<Result<T>> {
        self.poll();
        match std::mem::take(self) {
            Self::Ready(v) => Some(Ok(v)),
            Self::Failed(e) => Some(Err(e)),
            other => {
                *self = other;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_done_picks_up_a_finished_request() {
        let (tx, rx) = oneshot::channel();
        let mut load: Load<u32> = Load::Loading(rx);
        assert!(load.take_done().is_none(), "still running");
        assert!(load.is_loading());
        tx.send(Ok(7)).unwrap();
        assert!(matches!(load.take_done(), Some(Ok(7))));
        assert!(matches!(load, Load::Idle));
    }
}
