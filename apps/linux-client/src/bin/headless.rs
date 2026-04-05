use anyhow::{Context, Result, anyhow};
use sharkord_linux_client::native_client::{
    NativeBootstrap, NativeClient, NativeEventEnvelope, NativeMessage, NativeMessagesResponse,
    NativeSearchResults, first_text_channel_id,
};
use std::env;

#[derive(Debug)]
struct Config {
    server: String,
    username: String,
    password: String,
    server_password: Option<String>,
    channel_id: Option<u64>,
    send_message: Option<String>,
    get_message_id: Option<u64>,
    search_query: Option<String>,
    edit_message_id: Option<u64>,
    edit_content: Option<String>,
    delete_message_id: Option<u64>,
    no_stream: bool,
}

fn parse_args() -> Result<Config> {
    let mut args = env::args().skip(1);

    let mut server = None;
    let mut username = None;
    let mut password = None;
    let mut server_password = None;
    let mut channel_id = None;
    let mut send_message = None;
    let mut get_message_id = None;
    let mut search_query = None;
    let mut edit_message_id = None;
    let mut edit_content = None;
    let mut delete_message_id = None;
    let mut no_stream = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--server" => server = args.next(),
            "--username" => username = args.next(),
            "--password" => password = args.next(),
            "--server-password" => server_password = args.next(),
            "--channel" => {
                channel_id = args
                    .next()
                    .map(|value| value.parse::<u64>())
                    .transpose()
                    .context("invalid --channel value")?;
            }
            "--send" => send_message = args.next(),
            "--get-message" => {
                get_message_id = args
                    .next()
                    .map(|value| value.parse::<u64>())
                    .transpose()
                    .context("invalid --get-message value")?;
            }
            "--search" => search_query = args.next(),
            "--edit-message" => {
                edit_message_id = args
                    .next()
                    .map(|value| value.parse::<u64>())
                    .transpose()
                    .context("invalid --edit-message value")?;
            }
            "--edit-content" => edit_content = args.next(),
            "--delete-message" => {
                delete_message_id = args
                    .next()
                    .map(|value| value.parse::<u64>())
                    .transpose()
                    .context("invalid --delete-message value")?;
            }
            "--no-stream" => no_stream = true,
            "--help" | "-h" => {
                println!(
                    "Usage: cargo run --manifest-path apps/linux-client/Cargo.toml --bin headless -- \\
  --server https://chat.zooi.org \\
  --username dan \\
  --password secret \\
  [--server-password serverpass] \\
  [--channel 123] \\
  [--send \"hello from native\"] \\
  [--search \"hello\"] \\
  [--get-message 42] \\
  [--edit-message 42 --edit-content \"updated text\"] \\
  [--delete-message 42] \\
  [--no-stream]"
                );

                std::process::exit(0);
            }
            unknown => {
                return Err(anyhow!("unknown argument: {unknown}"));
            }
        }
    }

    Ok(Config {
        server: server.context("missing --server")?,
        username: username.context("missing --username")?,
        password: password.context("missing --password")?,
        server_password,
        channel_id,
        send_message,
        get_message_id,
        search_query,
        edit_message_id,
        edit_content,
        delete_message_id,
        no_stream,
    })
}

fn print_bootstrap_summary(bootstrap: &NativeBootstrap) {
    println!("Connected to: {}", bootstrap.server_name);
    println!("Own user id: {}", bootstrap.own_user_id);
    println!("Visible users: {}", bootstrap.users.len());
    println!("Channels:");

    for channel in &bootstrap.channels {
        let name = channel.name.as_deref().unwrap_or("(unnamed)");
        println!("  [{}] {} ({})", channel.id, name, channel.kind);
    }
}

fn print_messages(messages: &NativeMessagesResponse) {
    if messages.messages.is_empty() {
        println!("No recent root messages in selected channel.");
        return;
    }

    println!("Recent root messages:");

    for message in messages.messages.iter().take(10).rev() {
        println!(
            "  #{} user={} at {} -> {}",
            message.id, message.user_id, message.created_at, message.content
        );
    }
}

fn print_message(message: &NativeMessage) {
    println!(
        "Message #{} user={} at {} -> {}",
        message.id, message.user_id, message.created_at, message.content
    );
}

fn print_search_results(results: &NativeSearchResults) {
    if results.messages.is_empty() && results.files.is_empty() {
        println!("No search results.");
        return;
    }

    if !results.messages.is_empty() {
        println!("Matched messages:");

        for message in &results.messages {
            println!(
                "  #{} [{}] {} -> {}",
                message.id, message.created_at, message.channel_name, message.plain_content
            );
        }
    }

    if !results.files.is_empty() {
        println!("Matched files:");

        for file in &results.files {
            println!(
                "  msg={} [{}] {}",
                file.message_id, file.message_created_at, file.channel_name
            );
        }
    }
}

fn print_event(event: NativeEventEnvelope) {
    println!("event {} -> {}", event.event_type, event.payload);
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = parse_args()?;
    let client = NativeClient::new(&config.server)?;

    let token = client.login(&config.username, &config.password).await?;
    let bootstrap = client
        .bootstrap(&token, config.server_password.as_deref())
        .await?;

    print_bootstrap_summary(&bootstrap);

    let selected_channel_id = config
        .channel_id
        .or_else(|| first_text_channel_id(&bootstrap))
        .context("no text channel available; pass --channel explicitly")?;

    let messages = client.fetch_messages(&token, selected_channel_id).await?;
    print_messages(&messages);

    if let Some(message) = &config.send_message {
        let sent = client
            .send_message(&token, selected_channel_id, message, None, None)
            .await?;

        println!("Sent message id {}", sent.message_id);
    }

    if let Some(message_id) = config.get_message_id {
        let message = client.get_message(&token, message_id).await?;
        print_message(&message);
    }

    if let Some(query) = &config.search_query {
        let results = client.search_messages(&token, query).await?;
        print_search_results(&results);
    }

    if let Some(message_id) = config.edit_message_id {
        let content = config
            .edit_content
            .as_deref()
            .context("--edit-message requires --edit-content")?;

        client.edit_message(&token, message_id, content).await?;
        println!("Edited message {}", message_id);
    }

    if let Some(message_id) = config.delete_message_id {
        client.delete_message(&token, message_id).await?;
        println!("Deleted message {}", message_id);
    }

    if config.no_stream {
        return Ok(());
    }

    println!("Event stream connected. Waiting for updates...");

    client
        .stream_events(&token, |event| {
            print_event(event);
            Ok(())
        })
        .await
}
