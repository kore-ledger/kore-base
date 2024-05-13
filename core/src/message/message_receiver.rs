use futures::StreamExt;
use rmp_serde::Deserializer;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::io::Cursor;
use tokio::sync::mpsc::{self};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

use crate::{commons::channel::SenderEnd, signature::Signed, KeyIdentifier, Notification};

use network::Event as NetworkEvent;

use super::{MessageContent, TaskCommandContent};

/// A message receiver that listens for incoming messages and forwards them to the sender.
pub struct MessageReceiver<T>
where
    T: TaskCommandContent + Serialize + DeserializeOwned,
{
    receiver: ReceiverStream<NetworkEvent>,
    sender: SenderEnd<Signed<MessageContent<T>>, ()>,
    token: CancellationToken,
    // TODO: Could be removed?
    _notification_tx: tokio::sync::mpsc::Sender<Notification>,
    own_id: KeyIdentifier,
}

impl<T: TaskCommandContent + Serialize + DeserializeOwned + 'static> MessageReceiver<T> {
    pub fn new(
        receiver: mpsc::Receiver<NetworkEvent>,
        sender: SenderEnd<Signed<MessageContent<T>>, ()>,
        token: CancellationToken,
        notification_tx: tokio::sync::mpsc::Sender<Notification>,
        own_id: KeyIdentifier,
    ) -> Self {
        let receiver = ReceiverStream::new(receiver);
        Self {
            receiver,
            sender,
            token,
            _notification_tx: notification_tx,
            own_id,
        }
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                event = self.receiver.next() => if let Some(NetworkEvent::MessageReceived { message, .. }) = event {
                    // The message will be a string for now
                    // Deserialize the message
                    let cur = Cursor::new(message);
                    let mut de = Deserializer::new(cur);
                    let message: Signed<MessageContent<T>> = Deserialize::deserialize(&mut de).expect("Fallo de deserialización");
                    // Check message signature
                    if message.verify().is_err() || message.content.sender_id != message.signature.signer {
                        log::error!("Invalid signature in message");
                    } else if message.content.receiver != self.own_id {
                        log::error!("Message not for me");
                    } else {
                        self.sender.tell(message).await.expect("Channel Error");
                    }
                } else {
                    log::debug!("Shutdown received");
                    break;
                }
            }
        }
        self.token.cancel();
        log::info!("Ended");
    }
}
