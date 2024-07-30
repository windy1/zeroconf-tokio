use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use zeroconf::prelude::*;
use zeroconf::{MdnsService, ServiceRegistration};

pub struct MdnsServiceAsync {
    inner: MdnsService,
    timeout: Duration,
    running: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

impl MdnsServiceAsync {
    pub fn new(service: MdnsService, timeout: Option<Duration>) -> zeroconf::Result<Self> {
        Ok(Self {
            inner: service,
            timeout: timeout.unwrap_or(Duration::from_secs(0)),
            running: Arc::default(),
            join_handle: None,
        })
    }

    pub async fn start(&mut self) -> zeroconf::Result<ServiceRegistration> {
        let (sender, receiver) = oneshot::channel();
        let sender = Arc::new(Mutex::new(Some(sender)));

        let callback = Box::new(move |result, _| {
            sender
                .lock()
                .expect("should have been able to lock sender")
                .take()
                .expect("should have been able to take sender")
                .send(result)
                .expect("should have been able to send result");
            println!("debug");
        });

        self.inner.set_registered_callback(callback);

        let event_loop = self.inner.register()?;
        let timeout = self.timeout;
        let running = self.running.clone();

        self.join_handle = Some(tokio::spawn(async move {
            running.store(true, Ordering::Relaxed);

            while running.load(Ordering::Relaxed) {
                event_loop
                    .poll(timeout)
                    .expect("should have been able to poll event loop");
            }
        }));

        // await on registration
        receiver.await.unwrap()
    }

    pub async fn shutdown(&mut self) {
        self.running.store(false, Ordering::Relaxed);

        // await on the task to complete
        if let Some(join_handle) = self.join_handle.take() {
            join_handle
                .await
                .expect("should be able to join on the task");
        }
    }
}
