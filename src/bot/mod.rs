use chrono::{prelude::*, Duration as ChronoDuration};
use matrix_sdk::{
    self,
    api::r0::config::get_global_account_data::Request as GlobalAccountDataGetRequest,
    api::r0::config::set_global_account_data::Request as GlobalAccountDataSetRequest,
    api::r0::filter::RoomEventFilter,
    api::r0::{
        filter::{FilterDefinition, LazyLoadOptions, RoomFilter},
        message::get_message_events::{Direction, Request as MessagesRequest},
        sync::sync_events::Filter,
    },
    async_trait,
    events::{
        custom::CustomEventContent,
        direct::DirectEventContent,
        room::member::MemberEventContent,
        room::message::{MessageEventContent, MessageType, TextMessageEventContent},
        AnyMessageEvent, AnyMessageEventContent, AnyRoomEvent, AnyStateEventContent,
        StrippedStateEvent, SyncMessageEvent,
    },
    identifiers::{EventId, RoomId, RoomIdOrAliasId},
    room::{Joined, Room},
    uint, Client, ClientConfig, EventHandler, Raw, Session as SdkSession, SyncSettings,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, convert::TryFrom, path::PathBuf};
use tokio::time::{sleep, Duration};
use tracing::{error, info};
use url::Url;
#[derive(Debug, Clone)]
struct VoyagerBot {
    client: Client,
}

impl VoyagerBot {
    pub fn new(client: Client) -> Self {
        info!("Got new bot");
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
            if let Some(is_direct) = room_member.content.is_direct {
                if is_direct {
                    let user_id = self.client.user_id().await.unwrap();
                    let get_req = GlobalAccountDataGetRequest::new(&user_id, "m.direct");

                    let sender = &room_member.sender;
                    match self.client.send(get_req, None).await {
                        Ok(old_direct) => {
                            let raw = Raw::<DirectEventContent>::from_json(
                                old_direct.account_data.into_json(),
                            );
                            let deserialized_message = raw.deserialize();
                            match deserialized_message {
                                Ok(mut contents) => {
                                    info!("deserialized");
                                    if contents.contains_key(sender) {
                                        let mut cloned_contents = contents.clone();
                                        let raw_content = cloned_contents.get_mut(sender);
                                        if let Some(content) = raw_content {
                                            content.push(room.room_id().clone());
                                            contents.insert(sender.clone(), content.clone());
                                        }
                                    }

                                    let rawed_contents: Raw<DirectEventContent> = contents.into();
                                    // make new event
                                    let set_request = GlobalAccountDataSetRequest::new(
                                        rawed_contents.json(),
                                        "m.direct",
                                        &user_id,
                                    );
                                    if let Err(e) = self.client.send(set_request, None).await {
                                        error!("Failed to set m.direct: {}", e);
                                    }
                                }
                                Err(e) => {
                                    error!("unable to deserialize: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to get m.direct: {}", e);
                            let mut hashmap: BTreeMap<String, serde_json::Value> = BTreeMap::new();
                            hashmap.insert(
                                sender.to_string(),
                                serde_json::json!(vec![serde_json::json!(room.room_id().as_str())]),
                            );
                            let rawed_contents: Raw<BTreeMap<String, serde_json::Value>> =
                                hashmap.into();
                            // make new event
                            let set_request = GlobalAccountDataSetRequest::new(
                                rawed_contents.json(),
                                "m.direct",
                                &user_id,
                            );
                            if let Err(e) = self.client.send(set_request, None).await {
                                error!("Failed to set m.direct: {}", e);
                            }
                        }
                    }
                }
            }
            info!("Successfully joined room {}", room.room_id());
            // TODO scan joined room
        }
    }
    async fn on_room_message(&self, room: Room, event: &SyncMessageEvent<MessageEventContent>) {
        if let Room::Joined(room) = room {
            let msg_body = if let SyncMessageEvent {
                content:
                    MessageEventContent {
                        msgtype: MessageType::Text(TextMessageEventContent { body: msg_body, .. }),
                        ..
                    },
                ..
            } = event
            {
                msg_body.clone()
            } else {
                return;
            };
            let event_id = event.event_id.clone();

            //info!("msg_body: {}, is_direct: {}", msg_body, room.is_direct());
            if msg_body.contains("!help") && room.is_direct() {
                info!("Sending help");
                room.typing_notice(true)
                    .await
                    .expect("Can't send typing event");
                let content = AnyMessageEventContent::RoomMessage(
                    MessageEventContent::notice_html(
                        r#"Hi! I am the server_stats Discovery bot by @mtrnord:nordgedanken.dev ! \n\n\n
                    What am I doing?\n\n I am a bot discovering matrix rooms. I am just looking for tasty room aliases :) I do not save your content!\n\n
                    How do I get removed?\n\n Its simple! Just ban me and I will not come back :)\n\n
                    Where can I learn more?\n\n You can either look at my source over at https://git.nordgedanken.dev/MTRNord/server_stats/-/tree/main or join #server_stats:nordgedanken.dev :)"#,
                        r#"<h1>Hi! I am the server_stats Discovery bot by <a href=\"https://matrix.to/#/@mtrnord:nordgedanken.dev\">MTRNord</a></h1>
                        <h2>What am I doing?</h2> I am a bot discovering matrix rooms. I am just looking for tasty room aliases :) I do not read the actual content or save it!
                        <h2>How do I get removed?</h2> Its simple! Just ban me and I will not come back :)
                        <h2>Where can I learn more?</h2> You can either look at my source over at https://git.nordgedanken.dev/MTRNord/server_stats/-/tree/main or join <a href=\"https://matrix.to/#/#server_stats:nordgedanken.dev\">#server_stats:nordgedanken.dev</a> :)"#,
                    ),
                );
                room.send(content, None).await.unwrap();

                room.typing_notice(false)
                    .await
                    .expect("Can't send typing event");
            }

            // Handle message
            let client = self.client.clone();
            tokio::spawn(async move {
                VoyagerBot::process_message(client, &msg_body, room, Some(event_id)).await;
            });
        }
    }
}

impl VoyagerBot {
    async fn cleanup(room_id: String) -> color_eyre::Result<()> {
        let now = Utc::now();
        let time = now - ChronoDuration::days(2);
        let timestamp = time.timestamp_millis();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        if let Some(ref bot) = crate::CONFIG.get().unwrap().bot {
            let server_address = &bot.homeserver_url;
            let map = serde_json::json!({"delete_local_events": false, "purge_up_to_ts":timestamp});
            let auth_header = format!("Bearer {}", bot.admin_access_token);
            let url = format!(
                "{}/_synapse/admin/v1/purge_history/{}",
                server_address, room_id
            );
            info!("{}", url);
            let body = client
                .post(url.clone())
                .header("Authorization", auth_header.clone())
                .json(&map)
                .send()
                .await?
                .text()
                .await?;

            info!("Started cleanup for: {} = {:?}", url, body);
        }
        Ok(())
    }
    async fn try_join(client: Client, room_alias: String, parent_room: Joined) -> Option<RoomId> {
        // TODO cache the room id of aliases if not tombstoned
        let room_id_or_alias = RoomIdOrAliasId::try_from(room_alias.clone());
        if let Ok(room_id_or_alias) = room_id_or_alias {
            let room_id = match client
                .join_room_by_id_or_alias(&room_id_or_alias, &[])
                .await
            {
                Ok(resp) => Some(resp.room_id),
                Err(e) => {
                    error!("Failed to join room ({}): {}", room_alias, e);
                    return None;
                }
            };
            if let Some(ref room_id) = room_id {
                let parent_id = parent_room.room_id().as_str();
                let parent_displayname = parent_room.display_name().await;
                if crate::CACHE_DB.graph.knows_room(room_id.as_str()) {
                    if let Some(parent) = crate::CACHE_DB.graph.get_parent(room_id.as_str()) {
                        if parent.as_ref() != parent_id {
                            // We know the room and only want to do the new relation
                            info!(
                                "New room relation for already known room: {:?} -> {}",
                                parent_displayname, room_alias
                            );
                            if let Err(e) =
                                crate::CACHE_DB.graph.add_child(parent_id, room_id.as_str())
                            {
                                error!("failed to save child: {}", e);
                            }
                        }
                    }
                    return None;
                }
                if let Err(e) = crate::CACHE_DB.graph.add_child(parent_id, room_id.as_str()) {
                    error!("failed to save child: {}", e);
                }

                info!(
                    "New room relation: {:?} -> {}",
                    parent_displayname, room_alias
                );
            }

            return room_id;
        }
        None
    }
    async fn search_new_room(client: Client, room_alias: String, parent_room: Joined) {
        // Got link
        // Join new room
        let room_id = VoyagerBot::try_join(client.clone(), room_alias, parent_room.clone()).await;

        // If we got the room_id continue
        if let Some(room_id) = room_id {
            // Wait for sync to roughly complete
            sleep(Duration::from_secs(5)).await;

            // Get the room object
            if let Some(Room::Joined(room)) = client.get_room(&room_id) {
                // Get one level back in history

                // Get prev_batch id
                let prev_batch = room.last_prev_batch();
                if let Some(prev_batch) = prev_batch {
                    // Make filter for what we care about
                    let mut filter = RoomEventFilter::empty();
                    let types = vec!["m.room.message".to_string()];
                    filter.types = Some(&types);

                    // Prepare request
                    let mut request =
                        MessagesRequest::new(&room_id, &prev_batch, Direction::Backward);
                    request.limit = uint!(30);
                    request.filter = Some(filter.clone());

                    // Run request
                    let resp = room.messages(request).await;

                    match resp {
                        Ok(resp) => {
                            // Iterate as long as chung is not empty
                            let mut chunk = resp.chunk;
                            let mut failed = false;

                            let mut from = prev_batch;
                            let mut end: String = resp.end.clone().unwrap();
                            while !chunk.is_empty() && !failed && from != end {
                                // For each message we recursivly do this again
                                for message in &chunk {
                                    let deserialized_message = message.deserialize();
                                    if let Ok(AnyRoomEvent::Message(
                                        AnyMessageEvent::RoomMessage(message),
                                    )) = deserialized_message
                                    {
                                        // Ignore messages sent by us
                                        let sender = message.sender;
                                        if client.user_id().await.unwrap() == sender {
                                            continue;
                                        }

                                        let content = message.content.msgtype;
                                        if let MessageType::Text(text_content) = content {
                                            let client = client.clone();

                                            // Make sure we explicitly do not want to wait on this
                                            {
                                                let parent_room = room.clone();
                                                tokio::spawn(async move {
                                                    VoyagerBot::process_message(
                                                        client,
                                                        &text_content.body,
                                                        parent_room,
                                                        None,
                                                    )
                                                    .await;
                                                });
                                            }
                                        }
                                    }
                                }

                                // TODO use if your synapse is bad again
                                //sleep(Duration::from_secs(2)).await;
                                // Try getting older messages
                                let mut request =
                                    MessagesRequest::new(&room_id, &end, Direction::Backward);
                                request.limit = uint!(30);
                                request.filter = Some(filter.clone());
                                let previous = room.messages(request).await;

                                if let Ok(previous) = previous {
                                    // Set new chunk to make sure we iterate the correct data in the next round
                                    chunk = previous.chunk;
                                    from = end;
                                    end = previous.end.clone().unwrap()
                                } else {
                                    failed = true;
                                }
                            }
                        }
                        Err(e) => {
                            // Todo remove room if `Http(FromHttpResponse(Http(Known(Error { kind: Forbidden, message: "Host not in room.", status_code: 403 }))))` is returned
                            error!("Failed to get older events: {}", e);
                        }
                    }
                }
            }
            tokio::spawn(async move {
                if let Err(e) = VoyagerBot::cleanup(room_id.to_string()).await {
                    error!("failed to clean: {}", e);
                }
            });
        }
    }

    #[async_recursion::async_recursion]
    async fn process_message(
        client: Client,
        msg_body: &str,
        room: Joined,
        event_id: Option<EventId>,
    ) {
        // Regex is taken from https://github.com/turt2live/matrix-voyager-bot/blob/c6c9a1f2b2ee7b3a531a70646375915e5f6e4000/src/VoyagerBot.js#L96
        let re = Regex::new(r"[#!][a-zA-Z0-9.\-_#=]+:[a-zA-Z0-9.\-_]+[a-zA-Z0-9]").unwrap();
        if !re.is_match(&msg_body) {
            return;
        }

        if let Some(event_id) = event_id {
            if let Err(e) = room.read_marker(&event_id, None).await {
                error!("Can't send read marker event: {}", e);
            }
        }
        for cap in re.captures_iter(&msg_body) {
            let room_alias = cap[0].to_string();

            let client = client.clone();

            let room = room.clone();
            tokio::spawn(VoyagerBot::search_new_room(client, room_alias, room));
        }
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
    crate::MATRIX_CLIENT.set(client.clone());

    tokio::spawn(async {
        if let Some(client) = crate::MATRIX_CLIENT.get() {
            let joined_rooms = client.joined_rooms().len();
            //TODO make sure to filter only banned ones here .iter().filter(|x|{x.})
            let banned_rooms = client.left_rooms().len();
            let total = joined_rooms + banned_rooms;
            info!("Total joined rooms: {}", total);
            crate::ROOMS_TOTAL_COUNTER.set(total as f64);
            assert_eq!(crate::ROOMS_TOTAL_COUNTER.get() as i64, total as i64);

            //TODO allow configuration
            let room = crate::MATRIX_CLIENT.get().unwrap().get_joined_room(
                &RoomId::try_from("!zeFBFCASPaEUIHzbqj:nordgedanken.dev").unwrap(),
            );
            if let Some(room) = room {
                info!("Updating counter in public room");
                let mut data = BTreeMap::new();
                data.insert("link".to_string(), serde_json::json!(""));
                data.insert("severity".to_string(), serde_json::json!("normal"));
                data.insert("title".to_string(), serde_json::json!("Rooms found"));
                data.insert("value".to_string(), serde_json::json!(total));
                let state_event = AnyStateEventContent::Custom(CustomEventContent {
                    event_type: "re.jki.counter".into(),
                    data,
                });
                if let Err(e) = room.send_state_event(state_event, "rooms_found").await {
                    error!("Failed to update room counter: {}", e);
                }
            }
        }
    });

    client
        .set_event_handler(Box::new(VoyagerBot::new(client.clone())))
        .await;

    info!("start sync");

    let mut filter = FilterDefinition::default();
    let mut room_filter = RoomFilter::default();
    let mut event_filter = RoomEventFilter::default();

    event_filter.lazy_load_options = LazyLoadOptions::Enabled {
        include_redundant_members: false,
    };
    room_filter.state = event_filter;
    filter.room = room_filter;
    let filter_id = client.get_or_upload_filter("sync", filter).await.unwrap();

    let sync_settings = SyncSettings::new().filter(Filter::FilterId(&filter_id));

    client.sync(sync_settings).await;
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
