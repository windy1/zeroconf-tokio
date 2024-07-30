use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use zeroconf::prelude::*;
use zeroconf::MdnsBrowser;
use zeroconf::ServiceDiscovery;

type Sender = mpsc::SyncSender<zeroconf::Result<ServiceDiscovery>>;
type Receiver = mpsc::Receiver<zeroconf::Result<ServiceDiscovery>>;

pub struct MdnsBrowserAsync {
    inner: MdnsBrowser,
    timeout: Duration,
    sender: Arc<Mutex<Sender>>,
    receiver: Arc<Mutex<Receiver>>,
    running: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

impl MdnsBrowserAsync {
    pub fn new(browser: MdnsBrowser, timeout: Option<Duration>) -> zeroconf::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(0);

        Ok(Self {
            inner: browser,
            timeout: timeout.unwrap_or(Duration::from_secs(0)),
            sender: Arc::new(Mutex::new(sender)),
            receiver: Arc::new(Mutex::new(receiver)),
            running: Arc::default(),
            join_handle: None,
        })
    }

    pub async fn start(&mut self) -> zeroconf::Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Err("Browser already running".into());
        }

        info!("Starting async mDNS browser: {:?}", self.inner);

        let sender = self.sender.clone();

        let callback = Box::new(move |result, _| {
            // send the result to the sync channel
            // note: a normal mpsc does not work here because the callback is
            // not yet in an async context, so we use a sync channel to
            // "rendezvous" with the next `next()` call and forward it to a
            // oneshot channel from there
            debug!("Received service discovery: {:?}", result);
            sender
                .lock()
                .expect("should have been able to lock sender")
                .send(result)
                .expect("should have been able to send result");
        });

        self.inner.set_service_discovered_callback(callback);

        let event_loop = self.inner.browse_services()?;
        let timeout = self.timeout;
        let running = self.running.clone();

        running.store(true, Ordering::Relaxed);

        self.join_handle = Some(tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                event_loop
                    .poll(timeout)
                    .expect("should have been able to poll event loop");
            }

            info!("Browser shutdown");
        }));

        Ok(())
    }

    pub async fn next(&mut self) -> Option<zeroconf::Result<ServiceDiscovery>> {
        if !self.running.load(Ordering::Relaxed) {
            return None;
        }

        let (sender, receiver) = oneshot::channel();
        let tx_sync = self.receiver.clone();

        tokio::spawn(async move {
            // receive the result from the sync channel used in the callback and
            // forward to the oneshot channel that is awaited by this function
            let result = tx_sync.lock().unwrap().recv().unwrap();
            sender.send(result).unwrap();
        });

        Some(receiver.await.unwrap())
    }

    pub async fn shutdown(&mut self) {
        if !self.running.load(Ordering::Relaxed) {
            warn!("Attempted to shutdown browser that is not running");
            return;
        }

        info!("Shutting down async mDNS browser: {:?}", self.inner);

        self.running.store(false, Ordering::Relaxed);

        // unwind the main event loop and continue processing events to unblock
        // the task until it is able to complete
        let unwound = Arc::new(Mutex::new(AtomicBool::default()));
        let unwound_clone = unwound.clone();
        let rx = self.receiver.clone();

        tokio::spawn(async move {
            while !unwound_clone.lock().unwrap().load(Ordering::Relaxed) {
                // discard any results waiting in the sync channel
                let _ = rx.lock().unwrap().try_recv();
            }
        });

        // await on the task to complete
        if let Some(join_handle) = self.join_handle.take() {
            join_handle
                .await
                .expect("should be able to join on the task");
        }

        unwound.lock().unwrap().store(true, Ordering::Relaxed);
        self.join_handle = None;
    }
}

#[cfg(test)]
mod tests {
    use zeroconf::prelude::*;
    use zeroconf::MdnsService;
    use zeroconf::ServiceType;

    use crate::MdnsServiceAsync;

    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_browser() {
        let service_type = ServiceType::new("http", "tcp").unwrap();
        let service = MdnsService::new(service_type.clone(), 8080);
        let mut service = MdnsServiceAsync::new(service, None).unwrap();

        service.start().await.unwrap();

        let browser = MdnsBrowser::new(service_type);
        let mut browser = MdnsBrowserAsync::new(browser, None).unwrap();

        browser.start().await.unwrap();

        let discovered_service = browser.next().await.unwrap().unwrap();

        println!("Discovered service: {:?}", discovered_service);
    }
}
