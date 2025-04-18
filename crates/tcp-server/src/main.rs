use anyhow::Result;
use reticulum::{destination::DestinationName, identity::PrivateIdentity, iface::tcp_server::TcpServer, transport::{Transport, TransportConfig}};

/// Reticulum-rs demo chat app
#[derive(clap::Parser)]
struct Args {
    /// Port to run the server on
    #[arg(short, long, default_value_t = 8888)]
    port: u16,

    /// Host to run server on
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).init();
    let args = <Args as clap::Parser>::parse();
    let full_host = format!("{}:{}", args.host, args.port);

    // Create the Transport
    let mut transport = Transport::new(TransportConfig::new("server", false));
    let id = PrivateIdentity::new_from_name("server");
    transport
        .add_destination(id, DestinationName::new("chatter", "hub"))
        .await;

    // Add the Tcp client interfaces
    transport.iface_manager().lock().await.spawn(
        TcpServer::new(full_host.clone(), transport.iface_manager()),
        TcpServer::spawn,
    );

    log::info!("Tcp server running at {}", full_host.clone());
    let _ = tokio::signal::ctrl_c().await;
    Ok(())
}
