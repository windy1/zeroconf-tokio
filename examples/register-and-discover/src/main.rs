#[macro_use]
extern crate log;

use clap::Parser;
use zeroconf_tokio::prelude::*;
use zeroconf_tokio::MdnsBrowser;
use zeroconf_tokio::MdnsBrowserAsync;
use zeroconf_tokio::MdnsServiceAsync;
use zeroconf_tokio::{MdnsService, ServiceType, TxtRecord};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Name of the service type to register
    #[clap(short, long, default_value = "http")]
    name: String,

    /// Protocol of the service type to register
    #[clap(short, long, default_value = "tcp")]
    protocol: String,

    /// Sub-types of the service type to register
    #[clap(short, long)]
    sub_types: Vec<String>,
}

#[tokio::main]
async fn main() -> zeroconf_tokio::Result<()> {
    env_logger::init();

    let Args {
        name,
        protocol,
        sub_types,
    } = Args::parse();

    let sub_types = sub_types.iter().map(|s| s.as_str()).collect::<Vec<_>>();
    let service_type = ServiceType::with_sub_types(&name, &protocol, sub_types)?;
    let mut service = MdnsService::new(service_type.clone(), 8080);
    let mut txt_record = TxtRecord::new();

    txt_record.insert("foo", "bar")?;

    service.set_name("zeroconf_example_service");
    service.set_txt_record(txt_record);

    let mut service = MdnsServiceAsync::new(service, None)?;

    let result = service.start().await?;

    info!("Registered service: {:?}", result);

    let mut browser = MdnsBrowserAsync::new(MdnsBrowser::new(service_type), None)?;

    browser.start().await?;

    while let Some(Ok(discovery)) = browser.next().await {
        info!("Discovered service: {:?}", discovery);
        service.shutdown().await;
        browser.shutdown().await;
    }

    Ok(())
}
