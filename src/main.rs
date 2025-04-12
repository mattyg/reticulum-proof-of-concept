use anyhow::Result;
use reticulum_poc::Chatter;

/// Reticulum-rs demo chat app
#[derive(clap::Parser)]
struct Args {
    /// The nickname you'd like to use.
    nick: String,

    /// The topic to chat about
    /// Each topic is a separate chat
    topic: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).init();

    let args = <Args as clap::Parser>::parse();

    let chatter = Chatter::new(args.nick.clone(), args.topic.clone()).await;

    // Receive cli input
    let mut rl = rustyline::DefaultEditor::new()?;
    let mut input: String = "".to_string();
    loop {
        input = rl.readline(format!("{}> ", args.nick.clone()).as_str())?;
        chatter.chat(input.as_bytes()).await;
        println!("Chatted: {}", input);
    }
}
