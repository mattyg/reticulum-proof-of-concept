use anyhow::Result;
use chatter_app::Chatter;

/// Reticulum-rs demo chat app
#[derive(clap::Parser)]
struct Args {
    /// The nickname you'd like to use.
    #[arg(short, long)]
    nick: String,

    /// The topic to chat about
    /// Each topic is a separate chat
    #[arg(short, long, default_value = "general")]
    topic: String,

    /// Host name & port of the TCPServer
    /// i.e. absolute URL excluding protocol
    #[arg(short, long, default_value = "reticulum.betweentheborders.com:4242")]
    server_host: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).init();
    let args = <Args as clap::Parser>::parse();

    // Start app
    let chatter = Chatter::new(args.server_host.clone(), args.nick.clone(), args.topic.clone()).await;

    // Receive cli input
    let mut rl = rustyline::DefaultEditor::new()?;
    let mut input: String = "".to_string();
    loop {
        input = rl.readline(format!("{}> ", args.nick.clone()).as_str())?;
        chatter.chat(input.as_bytes()).await;
    }
}
