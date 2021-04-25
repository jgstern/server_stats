use futures::future::{BoxFuture, FutureExt};
use matrix_sdk::{
    self, async_trait,
    events::{
        room::message::{MessageEventContent, MessageType, TextMessageEventContent},
        AnyMessageEvent, AnyRoomEvent, SyncMessageEvent,
    },
    identifiers::RoomIdOrAliasId,
    room::Room,
    Client, ClientConfig, EventHandler, Session as SdkSession, SyncSettings,
};
use matrix_sdk::{
    api::r0::message::get_message_events::Request as MessagesRequest,
    events::{room::member::MemberEventContent, StrippedStateEvent},
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{convert::TryFrom, path::PathBuf};
use tokio::time::{sleep, Duration};
use tracing::{error, info};
use url::Url;

#[derive(Debug, Clone)]
struct VoyagerBot {
    client: Client,
}

impl VoyagerBot {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}
#[async_trait]
impl EventHandler for VoyagerBot {
    async fn on_stripped_state_member(
        &self,
        room: Room,
        room_member: &StrippedStateEvent<MemberEventContent>,
        _: Option<MemberEventContent>,
    ) {
        if room_member.state_key != self.client.user_id().await.unwrap() {
            return;
        }

        if let Room::Invited(room) = room {
            info!("Autojoining room {}", room.room_id());
            let mut delay = 2;

            while let Err(err) = room.accept_invitation().await {
                // retry autojoin due to synapse sending invites, before the
                // invited user can join for more information see
                // https://github.com/matrix-org/synapse/issues/4345
                eprintln!(
                    "Failed to join room {} ({:?}), retrying in {}s",
                    room.room_id(),
                    err,
                    delay
                );

                sleep(Duration::from_secs(delay)).await;
                delay *= 2;

                if delay > 3600 {
                    error!("Can't join room {} ({:?})", room.room_id(), err);
                    break;
                }
            }
            info!("Successfully joined room {}", room.room_id());
        }
    }
    async fn on_room_message(&self, room: Room, event: &SyncMessageEvent<MessageEventContent>) {
        if let Room::Joined(_) = room {
            let msg_body = if let SyncMessageEvent {
                content:
                    MessageEventContent {
                        msgtype: MessageType::Text(TextMessageEventContent { body: msg_body, .. }),
                        ..
                    },
                ..
            } = event
            {
                msg_body
            } else {
                return;
            };

            // Handle message
            let cloned_self = self.clone();
            let cloned_msg_body = msg_body.clone();
            tokio::spawn(async move {
                cloned_self.process_message(cloned_msg_body).await;
            });
        }
    }
}

impl VoyagerBot {
    // TODO save relations
    fn process_message(&self, msg_body: String) -> BoxFuture<'_, ()> {
        async move {
            // Regex is taken from https://github.com/turt2live/matrix-voyager-bot/blob/c6c9a1f2b2ee7b3a531a70646375915e5f6e4000/src/VoyagerBot.js#L96
            let re = Regex::new(r"[#!][a-zA-Z0-9.\-_#=]+:[a-zA-Z0-9.\-_]+[a-zA-Z0-9]").unwrap();
            if !re.is_match(&msg_body) {
                return;
            }
            for cap in re.captures_iter(&msg_body) {
                // Got link
                info!("New room: {}", &cap[0]);
                let room_id_or_alias = RoomIdOrAliasId::try_from(&cap[0]).unwrap();
                let room_id = match self
                    .client
                    .join_room_by_id_or_alias(&room_id_or_alias, &[])
                    .await
                {
                    Ok(resp) => Some(resp.room_id),
                    Err(e) => {
                        error!("Failed to join room: {}", e);
                        None
                    }
                };

                if let Some(room_id) = room_id {
                    sleep(Duration::from_secs(5)).await;
                    if let Some(Room::Joined(room)) = self.client.get_room(&room_id) {
                        let prev_batch = room.last_prev_batch().unwrap();
                        let request = MessagesRequest::backward(&room_id, &prev_batch);
                        let resp = room
                            .messages(request)
                            .await
                            .expect("failed to get older events");
                        let mut chunk = resp.chunk;
                        while !chunk.is_empty() {
                            for message in chunk {
                                let deserialized_message = message.deserialize();
                                if let Ok(AnyRoomEvent::Message(AnyMessageEvent::RoomMessage(
                                    message,
                                ))) = deserialized_message
                                {
                                    let sender = message.sender;
                                    if self.client.user_id().await.unwrap() == sender {
                                        continue;
                                    }

                                    let content = message.content.msgtype;
                                    if let MessageType::Text(text_content) = content {
                                        let cloned_self = self.clone();
                                        tokio::spawn(async move {
                                            cloned_self.process_message(text_content.body).await;
                                        });
                                    }
                                }
                            }
                            // Try further
                            let prev_batch = resp.end.clone().unwrap();
                            let request = MessagesRequest::backward(&room_id, &prev_batch);
                            let previous = room
                                .messages(request)
                                .await
                                .expect("failed to get older events");
                            chunk = previous.chunk;
                        }
                    }
                }
            }
        }
        .boxed()
    }
}

pub async fn login_and_sync(
    homeserver_url: String,
    username: String,
    password: String,
) -> Result<(), matrix_sdk::Error> {
    let client_config = ClientConfig::new().store_path("./store/");

    let homeserver_url = Url::parse(&homeserver_url).expect("Couldn't parse the homeserver URL");
    // create a new Client with the given homeserver url and config
    let client = Client::new_with_config(homeserver_url, client_config).unwrap();

    if let Some(session) = Session::load() {
        info!("Starting relogin");

        let session = SdkSession {
            access_token: session.access_token,
            device_id: session.device_id.into(),
            user_id: matrix_sdk::identifiers::UserId::try_from(session.user_id.as_str()).unwrap(),
        };

        if let Err(e) = client.restore_login(session).await {
            error!("{}", e);
        };
        info!("Finished relogin");
    } else {
        info!("Starting login");
        let login_response = client
            .login(
                &username,
                &password,
                None,
                Some(&"server_stats-discovery-bot".to_string()),
            )
            .await;
        match login_response {
            Ok(login_response) => {
                info!("Session: {:#?}", login_response);
                let session = Session {
                    homeserver: client.homeserver().to_string(),
                    user_id: login_response.user_id.to_string(),
                    access_token: login_response.access_token,
                    device_id: login_response.device_id.into(),
                };
                session.save().expect("Unable to persist session");
            }
            Err(e) => error!("Error while login: {}", e),
        }
        info!("Finished login");
    }

    info!("logged in as {}", username);

    client
        .set_event_handler(Box::new(VoyagerBot::new(client.clone())))
        .await;

    client.sync(SyncSettings::default()).await;
    println!("failed");
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    /// The homeserver used for this session.
    pub homeserver: String,
    /// The access token used for this session.
    pub access_token: String,
    /// The user the access token was issued for.
    pub user_id: String,
    /// The ID of the client device
    pub device_id: String,
}

impl Session {
    pub fn save(&self) -> color_eyre::Result<()> {
        let mut session_path = PathBuf::from("./store/session.json");
        info!("SessionPath: {:?}", session_path);
        std::fs::create_dir_all(&session_path)?;
        session_path.push("session.json");
        serde_json::to_writer(&std::fs::File::create(session_path)?, self)?;
        Ok(())
    }

    pub fn load() -> Option<Self> {
        let mut session_path = PathBuf::from("./store/session.json");
        session_path.push("session.json");
        let file = std::fs::File::open(session_path);
        match file {
            Ok(file) => {
                let session: Result<Self, serde_json::Error> = serde_json::from_reader(&file);
                match session {
                    Ok(session) => Some(session),
                    Err(_) => None,
                }
            }
            Err(_) => None,
        }
    }
}
