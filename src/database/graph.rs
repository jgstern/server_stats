use crate::webpage::api::{Link, RelationsJson, RoomRelation, SSEJson, ServersJson};
use color_eyre::Result;
use matrix_sdk::{identifiers::RoomId, room::Joined};
use sled::{IVec, Iter};
use sqlx::PgPool;
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    convert::{TryFrom, TryInto},
    sync::{Arc, RwLock},
};
use tokio::sync::watch::Sender;
use tracing::error;

type RelationsMix = Vec<((String, String), BTreeSet<u128>)>;

#[derive(Debug)]
pub struct GraphDb {
    hash_map: sled::Tree,
    state: sled::Tree,
    parent_child: sled::Tree,
    child_parent: sled::Tree,
    websocket_tx: Sender<Option<SSEJson>>,
    pool: PgPool,
    room_name_cache: RwLock<BTreeMap<String, String>>,
}

#[derive(sqlx::FromRow, Debug, Clone)]
struct MemberCount {
    room_id: String,
    count: i64,
}

#[derive(sqlx::FromRow, Debug, Clone)]
struct Server {
    server_name: String,
}

impl GraphDb {
    #[tracing::instrument(
        name = "GraphDb::new",
        skip(hash_map, state, parent_child, child_parent, tx)
    )]
    pub fn new(
        hash_map: sled::Tree,
        state: sled::Tree,
        parent_child: sled::Tree,
        child_parent: sled::Tree,
        tx: Sender<Option<SSEJson>>,
        pool: PgPool,
    ) -> Self {
        GraphDb {
            hash_map,
            state,
            parent_child,
            child_parent,
            websocket_tx: tx,
            pool,
            room_name_cache: RwLock::new(BTreeMap::new()),
        }
    }

    #[tracing::instrument(skip(self))]
    async fn get_synapse_joined_members_count(&self) -> BTreeMap<String, i64> {
        let res = sqlx::query_as(
            "SELECT room_id, COUNT(*) FROM current_state_events WHERE membership = 'join' AND type = 'm.room.member' GROUP by room_id;"
        )
        .fetch_all(&self.pool).await;
        match res {
            Ok(res) => {
                let rows: Vec<MemberCount> = res;
                let map = rows.iter().map(|x| (x.room_id.clone(), x.count)).collect();
                return map;
            }
            Err(e) => {
                error!("Failed to get member count from db {:?}", e);
            }
        }
        BTreeMap::new()
    }

    #[tracing::instrument(skip(self))]
    async fn get_synapse_joined_members(&self) -> BTreeSet<String> {
        let res = sqlx::query_as(
            "SELECT distinct split_part(state_key, ':', 2) AS server_name FROM current_state_events WHERE membership = 'join' AND type = 'm.room.member';",
        )
        .fetch_all(&self.pool).await;
        match res {
            Ok(res) => {
                let rows: Vec<Server> = res;
                return rows
                    .iter()
                    .map(|x| x.server_name.clone())
                    .collect::<BTreeSet<_>>();
            }
            Err(e) => {
                error!("Failed to get member count from db {:?}", e);
            }
        }
        BTreeSet::new()
    }

    #[tracing::instrument(skip(self))]
    pub fn get_parent(&self, child: &str) -> Vec<Cow<str>> {
        let child_hash = GraphDb::hash(child);
        if let Ok(Some(parent)) = self.child_parent.get(child_hash.to_le_bytes()) {
            let parent_hashes: Vec<u128> = bincode::deserialize(parent.as_ref()).unwrap();
            let parents: Vec<Cow<str>> = parent_hashes
                .iter()
                .map(|hash| {
                    if let Some(parent) = self.get_room_id_from_hash(hash) {
                        let parent = parent.as_ref();
                        let parent_id = std::str::from_utf8(parent).unwrap_or_default().to_string();
                        return Some(parent_id.into());
                    }
                    None
                })
                .flatten()
                .collect();
            return parents;
        }
        vec![]
    }

    #[tracing::instrument(skip(self))]
    pub async fn add_child(&self, parent: &str, child: &str) -> Result<()> {
        let parent_hash = GraphDb::hash(parent);
        let child_hash = GraphDb::hash(child);

        // Make sure we mapped both already
        if let Ok(res) = self.hash_map.contains_key(parent_hash.to_le_bytes()) {
            if !res {
                self.map_hash_to_room_id(parent_hash, parent)?;
            }
        } else {
            self.map_hash_to_room_id(parent_hash, parent)?;
        }

        if let Ok(res) = self.hash_map.contains_key(child_hash.to_le_bytes()) {
            if !res {
                self.map_hash_to_room_id(child_hash, child)?;
            }
        } else {
            self.map_hash_to_room_id(child_hash, child)?;
        }

        // Save relation
        self.parent_child
            .update_and_fetch(parent_hash.to_le_bytes(), |value_opt| {
                if let Some(existing) = value_opt {
                    let mut decoded: Vec<u128> = bincode::deserialize(existing).unwrap();

                    if !decoded.contains(&child_hash) {
                        decoded.push(child_hash);
                    }
                    Some(sled::IVec::from(bincode::serialize(&decoded).unwrap()))
                } else {
                    let data: Vec<u128> = vec![child_hash];

                    Some(sled::IVec::from(bincode::serialize(&data).unwrap()))
                }
            })?;
        self.parent_child.flush()?;
        self.add_parent(parent_hash, child_hash)?;

        if let Some(client) = crate::MATRIX_CLIENT.get() {
            if let Some(room) = client.get_joined_room(&RoomId::try_from(child).unwrap()) {
                let alias = if let Some(alias) = room.canonical_alias() {
                    alias.to_string()
                } else {
                    child.to_string()
                };

                let mut name = { self.room_name_cache.read().unwrap().get(child).cloned() };
                if name.is_none() {
                    name = if let Ok(name) = room.display_name().await {
                        if name.is_empty() {
                            Some(child.to_string())
                        } else {
                            self.room_name_cache
                                .write()
                                .unwrap()
                                .insert(child.to_string(), name.clone());
                            Some(name)
                        }
                    } else {
                        Some(child.to_string())
                    };
                };
                let name = name.unwrap();

                let topic = if let Some(topic) = room.topic() {
                    topic
                } else {
                    "".into()
                };
                let avatar_url = if let Some(avatar_url) = room.avatar_url() {
                    avatar_url.to_string()
                } else {
                    "".into()
                };
                if name == "MTRNord"
                    || base64::encode(parent_hash.to_le_bytes()) == "4u98GV1CGlCn6PvxBerjrw=="
                {
                    return Ok(());
                }
                let members = if let Some(members) =
                    self.get_synapse_joined_members_count().await.get(child)
                {
                    *members
                } else {
                    0
                };
                let sse_json = SSEJson {
                    node: Arc::new(RoomRelation {
                        id: base64::encode(child_hash.to_le_bytes()),
                        name,
                        alias,
                        avatar: avatar_url,
                        topic,
                        weight: Some(1),
                        incoming_links: None,
                        outgoing_links: None,
                        room_id: child.into(),
                        is_space: room.is_space(),
                        members,
                    }),
                    link: Arc::new(Link {
                        source: base64::encode(parent_hash.to_le_bytes()),
                        target: base64::encode(child_hash.to_le_bytes()),
                        value: 1,
                    }),
                };
                if let Err(e) = self.websocket_tx.send(Some(sse_json)) {
                    error!("Failed to broadcast to websockets: {:?}", e);
                }
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    fn add_parent(&self, parent: u128, child: u128) -> Result<()> {
        self.child_parent
            .update_and_fetch(child.to_le_bytes(), |value_opt| {
                if let Some(existing) = value_opt {
                    let mut decoded: Vec<u128> = bincode::deserialize(existing).unwrap();
                    if !decoded.contains(&parent) {
                        decoded.push(parent);
                    }

                    Some(sled::IVec::from(bincode::serialize(&decoded).unwrap()))
                } else {
                    let data: Vec<u128> = vec![parent];

                    Some(sled::IVec::from(bincode::serialize(&data).unwrap()))
                }
            })?;
        self.child_parent.flush()?;
        Ok(())
    }

    #[tracing::instrument]
    fn hash(input: &str) -> u128 {
        xxhash_rust::xxh3::xxh3_128(input.as_bytes())
    }

    #[tracing::instrument(skip(self))]
    fn map_hash_to_room_id(&self, hash: u128, alias: &str) -> Result<()> {
        self.hash_map.insert(hash.to_le_bytes(), alias.as_bytes())?;
        self.hash_map.flush()?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn knows_room(&self, room_alias: &str) -> bool {
        let room_alias_hash = GraphDb::hash(room_alias);
        if let Ok(res) = self.hash_map.contains_key(room_alias_hash.to_le_bytes()) {
            return res;
        }
        false
    }

    #[tracing::instrument(skip(self))]
    pub fn get_all_room_ids(
        &self,
    ) -> impl DoubleEndedIterator<Item = sled::Result<IVec>> + Send + Sync {
        let r = self.hash_map.iter();
        r.values()
    }

    #[tracing::instrument(skip(self))]
    fn get_room_id_from_hash(&self, hash: &u128) -> Option<IVec> {
        if let Ok(room_id) = self.hash_map.get(hash.to_le_bytes()) {
            return room_id;
        }
        None
    }

    #[tracing::instrument(skip(self))]
    fn get_all_parent_child(&self) -> Iter {
        self.parent_child.iter()
    }

    #[tracing::instrument(skip(self))]
    pub async fn get_node(&self, id: String) -> Option<RoomRelation> {
        let joined_members = self.get_synapse_joined_members_count().await;
        if let Ok(hash) = base64::decode(id.clone()) {
            let room_hash_bytes = GraphDb::fix_size(hash.as_ref());
            let room_hash = u128::from_le_bytes(room_hash_bytes);
            let room_id_bytes = self.get_room_id_from_hash(&room_hash);
            if let Some(room_id_bytes) = room_id_bytes {
                let room_id = std::str::from_utf8(room_id_bytes.as_ref()).unwrap_or_default();
                if let Some(client) = crate::MATRIX_CLIENT.get() {
                    let joined_rooms = client.joined_rooms();
                    if let Some(relation) = self
                        .generate_room_relation(id.clone(), room_id, &joined_rooms, &joined_members)
                        .await
                    {
                        if relation.name == "MTRNord" || id == "4u98GV1CGlCn6PvxBerjrw==" {
                            return None;
                        }
                        return Some(relation);
                    }
                }
            }
        }
        None
    }

    #[tracing::instrument]
    fn fix_size(raw: &[u8]) -> [u8; 16] {
        raw.try_into().expect("slice with incorrect length")
    }

    #[tracing::instrument(skip(self))]
    pub async fn get_json_relations(&self) -> RelationsJson {
        let mut nodes = BTreeSet::new();
        let mut all_links = BTreeSet::new();

        let room_id_relations: RelationsMix = self
            .get_all_parent_child()
            .filter_map(|s| s.ok())
            .map(|(key, val)| {
                let parent_hash = GraphDb::fix_size(key.as_ref());
                let child_hashes: BTreeSet<u128> = bincode::deserialize(val.as_ref()).unwrap();
                let parent_hash = u128::from_le_bytes(parent_hash);

                let parent = self.get_room_id_from_hash(&parent_hash);
                let parent_hash = base64::encode(parent_hash.to_le_bytes());
                ((parent_hash, parent), child_hashes)
            })
            .map(|((parent_hash, parent_bytes), child_hashes)| {
                let mut parent = "".to_string();
                if let Some(parent_bytes) = parent_bytes {
                    parent = std::str::from_utf8(parent_bytes.as_ref())
                        .unwrap_or_default()
                        .to_string();
                }

                ((parent_hash, parent), child_hashes)
            })
            .collect();

        let joined_members = self.get_synapse_joined_members_count().await;
        if let Some(client) = crate::MATRIX_CLIENT.get() {
            let joined_rooms = client.joined_rooms();
            for ((parent_hash, parent), child_hashes) in room_id_relations {
                if let Some(relation) = self
                    .generate_room_relation(
                        parent_hash.clone(),
                        &parent,
                        &joined_rooms,
                        &joined_members,
                    )
                    .await
                {
                    let links: BTreeSet<Link> = child_hashes
                        .iter()
                        .map(|child_hash| base64::encode(child_hash.to_le_bytes()))
                        .map(|child| Link {
                            source: parent_hash.clone(),
                            target: child,
                            value: 1,
                        })
                        .collect();

                    if relation.name == "MTRNord" || parent_hash == "4u98GV1CGlCn6PvxBerjrw==" {
                        continue;
                    }
                    all_links.extend(links.into_iter());

                    // TODO Use tokio channel to allow streaming of nodes
                    // Add the parent
                    nodes.insert(relation);
                }
            }

            let missing_child_nodes_links: Vec<&Link> = all_links
                .iter()
                .filter(|link| {
                    !nodes
                        .iter()
                        .any(|relation: &RoomRelation| relation.id == link.target)
                })
                .collect();

            // Add missing childs
            for link in &missing_child_nodes_links {
                if let Ok(hash) = base64::decode(link.target.clone()) {
                    let room_hash_bytes = GraphDb::fix_size(hash.as_ref());
                    let room_hash = u128::from_le_bytes(room_hash_bytes);
                    let room_id_bytes = self.get_room_id_from_hash(&room_hash);
                    if let Some(room_id_bytes) = room_id_bytes {
                        let room_id =
                            std::str::from_utf8(room_id_bytes.as_ref()).unwrap_or_default();
                        if let Some(relation) = self
                            .generate_room_relation(
                                link.target.clone(),
                                room_id,
                                &joined_rooms,
                                &joined_members,
                            )
                            .await
                        {
                            if relation.name == "MTRNord"
                                || link.target == "4u98GV1CGlCn6PvxBerjrw=="
                            {
                                continue;
                            }
                            nodes.insert(relation);
                        }
                    }
                }
            }

            // Remove broken links
            let node_ids: BTreeSet<String> = nodes.iter().map(|node| node.id.clone()).collect();
            all_links.retain(|link| {
                node_ids.contains(&link.target.to_string())
                    && node_ids.contains(&link.source.to_string())
            });

            let nodes: BTreeSet<RoomRelation> = nodes
                .into_iter()
                .map(|mut node| {
                    let links = all_links
                        .iter()
                        .filter(|x| {
                            (x.source == node.id || x.target == node.id) && x.source != x.target
                        })
                        .count();
                    let incoming_links = all_links.iter().filter(|x| x.target == node.id).count();
                    let outgoing_links = all_links.iter().filter(|x| x.source == node.id).count();
                    node.weight = Some(links.try_into().unwrap());
                    node.incoming_links = Some(incoming_links.try_into().unwrap());
                    node.outgoing_links = Some(outgoing_links.try_into().unwrap());
                    node
                })
                .collect();

            RelationsJson {
                nodes,
                links: all_links,
            }
        } else {
            panic!("SDK Client missing");
        }
    }

    #[tracing::instrument(skip(self, joined_rooms, joined_members))]
    async fn generate_room_relation(
        &self,
        room_hash: String,
        room_id: &str,
        joined_rooms: &[Joined],
        joined_members: &BTreeMap<String, i64>,
    ) -> Option<RoomRelation> {
        let room_id_serialized = &RoomId::try_from(room_id).unwrap();

        if let Some(room) = joined_rooms
            .iter()
            .find(|room| room.room_id() == room_id_serialized)
        {
            if room.is_tombstoned() {
                return None;
            }
            let alias = if let Some(alias) = room.canonical_alias() {
                alias.as_str().into()
            } else {
                room_id.into()
            };

            // TODO update on changes somehow
            let mut name = { self.room_name_cache.read().unwrap().get(room_id).cloned() };
            if name.is_none() {
                name = if let Ok(name) = room.display_name().await {
                    if name.is_empty() {
                        Some(room_id.to_string())
                    } else {
                        self.room_name_cache
                            .write()
                            .unwrap()
                            .insert(room_id.to_string(), name.clone());
                        Some(name)
                    }
                } else {
                    Some(room_id.to_string())
                };
            };
            let name = name.unwrap();

            let topic = if let Some(topic) = room.topic() {
                topic
            } else {
                "".into()
            };
            let avatar_url = if let Some(avatar_url) = room.avatar_url() {
                avatar_url.to_string()
            } else {
                "".into()
            };
            let members = if let Some(members) = joined_members.get(room_id) {
                *members
            } else {
                0
            };
            return Some(RoomRelation {
                id: room_hash,
                name,
                alias,
                avatar: avatar_url,
                topic,
                weight: None,
                incoming_links: None,
                outgoing_links: None,
                room_id: room_id.into(),
                is_space: room.is_space(),
                members,
            });
        }

        None
    }

    #[tracing::instrument(skip(self, joined_rooms))]
    pub async fn is_joined(&self, room_id: &str, joined_rooms: &[Joined]) -> bool {
        let room_id_serialized = &RoomId::try_from(room_id).unwrap();
        joined_rooms
            .iter()
            .any(|room| room.room_id() == room_id_serialized)
    }

    #[tracing::instrument(skip(self))]
    pub async fn get_servers_json(&self, include_members: bool) -> ServersJson {
        let mut rooms = BTreeSet::new();
        let mut all_links = BTreeSet::new();

        let room_id_relations: RelationsMix = self
            .get_all_parent_child()
            .filter_map(|s| s.ok())
            .map(|(key, val)| {
                let parent_hash = GraphDb::fix_size(key.as_ref());
                let child_hashes: BTreeSet<u128> = bincode::deserialize(val.as_ref()).unwrap();
                let parent_hash = u128::from_le_bytes(parent_hash);

                let parent = self.get_room_id_from_hash(&parent_hash);
                let parent_hash = base64::encode(parent_hash.to_le_bytes());
                ((parent_hash, parent), child_hashes)
            })
            .map(|((parent_hash, parent_bytes), child_hashes)| {
                let mut parent = "".to_string();
                if let Some(parent_bytes) = parent_bytes {
                    parent = std::str::from_utf8(parent_bytes.as_ref())
                        .unwrap_or_default()
                        .to_string();
                }

                ((parent_hash, parent), child_hashes)
            })
            .collect();

        if let Some(client) = crate::MATRIX_CLIENT.get() {
            let joined_rooms = client.joined_rooms();
            for ((parent_hash, parent), child_hashes) in room_id_relations {
                if self.is_joined(&parent, &joined_rooms).await {
                    let links: BTreeSet<Link> = child_hashes
                        .iter()
                        .map(|child_hash| base64::encode(child_hash.to_le_bytes()))
                        .map(|child| Link {
                            source: parent_hash.clone(),
                            target: child,
                            value: 1,
                        })
                        .collect();

                    if parent_hash == "4u98GV1CGlCn6PvxBerjrw==" {
                        continue;
                    }
                    all_links.extend(links.into_iter());

                    // Add the parent
                    rooms.insert((parent_hash, parent));
                }
            }

            let missing_child_nodes_links: Vec<&Link> = all_links
                .iter()
                .filter(|link| !rooms.iter().any(|(hash, _)| hash.clone() == link.target))
                .collect();

            // Add missing childs
            for link in &missing_child_nodes_links {
                if let Ok(hash) = base64::decode(link.target.clone()) {
                    let room_hash_bytes = GraphDb::fix_size(hash.as_ref());
                    let room_hash = u128::from_le_bytes(room_hash_bytes);
                    let room_id_bytes = self.get_room_id_from_hash(&room_hash);
                    if let Some(room_id_bytes) = room_id_bytes {
                        let room_id =
                            std::str::from_utf8(room_id_bytes.as_ref()).unwrap_or_default();
                        if self.is_joined(room_id, &joined_rooms).await {
                            if link.target == "4u98GV1CGlCn6PvxBerjrw==" {
                                continue;
                            }
                            rooms.insert((link.target.clone(), room_id.to_string()));
                        }
                    }
                }
            }

            // Remove broken links
            let node_ids: BTreeSet<String> = rooms.iter().map(|(hash, _)| hash.clone()).collect();
            all_links.retain(|link| {
                node_ids.contains(&link.target.to_string())
                    && node_ids.contains(&link.source.to_string())
            });

            let mut servers: BTreeSet<String> = rooms
                .into_iter()
                .filter_map(|(_, room_id)| {
                    let splits: Vec<&str> = room_id.split(':').collect();
                    if splits.len() > 1 {
                        return Some(splits[1].to_string());
                    }
                    None
                })
                .collect();

            if include_members {
                let members = self.get_synapse_joined_members().await;
                servers.extend(members);
            }

            ServersJson { servers }
        } else {
            panic!("SDK Client missing");
        }
    }
}
