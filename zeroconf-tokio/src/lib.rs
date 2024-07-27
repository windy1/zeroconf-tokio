use std::sync::mpsc;
use std::time::Duration;

use zeroconf::prelude::*;
use zeroconf::MdnsService;

struct MdnsServiceAsync {
    inner: MdnsService,
}

impl MdnsServiceAsync {
    fn new(service: MdnsService) -> Self {
        Self { inner: service }
    }

    async fn start(mut self) -> zeroconf::Result<()> {
        let (tx, mut rx) = mpsc::sync_channel(0);
        let mut timeout = Duration::from_secs(0);

        let event_loop = self.inner.register()?;

        tokio::spawn(async move {
            loop {
                let result = event_loop.poll(timeout);
                tx.send(result)
                    .expect("should have been able to send result");
            }
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {}
