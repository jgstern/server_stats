use crate::config::Config;
use chrono::{prelude::*, Duration as ChronoDuration};
use matrix_sdk::{
    api::r0::config::get_global_account_data::Request as GlobalAccountDataGetRequest,
    api::r0::config::set_global_account_data::Request as GlobalAccountDataSetRequest,
    api::r0::{
        filter::RoomEventFilter,
        message::get_message_events::{Direction, Request as MessagesRequest},
    },
    async_trait,
    events::{
        direct::DirectEventContent,
        room::{
            member::{MemberEventContent, MembershipState},
            message::{MessageEventContent, MessageType, TextMessageEventContent},
        },
        AnyMessageEvent, AnyMessageEventContent, AnyRoomEvent, SyncMessageEvent, SyncStateEvent,
    },
    identifiers::{EventId, RoomIdOrAliasId, UserId},
    room::{Joined, Room},
    uint, Client, EventHandler, Raw,
};
use matrix_sdk_appservice::{Appservice, AppserviceRegistration};
use once_cell::sync::Lazy;
use regex::Regex;
use std::{collections::BTreeMap, convert::TryFrom, time::Duration};
use tokio::time::sleep;
use tracing::{error, info};

pub async fn generate_appservice(config: &Config<'_>) -> Appservice {
    let homeserver_url = &config.bot.homeserver_url;
    let server_name = &config.bot.server_name;
    let registration = AppserviceRegistration::try_from_yaml_file("./registration.yaml").unwrap();

    let appservice = Appservice::new(homeserver_url.as_ref(), server_name.as_ref(), registration)
        .await
        .unwrap();

    let event_handler = VoyagerBot::new(appservice.clone());

    appservice
        .client()
        .set_event_handler(Box::new(event_handler))
        .await;

    let client = appservice
        .client_with_localpart("server_stats")
        .await
        .unwrap();
    crate::MATRIX_CLIENT.set(client);

    appservice
}

/*static MESSAGES_FILTER: Lazy<RoomEventFilter> = Lazy::new(|| {
    // Make filter for what we care about
    let mut filter = RoomEventFilter::empty();
    let array = vec!["m.room.message".to_string()];
    filter.types = Some(array.as_slice());
    filter
});*/

static REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[#!][a-zA-Z0-9.\-_#=]+:[a-zA-Z0-9.\-_]+[a-zA-Z0-9]").unwrap());
struct VoyagerBot {
    appservice: Appservice,
}

impl VoyagerBot {
    pub fn new(appservice: Appservice) -> Self {
        Self { appservice }
    }

    async fn set_direct(
        client: Client,
        room: Room,
        room_member: &SyncStateEvent<MemberEventContent>,
    ) {
        if let Some(is_direct) = room_member.content.is_direct {
            if is_direct {
                // TODO make this use the correct user_id
                let user_id = UserId::try_from("@server_stats:nordgedanken.dev").unwrap();
                let get_req = GlobalAccountDataGetRequest::new(&user_id, "m.direct");

                let sender = &room_member.sender;
                match client.send(get_req, None).await {
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
                                if let Err(e) = client.send(set_request, None).await {
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
                        if let Err(e) = client.send(set_request, None).await {
                            error!("Failed to set m.direct: {}", e);
                        }
                    }
                }
            }
        }
    }

    #[async_recursion::async_recursion]
    async fn process_message(
        client: Client,
        msg_body: &str,
        room: Joined,
        event_id: Option<EventId>,
    ) {
        // Early exist if there are no regex matches
        if !REGEX.is_match(&msg_body) {
            return;
        }

        // If there is a match mark the event as read to indicate it worked
        if let Some(event_id) = event_id {
            if let Err(e) = room.read_marker(&event_id, Some(&event_id)).await {
                error!("Can't send read marker event: {}", e);
            }
        }

        // Iterate over aliases
        for cap in REGEX.captures_iter(&msg_body) {
            let room_alias = cap[0].to_string();

            let client = client.clone();
            let room = room.clone();
            tokio::spawn(VoyagerBot::search_new_room(client, room_alias, room));
        }
    }

    async fn search_new_room(client: Client, room_alias: String, parent_room: Joined) {
        // Try to join and give it max 5 tries to do so.
        let mut tries: i32 = 0;
        let mut room = None;
        while tries <= 5 && room.is_none() {
            room = VoyagerBot::try_join(client.clone(), &room_alias, tries).await;
            tries += 1;
        }

        // Do not continue if room not found
        if room.is_none() {
            return;
        }

        // Access room_id once
        let room = room.unwrap().clone();
        let room_id = room.room_id();

        // Save room to db
        VoyagerBot::save_to_db(room_alias, room_id.as_str(), parent_room).await;

        // Make filter for what we care about
        let mut filter = RoomEventFilter::empty();
        let array = vec!["m.room.message".to_string()];
        filter.types = Some(array.as_slice());

        // Prepare messages request
        let mut request = MessagesRequest::new(&room_id, "", Direction::Backward);
        request.limit = uint!(60);
        request.filter = Some(filter.clone());

        // Run request
        match room.messages(request).await {
            Ok(resp) => {
                // Iterate as long as tokens arent the same
                let mut chunk = resp.chunk;
                let mut failed = false;

                let mut from = "".to_string();
                let mut end: String = resp.end.clone().unwrap();
                while !failed && from != end {
                    // For each message we recursivly do this again
                    for message in &chunk {
                        let deserialized_message = message.deserialize();
                        if let Ok(AnyRoomEvent::Message(AnyMessageEvent::RoomMessage(message))) =
                            deserialized_message
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

                    // Do next page

                    // TODO use if your synapse is bad again. We should use a queue
                    sleep(Duration::from_secs(2)).await;

                    // Try getting older messages 5 times
                    let old_end = end.clone();

                    let tries: i32 = 0;
                    while tries <= 5 && end == old_end {
                        let mut request = MessagesRequest::new(&room_id, &end, Direction::Backward);
                        request.limit = uint!(60);
                        request.filter = Some(filter.clone());
                        let previous = room.messages(request).await;
                        if let Ok(previous) = previous {
                            // Set new chunk to make sure we iterate the correct data in the next round
                            chunk = previous.chunk;
                            from = end;
                            end = previous.end.clone().unwrap();
                        }
                    }
                    if end == old_end {
                        failed = true;
                    }
                }
                if let Err(e) = VoyagerBot::cleanup(room_id.to_string()).await {
                    error!("failed to clean: {}", e);
                }
            }
            Err(e) => {
                // TODO remove room if `Http(FromHttpResponse(Http(Known(Error { kind: Forbidden, message: "Host not in room.", status_code: 403 }))))` is returned
                error!("Failed to get older events: {}", e);
            }
        }
    }

    pub async fn try_join(client: Client, room_alias: &str, tries: i32) -> Option<Joined> {
        if tries > 0 {
            // Check if we already knew the room and exit early if it is the case
            let already_joined_room_id = client
                .joined_rooms()
                .iter()
                .find(|room| {
                    if let Some(alias) = room.canonical_alias() {
                        return alias == room_alias;
                    }
                    *room.room_id() == room_alias
                })
                .cloned();
            if already_joined_room_id.is_some() {
                return already_joined_room_id;
            }
        }
        // Join the room via the server
        match RoomIdOrAliasId::try_from(room_alias) {
            Ok(room_id_or_alias) => {
                match client
                    .join_room_by_id_or_alias(&room_id_or_alias, &[])
                    .await
                {
                    Ok(resp) => {
                        // Wait for sync to roughly complete
                        sleep(Duration::from_secs(5)).await;
                        let room = client.get_joined_room(&resp.room_id);
                        if let Some(room) = room {
                            return Some(room);
                        }
                    }
                    Err(e) => {
                        error!("Failed to join room ({}): {}", room_alias, e);
                    }
                };
            }
            Err(e) => error!("Found invalid alias ({}): {}", room_alias, e),
        }

        None
    }

    async fn save_to_db(room_alias: String, room_id: &str, parent_room: Joined) {
        // Get parent Data
        let parent_id = parent_room.room_id().as_str();
        let parent_displayname = parent_room.display_name().await;

        // Check if we know the child already
        if crate::CACHE_DB.graph.knows_room(room_id) {
            // Check if the parent was known for this child already
            let parents = crate::CACHE_DB.graph.get_parent(room_id);
            if parents.iter().any(|x| x.as_ref() == parent_id) {
                // If it is not already known as a parent
                info!(
                    "New room relation for already known room: {:?} -> {}",
                    parent_displayname, room_alias
                );
                if let Err(e) = crate::CACHE_DB.graph.add_child(parent_id, room_id) {
                    error!("failed to save child: {}", e);
                }
            } else {
                error!("We knew {} but didnt find a parent", room_alias);
                if let Err(e) = crate::CACHE_DB.graph.add_child(parent_id, room_id) {
                    error!("failed to save child: {}", e);
                }

                info!(
                    "New room relation: {:?} -> {}",
                    parent_displayname, room_alias
                );
            }
            return;
        } else {
            // Save it as it is a new relation
            if let Err(e) = crate::CACHE_DB.graph.add_child(parent_id, room_id) {
                error!("failed to save child: {}", e);
            }

            info!(
                "New room relation: {:?} -> {}",
                parent_displayname, room_alias
            );
        }
    }

    /// Calls the purge_history API at synapse to cleanup rooms
    async fn cleanup(room_id: String) -> color_eyre::Result<()> {
        let now = Utc::now();
        let time = now - ChronoDuration::days(2);
        let timestamp = time.timestamp_millis();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        let server_address = &crate::CONFIG.get().unwrap().bot.homeserver_url;
        let map = serde_json::json!({"delete_local_events": false, "purge_up_to_ts":timestamp});
        let auth_header = format!(
            "Bearer {}",
            crate::CONFIG.get().unwrap().bot.admin_access_token
        );
        let url = format!(
            "{}/_synapse/admin/v1/purge_history/{}",
            server_address, room_id
        );
        let body = client
            .post(url.clone())
            .header("Authorization", auth_header.clone())
            .json(&map)
            .send()
            .await?
            .text()
            .await?;

        info!(
            "Started cleanup for room ({}): {} = {:?}",
            room_id, url, body
        );

        Ok(())
    }
}

#[async_trait]
impl EventHandler for VoyagerBot {
    async fn on_room_member(&self, room: Room, event: &SyncStateEvent<MemberEventContent>) {
        if !self
            .appservice
            .user_id_is_in_namespace(&event.state_key)
            .unwrap()
            || !&event.state_key.contains("server_stats")
        {
            dbg!("not an appservice user");
            return;
        }

        if let MembershipState::Invite = event.content.membership {
            let client = self
                .appservice
                .client_with_localpart("server_stats")
                .await
                .unwrap();

            client.join_room_by_id(room.room_id()).await.unwrap();
            VoyagerBot::set_direct(client, room.clone(), event).await;
            info!("Successfully joined room {}", room.room_id());
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
            let client = self
                .appservice
                .client_with_localpart("server_stats")
                .await
                .unwrap();
            tokio::spawn(async move {
                VoyagerBot::process_message(client, &msg_body, room, Some(event_id)).await;
            });
        }
    }
}
