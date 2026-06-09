use std::fmt;

use crate::commands::{Command, CommandExecutionError, execute_command, parse_command};
use crate::config::SecretString;
use crate::store::StoreHandle;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboundTextMessage {
    reply_target: String,
    auth_identity: ChatAuthIdentity,
    message_id: String,
    text: String,
}

impl InboundTextMessage {
    pub fn new(
        reply_target: String,
        auth_identity: ChatAuthIdentity,
        message_id: String,
        text: String,
    ) -> Self {
        Self {
            reply_target,
            auth_identity,
            message_id,
            text,
        }
    }

    pub fn reply_target(&self) -> &str {
        &self.reply_target
    }

    pub fn auth_identity(&self) -> &ChatAuthIdentity {
        &self.auth_identity
    }

    pub fn message_id(&self) -> &str {
        &self.message_id
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatAuthIdentity {
    provider: String,
    sender_id: String,
}

impl ChatAuthIdentity {
    pub fn new(provider: impl Into<String>, sender_id: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            sender_id: sender_id.into(),
        }
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn sender_id(&self) -> &str {
        &self.sender_id
    }
}

pub async fn process_inbound_text_messages<SendReply, SendReplyFuture, ReplyError>(
    store: &StoreHandle,
    chat_auth_key: Option<&SecretString>,
    provider_name: &str,
    messages: Vec<InboundTextMessage>,
    mut send_reply: SendReply,
) -> Result<(), CommandExecutionError>
where
    SendReply: FnMut(String, String) -> SendReplyFuture,
    SendReplyFuture: Future<Output = Result<(), ReplyError>>,
    ReplyError: fmt::Display,
{
    for message in messages {
        let _ = message.message_id();
        let command = parse_command(message.text());
        let Some(reply) =
            process_authorized_command(store, chat_auth_key, message.auth_identity(), command)?
        else {
            continue;
        };
        println!("{reply}");
        if let Err(error) = send_reply(message.reply_target().to_owned(), reply).await {
            eprintln!("Failed to send {provider_name} reply: {error}");
        }
    }

    Ok(())
}

fn process_authorized_command(
    store: &StoreHandle,
    chat_auth_key: Option<&SecretString>,
    auth_identity: &ChatAuthIdentity,
    command: Command,
) -> Result<Option<String>, CommandExecutionError> {
    match command {
        Command::Login(Some(key)) if chat_auth_key.is_some_and(|secret| secret.as_str() == key) => {
            if store
                .is_chat_sender_authorized(auth_identity.provider(), auth_identity.sender_id())?
            {
                return Ok(Some("Already logged in.".to_owned()));
            }

            store.authorize_chat_sender(auth_identity.provider(), auth_identity.sender_id())?;
            Ok(Some("Logged in.".to_owned()))
        }
        Command::Login(_) => Ok(None),
        command => {
            if !store
                .is_chat_sender_authorized(auth_identity.provider(), auth_identity.sender_id())?
            {
                return Ok(None);
            }

            if command == Command::Logout {
                store
                    .deauthorize_chat_sender(auth_identity.provider(), auth_identity.sender_id())?;
                return Ok(Some("Logged out.".to_owned()));
            }

            execute_command(store, command).map(Some)
        }
    }
}
