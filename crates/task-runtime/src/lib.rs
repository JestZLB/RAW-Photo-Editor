//! Lightweight, dependency-free cancellation and task registry built on
//! `tokio::sync::watch`.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPriority {
    Interactive,
    Foreground,
    Background,
    Export,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskCancelled;

impl fmt::Display for TaskCancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("task cancelled")
    }
}

impl std::error::Error for TaskCancelled {}

#[derive(Clone)]
pub struct CancellationToken {
    tx: Arc<watch::Sender<bool>>,
    rx: watch::Receiver<bool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        let (tx, rx) = watch::channel(false);
        Self {
            tx: Arc::new(tx),
            rx,
        }
    }

    pub fn cancel(&self) {
        let _ = self.tx.send(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.rx.borrow()
    }

    /// Resolves as soon as cancellation is requested. Safe to call after the
    /// token was already cancelled (returns immediately).
    pub async fn cancelled(&self) {
        let mut rx = self.rx.clone();
        if *rx.borrow() {
            return;
        }
        loop {
            if rx.changed().await.is_err() {
                return;
            }
            if *rx.borrow() {
                return;
            }
        }
    }

    pub fn check(&self) -> Result<(), TaskCancelled> {
        if self.is_cancelled() {
            Err(TaskCancelled)
        } else {
            Ok(())
        }
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for CancellationToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CancellationToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct TaskHandle {
    pub id: String,
    pub priority: TaskPriority,
    pub token: CancellationToken,
}

#[derive(Clone, Default)]
pub struct TaskRegistry {
    tasks: Arc<Mutex<HashMap<String, TaskHandle>>>,
}

impl fmt::Debug for TaskRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskRegistry")
            .field("active", &self.active_count())
            .finish()
    }
}

impl TaskRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers (or replaces) a task. Replacing cancels the previous token so
    /// no older execution can keep running unnoticed.
    pub fn register(&self, id: impl Into<String>, priority: TaskPriority) -> TaskHandle {
        let id = id.into();
        let token = CancellationToken::new();
        let handle = TaskHandle {
            id: id.clone(),
            priority,
            token,
        };
        if let Ok(mut tasks) = self.tasks.lock() {
            if let Some(previous) = tasks.insert(id, handle.clone()) {
                previous.token.cancel();
            }
        }
        handle
    }

    pub fn lookup(&self, id: &str) -> Option<TaskHandle> {
        self.tasks
            .lock()
            .ok()
            .and_then(|tasks| tasks.get(id).cloned())
    }

    pub fn cancel(&self, id: &str) -> bool {
        match self.lookup(id) {
            Some(handle) => {
                handle.token.cancel();
                true
            }
            None => false,
        }
    }

    /// Remove a finished task. Returns its token (already cancelled or not) for
    /// callers that want to clean up additional state.
    pub fn finish(&self, id: &str) -> Option<CancellationToken> {
        self.tasks
            .lock()
            .ok()
            .and_then(|mut tasks| tasks.remove(id))
            .map(|handle| handle.token)
    }

    pub fn is_cancelled(&self, id: &str) -> bool {
        self.lookup(id)
            .map(|handle| handle.token.is_cancelled())
            .unwrap_or(false)
    }

    pub fn active_count(&self) -> usize {
        self.tasks.lock().map(|tasks| tasks.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn cancel_wakes_awaiters() {
        let registry = TaskRegistry::new();
        let handle = registry.register("job-1", TaskPriority::Background);
        assert!(registry.lookup("job-1").is_some());
        assert_eq!(registry.active_count(), 1);

        let waiter = handle.token.clone();
        let registry2 = registry.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            assert!(registry2.cancel("job-1"));
        });

        tokio::time::timeout(Duration::from_secs(1), waiter.cancelled())
            .await
            .expect("cancellation should resolve");
        assert!(handle.token.is_cancelled());
        assert_eq!(handle.token.check(), Err(TaskCancelled));
        assert!(registry.is_cancelled("job-1"));
    }

    #[tokio::test]
    async fn cancellation_before_await_is_immediate() {
        let token = CancellationToken::new();
        token.cancel();
        tokio::time::timeout(Duration::from_millis(50), token.cancelled())
            .await
            .expect("already cancelled");
    }

    #[test]
    fn finish_removes_and_further_cancel_is_noop() {
        let registry = TaskRegistry::new();
        let handle = registry.register("export", TaskPriority::Export);
        assert_eq!(handle.priority, TaskPriority::Export);
        assert!(registry.finish("export").is_some());
        assert!(registry.lookup("export").is_none());
        assert!(!registry.cancel("export"));
        assert_eq!(registry.active_count(), 0);
    }

    #[test]
    fn registry_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TaskRegistry>();
        assert_send_sync::<CancellationToken>();
    }
}
