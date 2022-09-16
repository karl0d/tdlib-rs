use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tdlib::{
    self,
    enums::{AuthorizationState, InputMessageContent, Update, self},
    functions,
    types::{FormattedText, InputMessageText, TdlibParameters},
};
use tokio::sync::mpsc::{self, Receiver, Sender};

fn ask_user(string: &str) -> String {
    println!("{}", string);
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    input.trim().to_string()
}

async fn handle_update(update: Update, auth_tx: &Sender<AuthorizationState>, client_id: i32) {
    match update {
        Update::AuthorizationState(update) => {
            auth_tx.send(update.authorization_state).await.unwrap();
        }
        Update::NewChat(data) => {
            let chat = data.chat;
            let title = chat.title;
            println!("chat: {title}");
        }
        Update::Supergroup(data) => {
            let supergroup = data.supergroup;
            let username = supergroup.username;
            println!("username: {username}");
        }
        Update::User(data) => {
            let user = data.user;
            let name = user.first_name + " " + &user.last_name;
            let username = user.username;
            println!("user: {name}");
            println!("username: {username}");
        }
        Update::NewMessage(data) => {
            let message = data.message;
            let chat_id = message.chat_id;
            let content = message.content;
            println!("message: {content:?}");
            let msg = ask_user("Do you want to reply? if not leave empty");
            if !msg.is_empty() {
                reply(msg, chat_id, message.id, client_id).await;
            }
        }
        _ => (),
    }
}

async fn handle_authorization_state(
    client_id: i32,
    mut auth_rx: Receiver<AuthorizationState>,
    run_flag: Arc<AtomicBool>,
) -> Receiver<AuthorizationState> {
    while let Some(state) = auth_rx.recv().await {
        match state {
            AuthorizationState::WaitTdlibParameters => {
                let parameters = TdlibParameters {
                    database_directory: "bot_db".to_string(),
                    api_id: env!("API_ID").parse::<i32>().unwrap(),
                    api_hash: env!("API_HASH").to_string(),
                    system_language_code: "en".to_string(),
                    device_model: "Desktop".to_string(),
                    application_version: "0.1".to_string(),
                    ..Default::default()
                };

                let response = functions::set_tdlib_parameters(parameters, client_id).await;
                if let Err(error) = response {
                    println!("{}", error.message);
                }
            }
            AuthorizationState::WaitPhoneNumber => loop {
                let response = functions::check_authentication_bot_token(
                    env!("BOT_TOKEN").to_string(),
                    client_id,
                )
                .await;
                match response {
                    Ok(_) => break,
                    Err(e) => println!("{}", e.message),
                }
            },
            AuthorizationState::Ready => {
                break;
            }
            AuthorizationState::Closed => {
                // Set the flag to false to stop receiving updates from the
                // spawned task
                run_flag.store(false, Ordering::Release);
                break;
            }
            AuthorizationState::Closing => {
                println!("error 500");
                break;
            }
            AuthorizationState::WaitEncryptionKey(_) => {
                let response = functions::check_database_encryption_key(
                    option_env!("PASSWD").unwrap_or_default().to_string(),
                    client_id,
                )
                .await;
                match response {
                    Ok(_) => (),
                    Err(e) => println!("{}", e.message),
                }
            }
            _ => (),
        }
    }

    auth_rx
}

#[tokio::main]
async fn main() {
    // Create the client object
    let client_id = tdlib::create_client();

    // Create a mpsc channel for handling AuthorizationState updates separately
    // from the task
    let (auth_tx, auth_rx) = mpsc::channel(5);

    // Create a flag to make it possible to stop receiving updates
    let run_flag = Arc::new(AtomicBool::new(true));
    let run_flag_clone = run_flag.clone();

    // Spawn a task to receive updates/responses
    let handle = tokio::spawn(async move {
        while run_flag_clone.load(Ordering::Acquire) {
            if let Some((update, _client_id)) = tdlib::receive() {
                handle_update(update, &auth_tx, client_id).await;
            }
        }
    });

    // Set a fairly low verbosity level. We mainly do this because tdlib
    // requires to perform a random request with the client to start receiving
    // updates for it.
    functions::set_log_verbosity_level(2, client_id)
        .await
        .unwrap();

    // Handle the authorization state to authenticate the client
    let auth_rx = handle_authorization_state(client_id, auth_rx, run_flag.clone()).await;

    println!("ready");

    // Run the get_me() method to get user informations
    // FIXME: Delete this once moved to tdjson 1.8.5
    // The code below crashes when tring to get bot info on tdjson 1.8.2,
    // but from my testing it does work fine when tdjson 1.8.5 is installed
    // Please keep in mind that tdjson 1.8.5 is not fully compatiple with tdlib-rs 0.2
    // and will likely break telegrand so I would suggest to wait for the new tdlib Release

    let me_response = functions::get_me(client_id).await;
    match me_response {
        Ok(data) => {
            let enums::User::User(user) = data;
            println!("Hi, I'm {}", user.first_name);
        }
        Err(e) => println!("-{}", e.message),
    }

    // Tell the client to close
    //functions::close(client_id).await.unwrap();

    // Handle the authorization state to wait for the "Closed" state
    handle_authorization_state(client_id, auth_rx, run_flag.clone()).await;

    // Wait for the previously spawned task to end the execution
    handle.await.unwrap();
}

async fn reply(msg: String, chat_id: i64, reply_to_message_id: i64, client_id: i32) {
    let text = FormattedText {
        text: msg,
        ..Default::default()
    };
    let content = InputMessageText {
        text,
        disable_web_page_preview: true,
        clear_draft: true,
    };
    let input_message_content = InputMessageContent::InputMessageText(content);
    functions::send_message(
        chat_id,
        0,
        reply_to_message_id,
        None,
        None,
        input_message_content,
        client_id,
    )
    .await
    .expect("Failed to send a message");
    println!("message sent");
}