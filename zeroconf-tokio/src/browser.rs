//! Asynchronous mDNS browser.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::sync::mpsc;
use zeroconf::prelude::*;
use zeroconf::{BrowserEvent, MdnsBrowser};

use crate::event_processor::EventProcessor;

type Sender = mpsc::Sender<zeroconf::Result<BrowserEvent>>;
type Receiver = mpsc::Receiver<zeroconf::Result<BrowserEvent>>;

/// Asynchronous mDNS browser.
pub struct MdnsBrowserAsync {
    inner: MdnsBrowser,
    event_processor: EventProcessor,
    sender: Arc<Mutex<Sender>>,
    receiver: Receiver,
}

impl MdnsBrowserAsync {
    /// Create a new asynchronous mDNS browser.
    pub fn new(browser: MdnsBrowser) -> zeroconf::Result<Self> {
        let (sender, receiver) = mpsc::channel(1);

        Ok(Self {
            inner: browser,
            event_processor: EventProcessor::new(),
            sender: Arc::new(Mutex::new(sender)),
            receiver,
        })
    }

    /// Start the browser with a timeout passed to the [`zeroconf::EventLoop`].
    pub async fn start_with_timeout(&mut self, timeout: Duration) -> zeroconf::Result<()> {
        if self.event_processor.is_running() {
            return Err("Browser already running".into());
        }

        info!("Starting async mDNS browser: {:?}", self.inner);

        let sender = self.sender.clone();

        let callback = Box::new(move |result, _| {
            debug!("Received browser event: {:?}", result);
            let sender = sender.clone();
            tokio::spawn(async move { sender.lock().await.send(result).await });
        });

        self.inner.set_service_callback(callback);

        let event_loop = self.inner.browse_services()?;
        self.event_processor
            .start_with_timeout(event_loop, timeout)?;

        Ok(())
    }

    /// Start the browser.
    pub async fn start(&mut self) -> zeroconf::Result<()> {
        self.start_with_timeout(Duration::ZERO).await
    }

    /// Get the next browser event or `None` if the browser is not running.
    pub async fn next(&mut self) -> Option<zeroconf::Result<BrowserEvent>> {
        if !self.event_processor.is_running() {
            return None;
        }

        self.receiver.recv().await
    }

    /// Shutdown the browser.
    pub async fn shutdown(&mut self) -> zeroconf::Result<()> {
        info!("Shutting down browser...");
        self.event_processor.shutdown().await?;
        info!("Browser shut down");
        Ok(())
    }
}

impl Drop for MdnsBrowserAsync {
    fn drop(&mut self) {
        self.event_processor.sync_shutdown();
    }
}

#[cfg(test)]
mod tests {
    use ntest::timeout;
    use zeroconf::MdnsService;
    use zeroconf::ServiceType;
    use zeroconf::prelude::*;

    use crate::MdnsServiceAsync;

    use super::*;

    struct Fixture {
        services: Vec<MdnsServiceAsync>,
        browser: MdnsBrowserAsync,
    }

    impl Fixture {
        fn new(services: Vec<MdnsServiceAsync>, browser: MdnsBrowserAsync) -> Self {
            Self { services, browser }
        }

        fn with_single_service() -> Self {
            let service_type = ServiceType::new("http", "tcp").unwrap();
            let mut service = MdnsService::new(service_type.clone(), 8080);

            service.set_name("test_service");

            Self::new(
                vec![MdnsServiceAsync::new(service).unwrap()],
                MdnsBrowserAsync::new(MdnsBrowser::new(service_type)).unwrap(),
            )
        }

        async fn start(&mut self) -> zeroconf::Result<()> {
            for service in self.services.iter_mut() {
                service.start().await?;
            }

            self.browser.start().await
        }

        async fn shutdown(&mut self) {
            self.browser.shutdown().await.unwrap();

            for service in self.services.iter_mut() {
                service.shutdown().await.unwrap();
            }
        }
    }

    #[tokio::test]
    async fn it_discovers() {
        let mut fixture = Fixture::with_single_service();

        fixture.start().await.unwrap();

        let event = fixture.browser.next().await.unwrap().unwrap();
        let discovered_service = match event {
            BrowserEvent::Add(service) => service,
            _ => panic!("Expected Add event"),
        };
        let service_type = discovered_service.service_type();

        assert_eq!(discovered_service.name(), "test_service");
        assert_eq!(service_type.name(), "http");
        assert_eq!(service_type.protocol(), "tcp");
        assert_eq!(discovered_service.port(), &8080);

        fixture.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn it_discovers_multi_thread() {
        let mut fixture = Fixture::with_single_service();
        fixture.start().await.unwrap();

        let event = fixture.browser.next().await.unwrap().unwrap();
        let discovered_service = match event {
            BrowserEvent::Add(service) => service,
            _ => panic!("Expected Add event"),
        };
        let service_type = discovered_service.service_type();

        assert_eq!(discovered_service.name(), "test_service");
        assert_eq!(service_type.name(), "http");
        assert_eq!(service_type.protocol(), "tcp");
        assert_eq!(discovered_service.port(), &8080);

        fixture.shutdown().await;
    }

    #[tokio::test]
    #[timeout(10000)]
    async fn it_discovers_n_services() {
        let service_type = ServiceType::new("http", "tcp").unwrap();
        let mut service1 = MdnsService::new(service_type.clone(), 8080);
        let mut service2 = MdnsService::new(service_type.clone(), 8081);

        service1.set_name("test_service_1");
        service2.set_name("test_service_2");

        let services = vec![
            MdnsServiceAsync::new(service1).unwrap(),
            MdnsServiceAsync::new(service2).unwrap(),
        ];

        let browser = MdnsBrowserAsync::new(MdnsBrowser::new(service_type)).unwrap();
        let mut fixture = Fixture::new(services, browser);
        let mut service1_discovered = false;
        let mut service2_discovered = false;

        while let Some(Ok(event)) = fixture.browser.next().await {
            if service1_discovered && service2_discovered {
                break;
            }

            if let BrowserEvent::Add(service) = event {
                if service.name() == "test_service_1" {
                    service1_discovered = true;
                } else if service.name() == "test_service_2" {
                    service2_discovered = true;
                }
            }
        }
    }

    #[tokio::test]
    async fn it_cannot_start_if_already_running() {
        let mut fixture = Fixture::with_single_service();
        fixture.start().await.unwrap();

        let result = fixture.browser.start().await;

        assert!(result.is_err());

        fixture.shutdown().await;
    }

    #[tokio::test]
    async fn it_cannot_discover_if_not_running() {
        let mut fixture = Fixture::with_single_service();

        let result = fixture.browser.next().await;

        assert!(result.is_none());
    }
}
