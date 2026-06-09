use std::fmt;

use crate::commands::{CommandExecutionError, execute_command, parse_command};
use crate::store::StoreHandle;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboundTextMessage {
    reply_target: String,
    message_id: String,
    text: String,
}

impl InboundTextMessage {
    pub fn new(reply_target: String, message_id: String, text: String) -> Self {
        Self {
            reply_target,
            message_id,
            text,
        }
    }

    pub fn reply_target(&self) -> &str {
        &self.reply_target
    }

    pub fn message_id(&self) -> &str {
        &self.message_id
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

pub async fn process_inbound_text_messages<SendReply, SendReplyFuture, ReplyError>(
    store: &StoreHandle,
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
        let reply = execute_command(store, command)?;
        println!("{reply}");
        if let Err(error) = send_reply(message.reply_target().to_owned(), reply).await {
            eprintln!("Failed to send {provider_name} reply: {error}");
        }
    }

    Ok(())
}
