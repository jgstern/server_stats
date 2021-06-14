use crate::{config::Config, database::cache::CacheDb, MESSAGES_SEMPAHORE};
use chrono::{prelude::*, Duration as ChronoDuration};
use matrix_sdk::{
    api::r0::config::get_global_account_data::Request as GlobalAccountDataGetRequest,
    api::r0::config::set_global_account_data::Request as GlobalAccountDataSetRequest,
    api::r0::{
        filter::RoomEventFilter,
        message::get_message_events::{Direction, Request as MessagesRequest},
    },
    assign, async_trait,
    events::{
        direct::DirectEventContent,
        room::{
            member::{MemberEventContent, MembershipState},
            message::{MessageEventContent, MessageType, TextMessageEventContent},
        },
        AnyMessageEvent, AnyMessageEventContent, AnyRoomEvent, AnyStateEventContent, EventType,
        RawExt, SyncMessageEvent, SyncStateEvent,
    },
    identifiers::{EventId, RoomId, RoomIdOrAliasId, ServerName, UserId},
    room::{Joined, Room},
    uint, Client, ClientConfig, EventHandler, Raw, RequestConfig, RoomType,
};
use matrix_sdk_appservice::{Appservice, AppserviceRegistration};
use once_cell::sync::Lazy;
use regex::Regex;
use std::{collections::BTreeMap, convert::TryFrom, time::Duration};
use tokio::time::sleep;
use tracing::{error, info, span, warn, Instrument, Level};

pub async fn generate_appservice(config: &Config, cache: CacheDb) -> Appservice {
    let homeserver_url = config.bot.clone().homeserver_url;
    let server_name = config.bot.clone().server_name;
    let registration = AppserviceRegistration::try_from_yaml_file("./registration.yaml").unwrap();

    let mut appservice = Appservice::new_with_config(
        homeserver_url.as_str(),
        server_name.as_str(),
        registration.clone(),
        ClientConfig::default()
            .store_path("./store_new/")
            .request_config(
                RequestConfig::default()
                    .assert_identity()
                    .disable_retry()
                    .retry_timeout(Duration::from_secs(30))
                    .timeout(Duration::from_secs(30)),
            ),
    )
    .await
    .unwrap();

    /*use matrix_sdk::{
        api::r0::{
            filter::{FilterDefinition, LazyLoadOptions, RoomFilter},
            sync::sync_events::Filter,
        },
        LoopCtrl, SyncSettings,
    };
    let client = appservice.get_cached_client(None).unwrap();
    tokio::spawn(async move {
        let mut filter = FilterDefinition::default();
        let mut room_filter = RoomFilter::default();
        let mut event_filter = RoomEventFilter::default();
        let mut timeline_event_filter = RoomEventFilter::default();

        event_filter.lazy_load_options = LazyLoadOptions::Enabled {
            include_redundant_members: false,
        };
        timeline_event_filter.lazy_load_options = LazyLoadOptions::Enabled {
            include_redundant_members: false,
        };
        room_filter.state = event_filter;
        room_filter.timeline = timeline_event_filter;
        filter.room = room_filter;
        let filter_id = client
            .get_or_upload_filter("state_reload2", filter)
            .await
            .unwrap();

        let sync_settings = SyncSettings::new()
            .filter(Filter::FilterId(&filter_id))
            .full_state(true)
            //.token(clone_registration.as_token.clone())
            .timeout(Duration::from_secs(5 * 60));
        client
            .sync_with_callback(sync_settings, |response| async move {
                info!("Got sync");

                LoopCtrl::Break
            })
            .await;
        info!("Finished Sync");
    });*/

    let client = appservice.get_cached_client(None).unwrap();
    if crate::MATRIX_CLIENT.set(client).is_err() {
        error!("Failed to globally set matrix client");
    };

    let event_handler = VoyagerBot::new(appservice.clone(), cache, config.clone());

    if let Err(e) = appservice.set_event_handler(Box::new(event_handler)).await {
        error!("Failed to set event handler: {}", e);
    };

    appservice
}

static MESSAGES_FILTER_EVENTS: Lazy<Vec<String>> = Lazy::new(|| vec!["m.room.message".into()]);
static MESSAGES_FILTER: Lazy<RoomEventFilter> = Lazy::new(|| {
    // Make filter for what we care about
    assign!(RoomEventFilter::empty(), {
        types: Some(&MESSAGES_FILTER_EVENTS),
    })
});

static REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[#!](?:[\p{Emoji}--\p{Ascii}]|[a-zA-Z0-9.\-_#=])+:[a-zA-Z0-9.\-_]+[a-zA-Z0-9]")
        .unwrap()
});

#[derive(Debug)]
struct VoyagerBot {
    appservice: Appservice,
    cache: CacheDb,
    config: Config,
}

impl VoyagerBot {
    #[tracing::instrument(name = "VoyagerBot::new", skip(config, cache, appservice))]
    pub fn new(appservice: Appservice, cache: CacheDb, config: Config) -> Self {
        Self {
            appservice,
            cache,
            config,
        }
    }

    #[tracing::instrument(skip(client, room))]
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
                                if contents.contains_key(sender) {
                                    let mut cloned_contents = contents.clone();
                                    let raw_content = cloned_contents.get_mut(sender);
                                    if let Some(content) = raw_content {
                                        content.push(room.room_id().clone());
                                        contents.insert(sender.clone(), content.clone());
                                    };
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
                                };
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
                        };
                    }
                }
            }
        };
    }

    #[tracing::instrument(skip(config, cache, client, room, msg_body))]
    #[async_recursion::async_recursion]
    async fn process_message(
        config: Config,
        cache: CacheDb,
        client: Client,
        msg_body: &str,
        room: Joined,
        event_id: Option<EventId>,
    ) {
        // Early exist if there are no regex matches
        if !REGEX.is_match(msg_body) {
            return;
        }

        // Iterate over aliases
        for cap in REGEX.captures_iter(msg_body) {
            let room_alias = cap[0].to_string();

            let client = client.clone();
            let room = room.clone();
            let span = span!(
                Level::INFO,
                "Starting to process new room",
                room_alias = &cap[0],
                parent_room_id = room.room_id().as_str()
            );
            tokio::spawn(
                VoyagerBot::search_new_room(
                    config.clone(),
                    cache.clone(),
                    client,
                    room_alias,
                    room,
                )
                .instrument(span),
            );
        }

        // If there is a match mark the event as read to indicate it worked
        if let Some(event_id) = event_id {
            if let Err(e) = room.read_marker(&event_id, Some(&event_id)).await {
                error!("Can't send read marker event: {}", e);
            };
        };
    }

    #[tracing::instrument(skip(config, cache, client, parent_room))]
    async fn search_new_room(
        config: Config,
        cache: CacheDb,
        client: Client,
        room_alias: String,
        parent_room: Joined,
    ) {
        // Workaround for: https://github.com/matrix-org/synapse/issues/10021
        if room_alias == "#emacs:matrix.org"
            || room_alias == "!TEwfEWdDwdaFazXmwD:matrix.org"
            || room_alias == "#nextcloud_:matrix.org"
            || room_alias == "#Nextcloud:matrix.org"
            || room_alias == "#NEXTCLOUD:matrix.org"
            || room_alias == "#NextCloud:matrix.org"
            || room_alias == "!UGYpXmlyESJlkXkarj:matrix.org"
        {
            return;
        }

        // Try to join and give it max 5 tries to do so.
        let mut tries: u8 = 5;
        let mut room = None;
        warn!("Trying to join {}", room_alias);
        while tries > 0 && room.is_none() {
            if tries == 0 {
                warn!("No retries left for {}", room_alias);
                break;
            }
            room = VoyagerBot::try_join(client.clone(), &room_alias).await;
            warn!("{} retries left for {}", tries, room_alias);
            tries -= 1;
        }

        // Do not continue if room not found
        if room.is_none() {
            warn!("Didnt get room for {}", room_alias);
            return;
        } else {
            info!("Got room for {}", room_alias);
        }

        // Access room_id once

        let room = room.unwrap().clone();
        let clone_room = room.clone();
        let room_id = clone_room.room_id();

        // Save room to db
        if VoyagerBot::save_to_db(&cache, room_alias.clone(), room_id.as_str(), parent_room).await {
            return;
        }
        VoyagerBot::fetch_messages(room_id, room, client, config, cache).await;
    }

    #[tracing::instrument(skip(room, client, config, cache))]
    async fn fetch_messages(
        room_id: &RoomId,
        room: Joined,
        client: Client,
        config: Config,
        cache: CacheDb,
    ) {
        let mut resp = None;
        if let Ok(_guard) = MESSAGES_SEMPAHORE.acquire().await {
            // Prepare messages request
            let mut request = MessagesRequest::new(room_id, "", Direction::Backward);
            request.limit = uint!(60);
            request.filter = Some(MESSAGES_FILTER.clone());
            resp = Some(room.messages(request).await);
        } else {
            error!("Semaphore closed");
        };
        match resp {
            Some(Ok(resp)) => {
                // Iterate as long as tokens arent the same
                let mut chunk = resp.chunk;
                let mut failed = false;

                let mut from = "".to_string();
                let mut end: String = resp.end.clone().unwrap();
                while !chunk.is_empty() && !failed && from != end {
                    let tickets = MESSAGES_SEMPAHORE.available_permits();
                    info!("Available permits: {}", tickets);
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
                                    let cache = cache.clone();
                                    let config = config.clone();
                                    let span =
                                        span!(Level::INFO, "Starting to process new message",);
                                    tokio::spawn(
                                        async move {
                                            let span = span!(
                                                Level::INFO,
                                                "Starting to process new message inner",
                                            );
                                            VoyagerBot::process_message(
                                                config,
                                                cache,
                                                client,
                                                &text_content.body,
                                                parent_room,
                                                None,
                                            )
                                            .instrument(span)
                                            .await;
                                        }
                                        .instrument(span),
                                    );
                                }
                            };
                        };
                    }

                    // Do next page

                    // TODO use if your synapse is bad again. We should use a queue
                    sleep(Duration::from_secs(2)).await;

                    // Try getting older messages 5 times
                    let old_end = end.clone();

                    let tries: i32 = 0;
                    while tries <= 5 && end == old_end {
                        if let Ok(_guard) = MESSAGES_SEMPAHORE.acquire().await {
                            let mut request =
                                MessagesRequest::new(room_id, &end, Direction::Backward);
                            request.limit = uint!(60);
                            request.filter = Some(MESSAGES_FILTER.clone());
                            let previous = room.messages(request).await;
                            if let Ok(previous) = previous {
                                // Set new chunk to make sure we iterate the correct data in the next round
                                chunk = previous.chunk;
                                from = end;
                                end = previous.end.clone().unwrap();
                            };
                        } else {
                            error!("Semaphore closed");
                        };
                    }
                    if end == old_end {
                        failed = true;
                    }
                }
                if let Err(e) = VoyagerBot::cleanup(room_id.to_string(), &config).await {
                    error!("failed to clean: {}", e);
                };
            }
            Some(Err(e)) => {
                // TODO remove room if `Http(FromHttpResponse(Http(Known(Error { kind: Forbidden, message: "Host not in room.", status_code: 403 }))))` is returned
                error!("Failed to get older events: {}", e);
            }
            _ => (),
        }
    }

    #[tracing::instrument(skip(client))]
    pub async fn join_via_server(client: Client, room_alias: &str) -> Option<Joined> {
        warn!("Trying to join {} via synapse", room_alias);
        // Join the room via the server
        match RoomIdOrAliasId::try_from(room_alias) {
            Ok(room_id_or_alias) => {
                let matrix_org = <&ServerName>::try_from("matrix.org").unwrap();
                match client
                    .join_room_by_id_or_alias(
                        &room_id_or_alias,
                        &[
                            room_id_or_alias.server_name().to_owned(),
                            matrix_org.to_owned(),
                        ],
                    )
                    .await
                {
                    Ok(resp) => {
                        let room = client.get_joined_room(&resp.room_id);
                        if let Some(room) = room {
                            return Some(room);
                        } else {
                            warn!("Room {} was not in the get_joined_room() response. Going to create a fake room for now...",room_alias);
                            // We need to fake a room for now
                            // TODO see how to correctly do this
                            let mut base_room = client
                                .store()
                                .get_or_create_room(&resp.room_id, RoomType::Joined)
                                .await;

                            if let RoomType::Invited = base_room.room_type() {
                                info!(
                                    "Fallback room created with type {:?} instead of Joined. Correcting...",
                                    base_room.room_type()
                                );
                                base_room.mark_as_joined();
                            } else if let RoomType::Left = base_room.room_type() {
                                info!(
                                    "Fallback room created with type {:?} instead of Joined. Correcting...",
                                    base_room.room_type()
                                );
                                base_room.mark_as_joined();
                            }
                            // Get base info

                            // Spaces
                            let room_create_request =
                                matrix_sdk::api::r0::state::get_state_events_for_key::Request::new(
                                    &resp.room_id,
                                    EventType::RoomCreate,
                                    "",
                                );

                            if let Ok(room_create_response) =
                                client.send(room_create_request, None).await
                            {
                                let deserialized = room_create_response
                                    .content
                                    .deserialize_content("m.room.encryption") // deserialize to the inner type
                                    .unwrap();
                                if let AnyStateEventContent::RoomCreate(create) = deserialized {
                                    base_room.set_matrix_room_type(create.room_type);
                                }
                            }

                            // Encryption
                            let room_encryption_request =
                                matrix_sdk::api::r0::state::get_state_events_for_key::Request::new(
                                    &resp.room_id,
                                    EventType::RoomEncryption,
                                    "",
                                );

                            if let Ok(room_encryption_response) =
                                client.send(room_encryption_request, None).await
                            {
                                let deserialized = room_encryption_response
                                    .content
                                    .deserialize_content("m.room.encryption") // deserialize to the inner type
                                    .unwrap();
                                base_room.handle_state_event(&deserialized);
                            }

                            // Avatar
                            let room_avatar_request =
                                matrix_sdk::api::r0::state::get_state_events_for_key::Request::new(
                                    &resp.room_id,
                                    EventType::RoomAvatar,
                                    "",
                                );

                            if let Ok(room_avatar_response) =
                                client.send(room_avatar_request, None).await
                            {
                                let deserialized = room_avatar_response
                                    .content
                                    .deserialize_content("m.room.avatar") // deserialize to the inner type
                                    .unwrap();
                                base_room.handle_state_event(&deserialized);
                            }

                            // name
                            let room_name_request =
                                matrix_sdk::api::r0::state::get_state_events_for_key::Request::new(
                                    &resp.room_id,
                                    EventType::RoomName,
                                    "",
                                );

                            if let Ok(room_name_response) =
                                client.send(room_name_request, None).await
                            {
                                let deserialized = room_name_response
                                    .content
                                    .deserialize_content("m.room.name") // deserialize to the inner type
                                    .unwrap();
                                base_room.handle_state_event(&deserialized);
                            }

                            // create
                            let room_create_request =
                                matrix_sdk::api::r0::state::get_state_events_for_key::Request::new(
                                    &resp.room_id,
                                    EventType::RoomCreate,
                                    "",
                                );

                            if let Ok(room_create_response) =
                                client.send(room_create_request, None).await
                            {
                                let deserialized = room_create_response
                                    .content
                                    .deserialize_content("m.room.create") // deserialize to the inner type
                                    .unwrap();
                                base_room.handle_state_event(&deserialized);
                            }

                            // history
                            let room_history_request =
                                matrix_sdk::api::r0::state::get_state_events_for_key::Request::new(
                                    &resp.room_id,
                                    EventType::RoomHistoryVisibility,
                                    "",
                                );

                            if let Ok(room_history_response) =
                                client.send(room_history_request, None).await
                            {
                                let deserialized = room_history_response
                                    .content
                                    .deserialize_content("m.room.history_visibility") // deserialize to the inner type
                                    .unwrap();
                                base_room.handle_state_event(&deserialized);
                            }

                            // guest_access
                            let room_guest_access_request =
                                matrix_sdk::api::r0::state::get_state_events_for_key::Request::new(
                                    &resp.room_id,
                                    EventType::RoomGuestAccess,
                                    "",
                                );

                            if let Ok(room_guest_access_response) =
                                client.send(room_guest_access_request, None).await
                            {
                                let deserialized = room_guest_access_response
                                    .content
                                    .deserialize_content("m.room.guest_access") // deserialize to the inner type
                                    .unwrap();
                                base_room.handle_state_event(&deserialized);
                            }

                            // RoomJoinRules
                            let room_join_rules_request =
                                matrix_sdk::api::r0::state::get_state_events_for_key::Request::new(
                                    &resp.room_id,
                                    EventType::RoomJoinRules,
                                    "",
                                );

                            if let Ok(room_join_rules_response) =
                                client.send(room_join_rules_request, None).await
                            {
                                let deserialized = room_join_rules_response
                                    .content
                                    .deserialize_content("m.room.join_rules") // deserialize to the inner type
                                    .unwrap();
                                base_room.handle_state_event(&deserialized);
                            }

                            // Canonical Alias
                            let room_canonical_alias_request =
                                matrix_sdk::api::r0::state::get_state_events_for_key::Request::new(
                                    &resp.room_id,
                                    EventType::RoomCanonicalAlias,
                                    "",
                                );

                            if let Ok(room_canonical_alias_response) =
                                client.send(room_canonical_alias_request, None).await
                            {
                                let deserialized = room_canonical_alias_response
                                    .content
                                    .deserialize_content("m.room.canonical_alias") // deserialize to the inner type
                                    .unwrap();
                                base_room.handle_state_event(&deserialized);
                            }

                            // Topic
                            let room_topic_request =
                                matrix_sdk::api::r0::state::get_state_events_for_key::Request::new(
                                    &resp.room_id,
                                    EventType::RoomTopic,
                                    "",
                                );

                            if let Ok(room_topic_response) =
                                client.send(room_topic_request, None).await
                            {
                                let deserialized = room_topic_response
                                    .content
                                    .deserialize_content("m.room.topic") // deserialize to the inner type
                                    .unwrap();
                                base_room.handle_state_event(&deserialized);
                            }

                            // Tombstone
                            let room_tombstone_request =
                                matrix_sdk::api::r0::state::get_state_events_for_key::Request::new(
                                    &resp.room_id,
                                    EventType::RoomTombstone,
                                    "",
                                );

                            if let Ok(room_tombstone_response) =
                                client.send(room_tombstone_request, None).await
                            {
                                let deserialized = room_tombstone_response
                                    .content
                                    .deserialize_content("m.room.tombstone") // deserialize to the inner type
                                    .unwrap();
                                base_room.handle_state_event(&deserialized);
                            }

                            // RoomPowerLevels
                            let room_power_levels_request =
                                matrix_sdk::api::r0::state::get_state_events_for_key::Request::new(
                                    &resp.room_id,
                                    EventType::RoomPowerLevels,
                                    "",
                                );

                            if let Ok(room_power_levels_response) =
                                client.send(room_power_levels_request, None).await
                            {
                                let deserialized = room_power_levels_response
                                    .content
                                    .deserialize_content("m.room.power_levels") // deserialize to the inner type
                                    .unwrap();
                                base_room.handle_state_event(&deserialized);
                            }

                            return Joined::new(client.clone(), base_room);
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

    #[tracing::instrument(skip(client))]
    pub async fn try_join(client: Client, room_alias: &str) -> Option<Joined> {
        // Check if we already knew the room and exit early if it is the case
        let already_joined_room_id = client
            .joined_rooms()
            .iter()
            .find(|room| {
                if let Some(alias) = room.canonical_alias() {
                    return alias == room_alias;
                };
                *room.room_id() == room_alias
            })
            .cloned();
        if already_joined_room_id.is_some() {
            return already_joined_room_id;
        }
        VoyagerBot::join_via_server(client, room_alias).await
    }

    #[tracing::instrument(skip(cache, parent_room))]
    /// Returns true if we want to exit early
    async fn save_to_db(
        cache: &CacheDb,
        room_alias: String,
        room_id: &str,
        parent_room: Joined,
    ) -> bool {
        // Get parent Data
        let parent_id = parent_room.room_id().as_str();
        let parent_displayname = parent_room.display_name().await;

        if parent_id.is_empty() {
            return true;
        }
        // Check if we know the child already
        if cache.graph.knows_room(room_id) {
            // Check if the parent was known for this child already
            let parents = cache.graph.get_parent(room_id);
            if !parents.iter().any(|x| x.as_ref() == parent_id) {
                // If it is not already known as a parent
                info!(
                    "New room relation for already known room: {:?} -> {}",
                    parent_displayname, room_alias
                );
                if let Err(e) = cache.graph.add_child(parent_id, room_id).await {
                    error!("failed to save child: {}", e);
                };
            }
            return true;
        } else {
            // Save it as it is a new relation
            if let Err(e) = cache.graph.add_child(parent_id, room_id).await {
                error!("failed to save child: {}", e);
            };

            info!(
                "New room relation: {:?} -> {}",
                parent_displayname, room_alias
            );
        }
        false
    }

    #[tracing::instrument(skip(config))]
    /// Calls the purge_history API at synapse to cleanup rooms
    async fn cleanup(room_id: String, config: &Config) -> color_eyre::Result<()> {
        let now = Utc::now();
        let time = now - ChronoDuration::days(2);
        let timestamp = time.timestamp_millis();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        let server_address = &config.bot.homeserver_url;
        let map = serde_json::json!({"delete_local_events": false, "purge_up_to_ts":timestamp});
        let auth_header = format!("Bearer {}", config.bot.admin_access_token);
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

    async fn handle_commands(msg_body: String, room: Joined) {
        if (msg_body.contains("!help") && room.is_direct())
            || (msg_body.contains("Server Stats Discoverer (traveler bot):")
                && msg_body.contains("!help"))
        {
            info!("Sending help");
            room.typing_notice(true)
                .await
                .expect("Can't send typing event");
            let content = AnyMessageEventContent::RoomMessage(MessageEventContent::notice_html(
                r#"Hi! I am the server_stats Discovery bot by @mtrnord:nordgedanken.dev ! \n\n\n
                    What am I doing?\n\n I am a bot discovering matrix rooms. I am just looking for tasty room aliases :) I do not save your content!\n\n
                    How do I get removed?\n\n Its simple! Just ban me and I will not come back :)\n\n
                    Where can I learn more?\n\n You can either look at my source over at https://github.com/MTRNord/server_stats or join #server_stats:nordgedanken.dev :)"#,
                r#"<h1>Hi! I am the server_stats Discovery bot by <a href=\"https://matrix.to/#/@mtrnord:nordgedanken.dev\">MTRNord</a></h1>
                        <h2>What am I doing?</h2> I am a bot discovering matrix rooms. I am just looking for tasty room aliases :) I do not read the actual content or save it!
                        <h2>How do I get removed?</h2> Its simple! Just ban me and I will not come back :)
                        <h2>Where can I learn more?</h2> You can either look at my source over at https://github.com/MTRNord/server_stats or join <a href=\"https://matrix.to/#/#server_stats:nordgedanken.dev\">#server_stats:nordgedanken.dev</a> :)"#,
            ));
            room.send(content, None).await.unwrap();

            room.typing_notice(false)
                .await
                .expect("Can't send typing event");
        }
    }
}

#[async_trait]
impl EventHandler for VoyagerBot {
    #[tracing::instrument(skip(self, room, event))]
    async fn on_room_member(&self, room: Room, event: &SyncStateEvent<MemberEventContent>) {
        if let MembershipState::Invite = event.content.membership {
            if !&event.state_key.contains("@server_stats:nordgedanken.dev") {
                return;
            }
            let client = self.appservice.get_cached_client(None).unwrap();
            let joined_room =
                VoyagerBot::join_via_server(client.clone(), room.room_id().as_str()).await;
            VoyagerBot::set_direct(client, room.clone(), event).await;
            info!("Successfully joined room {}", room.room_id());

            if let Some(room) = joined_room {
                if room.is_encrypted() {
                    info!("Sending mention that the bot cant do e2ee");
                    room.typing_notice(true)
                        .await
                        .expect("Can't send typing event");
                    let content = AnyMessageEventContent::RoomMessage(
                        MessageEventContent::notice_plain(
                            r#"Hi! I am the server_stats Discovery bot by @mtrnord:nordgedanken.dev ! \n\n\n
                   I am currently not able to read encrypted Rooms. Any command you try to send me will not work. Instead mention me in a room with !help."#,
                        ),
                    );
                    room.send(content, None).await.unwrap();

                    room.typing_notice(false)
                        .await
                        .expect("Can't send typing event");
                }
            };
        };
    }

    #[tracing::instrument(skip(event, room, self))]
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

            VoyagerBot::handle_commands(msg_body.clone(), room.clone()).await;

            // Handle message
            let client = self.appservice.get_cached_client(None).unwrap();
            let cache = self.cache.clone();
            let config = self.config.clone();

            let span = span!(
                Level::INFO,
                "Starting to process new message",
                event_id = event_id.as_str()
            );
            tokio::spawn(
                async move {
                    VoyagerBot::process_message(
                        config,
                        cache,
                        client,
                        &msg_body,
                        room,
                        Some(event_id),
                    )
                    .await;
                }
                .instrument(span),
            );
        };
    }
}
