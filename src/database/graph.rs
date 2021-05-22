use crate::webpage::ws::WsMessage;
use color_eyre::Result;
use matrix_sdk::identifiers::RoomId;
use serde::{Deserialize, Serialize};
use sled::{IVec, Iter};
use std::{
    borrow::Cow,
    collections::BTreeSet,
    convert::{TryFrom, TryInto},
};
use tracing::error;

type RelationsMix = Vec<((u128, String), BTreeSet<u128>)>;

#[derive(Debug)]
pub struct GraphDb {
    hash_map: sled::Tree,
    state: sled::Tree,
    parent_child: sled::Tree,
    child_parent: sled::Tree,
}

impl GraphDb {
    pub fn new(
        hash_map: sled::Tree,
        state: sled::Tree,
        parent_child: sled::Tree,
        child_parent: sled::Tree,
    ) -> Self {
        GraphDb {
            hash_map,
            state,
            parent_child,
            child_parent,
        }
    }
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
                let name = if let Ok(name) = room.display_name().await {
                    if name.is_empty() {
                        child.to_string()
                    } else {
                        name
                    }
                } else {
                    child.to_string()
                };

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
                let sse_json = SSEJson {
                    node: RoomRelation {
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
                    },
                    link: Link {
                        source: base64::encode(parent_hash.to_le_bytes()),
                        target: base64::encode(child_hash.to_le_bytes()),
                        value: 1,
                    },
                };
                let j = serde_json::to_string(&sse_json)?;

                for client in crate::WEBSOCKET_CLIENTS.read().await.values() {
                    if let Err(e) = client.send(WsMessage { msg: j.clone() }).await {
                        error!("Failed to send to WS: {}", e);
                    }
                }
            }
        }

        Ok(())
    }

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

    fn hash(input: &str) -> u128 {
        xxhash_rust::xxh3::xxh3_128(input.as_bytes())
    }

    fn map_hash_to_room_id(&self, hash: u128, alias: &str) -> Result<()> {
        self.hash_map.insert(hash.to_le_bytes(), alias.as_bytes())?;
        self.hash_map.flush()?;
        Ok(())
    }

    pub fn knows_room(&self, room_alias: &str) -> bool {
        let room_alias_hash = GraphDb::hash(room_alias);
        if let Ok(res) = self.hash_map.contains_key(room_alias_hash.to_le_bytes()) {
            return res;
        }
        false
    }

    pub fn get_all_room_ids(
        &self,
    ) -> impl DoubleEndedIterator<Item = sled::Result<IVec>> + Send + Sync {
        let r = self.hash_map.iter();
        r.values()
    }

    fn get_room_id_from_hash(&self, hash: &u128) -> Option<IVec> {
        if let Ok(room_id) = self.hash_map.get(hash.to_le_bytes()) {
            return room_id;
        }
        None
    }

    fn get_all_parent_child(&self) -> Iter {
        self.parent_child.iter()
    }

    pub async fn get_json_relations(&self) -> RelationsJson {
        //TODO use HashSet for performance and reuse the already existing hashes
        let mut nodes = BTreeSet::new();
        let mut all_links = BTreeSet::new();

        fn fix_size(raw: &[u8]) -> [u8; 16] {
            raw.try_into().expect("slice with incorrect length")
        }

        let room_id_relations: RelationsMix = self
            .get_all_parent_child()
            .filter_map(|s| s.ok())
            .map(|(key, val)| {
                let parent_hash = fix_size(key.as_ref());
                let child_hashes: BTreeSet<u128> = bincode::deserialize(val.as_ref()).unwrap();
                let parent_hash = u128::from_le_bytes(parent_hash);

                let parent = self.get_room_id_from_hash(&parent_hash);

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

        for ((parent_hash, parent), child_hashes) in room_id_relations {
            let parent_hash = base64::encode(parent_hash.to_le_bytes());

            if let Some(relation) = self
                .generate_room_relation(parent_hash.clone(), &parent)
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
                let room_hash_bytes = fix_size(hash.as_ref());
                let room_hash = u128::from_le_bytes(room_hash_bytes);
                let room_id_bytes = self.get_room_id_from_hash(&room_hash);
                if let Some(room_id_bytes) = room_id_bytes {
                    let room_id = std::str::from_utf8(room_id_bytes.as_ref()).unwrap_or_default();
                    if let Some(relation) = self
                        .generate_room_relation(link.target.clone(), room_id)
                        .await
                    {
                        if relation.name == "MTRNord" || link.target == "4u98GV1CGlCn6PvxBerjrw==" {
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
                node.weight = Some(links);
                node.incoming_links = Some(incoming_links);
                node.outgoing_links = Some(outgoing_links);
                node
            })
            .collect();

        RelationsJson {
            nodes,
            links: all_links,
        }
    }

    async fn generate_room_relation(
        &self,
        room_hash: String,
        room_id: &str,
    ) -> Option<RoomRelation> {
        if let Some(client) = crate::MATRIX_CLIENT.get() {
            let room_id_serialized = &RoomId::try_from(room_id).unwrap();

            if let Some(room) = client
                .joined_rooms()
                .iter()
                .find(|room| room.room_id() == room_id_serialized)
            {
                let alias = if let Some(alias) = room.canonical_alias() {
                    alias.as_str().into()
                } else {
                    room_id.into()
                };
                let name = if let Ok(name) = room.display_name().await {
                    if name.is_empty() {
                        room_id.to_string()
                    } else {
                        name
                    }
                } else {
                    room_id.to_string()
                };

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
                });
            }
        }
        None
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RelationsJson {
    pub nodes: BTreeSet<RoomRelation>,
    pub links: BTreeSet<Link>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SSEJson {
    pub node: RoomRelation,
    pub link: Link,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Ord, Eq, PartialOrd, Hash)]
pub struct Link {
    pub source: String,
    pub target: String,
    pub value: i64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Ord, Eq, PartialOrd, Hash)]
pub struct RoomRelation {
    pub id: String,
    pub room_id: String,
    pub name: String,
    pub alias: String,
    pub avatar: String,
    pub topic: String,
    pub weight: Option<usize>,
    pub incoming_links: Option<usize>,
    pub outgoing_links: Option<usize>,
    pub is_space: bool,
}
