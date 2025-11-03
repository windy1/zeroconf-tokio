//! Asynchronous mDNS service registration.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::oneshot;
use zeroconf::prelude::*;
use zeroconf::{MdnsService, ServiceRegistration};

use crate::event_processor::EventProcessor;

/// Asynchronous mDNS service registration.
pub struct MdnsServiceAsync {
    inner: MdnsService,
    event_processor: EventProcessor,
}

impl MdnsServiceAsync {
    /// Create a new asynchronous mDNS service.
    pub fn new(service: MdnsService) -> zeroconf::Result<Self> {
        Ok(Self {
            inner: service,
            event_processor: EventProcessor::new(),
        })
    }

    /// Start the service with a timeout passed to the [`zeroconf::EventLoop`].
    pub async fn start_with_timeout(
        &mut self,
        timeout: Duration,
    ) -> zeroconf::Result<ServiceRegistration> {
        if self.event_processor.is_running() {
            return Err("Service already running".into());
        }

        info!("Starting async mDNS service: {:?}", self.inner);

        let (sender, receiver) = oneshot::channel();
        let sender = Arc::new(Mutex::new(Some(sender)));

        let callback = Box::new(move |result, _| {
            debug!("Received service registration: {:?}", result);
            sender
                .lock()
                .expect("should have been able to lock sender")
                .take()
                .expect("should have been able to take sender")
                .send(result)
                .expect("should have been able to send result");
        });

        self.inner.set_registered_callback(callback);

        let event_loop = self.inner.register()?;
        self.event_processor
            .start_with_timeout(event_loop, timeout)?;

        // await on registration
        receiver
            .await
            .expect("should have been able to receive registration")
    }

    /// Start the service.
    pub async fn start(&mut self) -> zeroconf::Result<ServiceRegistration> {
        self.start_with_timeout(Duration::ZERO).await
    }

    /// Shutdown the service.
    pub async fn shutdown(&mut self) -> zeroconf::Result<()> {
        info!("Shutting down async mDNS service: {:?}", self.inner);
        self.event_processor.shutdown().await?;
        info!("Service shut down");
        Ok(())
    }
}

impl Drop for MdnsServiceAsync {
    fn drop(&mut self) {
        self.event_processor.sync_shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use zeroconf::ServiceType;

    struct Fixture {
        service: MdnsServiceAsync,
    }

    impl Fixture {
        fn new() -> Self {
            let service_type = ServiceType::new("http", "tcp").unwrap();
            let mut service = MdnsService::new(service_type, 8080);

            service.set_name("test_service");

            Self {
                service: MdnsServiceAsync::new(service).unwrap(),
            }
        }
    }

    #[tokio::test]
    async fn it_registers() {
        let mut fixture = Fixture::new();
        let registration = fixture.service.start().await.unwrap();
        let service_type = registration.service_type();

        assert_eq!(registration.name(), "test_service");
        assert_eq!(service_type.name(), "http");
        assert_eq!(service_type.protocol(), "tcp");

        fixture.service.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn it_cannot_start_if_already_running() {
        let mut fixture = Fixture::new();

        fixture.service.start().await.unwrap();

        let result = fixture.service.start().await;

        assert!(result.is_err());

        fixture.service.shutdown().await.unwrap();
    }
}
