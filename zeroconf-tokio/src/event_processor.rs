//! Event processor for mDNS event loop.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use tokio::task::JoinHandle;
use zeroconf::{EventLoop, prelude::*};

/// Event processor for mDNS event loop.
#[derive(Default)]
pub struct EventProcessor {
    running: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

impl EventProcessor {
    /// Create a new event processor.
    pub fn new() -> Self {
        Self {
            running: Arc::default(),
            join_handle: None,
        }
    }

    /// Check if the event processor is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Start the event processor with a timeout passed to the [`zeroconf::EventLoop`].
    pub fn start_with_timeout(
        &mut self,
        event_loop: EventLoop,
        timeout: Duration,
    ) -> zeroconf::Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Err("Event loop processor already running".into());
        }

        debug!("Starting mDNS event processor...");

        let running = self.running.clone();

        running.store(true, Ordering::Relaxed);

        self.join_handle = Some(tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                event_loop
                    .poll(timeout)
                    .expect("should have been able to poll event loop");
                tokio::task::yield_now().await;
            }
        }));

        Ok(())
    }

    /// Start the event processor.
    pub fn start(&mut self, event_loop: EventLoop) -> zeroconf::Result<()> {
        self.start_with_timeout(event_loop, Duration::ZERO)
    }

    /// Shutdown the event processor.
    pub async fn shutdown(&mut self) -> zeroconf::Result<()> {
        if !self.running.load(Ordering::Relaxed) {
            return Err("Event loop processor not running".into());
        }

        debug!("Shutting down mDNS event processor...");

        self.running.store(false, Ordering::Relaxed);

        if let Some(join_handle) = self.join_handle.take() {
            join_handle
                .await
                .expect("should be able to join on the task");
        }

        self.join_handle = None;

        debug!("mDNS event processor shut down");

        Ok(())
    }

    pub(crate) fn sync_shutdown(&mut self) {
        self.running.store(false, Ordering::Relaxed);

        if let Some(join_handle) = self.join_handle.take() {
            join_handle.abort();

            while !join_handle.is_finished() {
                std::hint::spin_loop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn it_cannot_shutdown_if_not_running() {
        let mut event_processor = EventProcessor::new();

        let result = event_processor.shutdown().await;
        assert!(result.is_err());
    }
}
