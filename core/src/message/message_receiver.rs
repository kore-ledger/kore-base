use futures::StreamExt;
use rmp_serde::Deserializer;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::io::Cursor;
use tokio::sync::mpsc::{self};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

use crate::{commons::channel::SenderEnd, signature::Signed, KeyIdentifier};

use network::{Event as NetworkEvent, NetworkState};

use super::{MessageContent, TaskCommandContent};

/// A message receiver that listens for incoming messages and forwards them to the sender.
pub struct MessageReceiver<T>
where
    T: TaskCommandContent + Serialize + DeserializeOwned,
{
    receiver: ReceiverStream<NetworkEvent>,
    sender: SenderEnd<Signed<MessageContent<T>>, ()>,
    token: CancellationToken,
    own_id: KeyIdentifier,
}

impl<T: TaskCommandContent + Serialize + DeserializeOwned + 'static> MessageReceiver<T> {
    pub fn new(
        receiver: mpsc::Receiver<NetworkEvent>,
        sender: SenderEnd<Signed<MessageContent<T>>, ()>,
        token: CancellationToken,
        own_id: KeyIdentifier,
    ) -> Self {
        let receiver = ReceiverStream::new(receiver);
        Self {
            receiver,
            sender,
            token,
            own_id,
        }
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                event = self.receiver.next() => if let Some(event) = event {
                        match event {
                           NetworkEvent::MessageReceived {message, ..} => {
                                // The message will be a string for now
                                // Deserialize the message
                                let cur = Cursor::new(message);
                                let mut de = Deserializer::new(cur);
                                let message: Signed<MessageContent<T>> = Deserialize::deserialize(&mut de).expect("Fallo de deserializaciÃ³n");
                                // Check message signature
                                if message.verify().is_err() || message.content.sender_id != message.signature.signer {
                                    log::error!("Invalid signature in message");
                                } else if message.content.receiver != self.own_id {
                                    log::error!("Message not for me");
                                } else {
                                    self.sender.tell(message).await.expect("Channel Error");
                                }
                            },
                            NetworkEvent::ConnectedToBootstrap { .. } => {
                                println!("NETWORK EVENT CONNECTEDTOBOOTSTRAP");
                            },
                            NetworkEvent::MessageSent { .. } => {
                                println!("NETWORK EVENT MESSAGESENT");
                            },
                            NetworkEvent::PeerIdentified { .. } => {
                                println!("NETWORK EVENT PEERIDENTIFIED");
                            },
                            NetworkEvent::StateChanged(state) => {
                            match state {
                                NetworkState::Start=> {
                                    println!("NETWORK EVENT STATECHANGED START");
                                },
                                NetworkState::Dial => {
                                    println!("NETWORK EVENT STATECHANGED DIAL");
                                },
                                NetworkState::Dialing => {
                                    println!("NETWORK EVENT STATECHANGED DIALING");
                                },
                                NetworkState::Running => {
                                    println!("NETWORK EVENT STATECHANGED RUNNING");
                                },
                                NetworkState::Disconnected => {
                                    println!("NETWORK EVENT STATECHANGED DISCONNECTED");
                                    break;
                                }
                            }
                           },
                           NetworkEvent::Error(e) => {
                            println!("NETWORK EVENT ERROR");
                            println!("{:?}", e);
                           },
                        }
                },
                _ = self.token.cancelled() => {
                    log::info!("Message receiver module shutdown received");
                    break;
                }
            }
        }
        self.token.cancel();
        log::info!("Ended");
    }
}
