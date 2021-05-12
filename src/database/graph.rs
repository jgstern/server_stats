use color_eyre::Result;
use matrix_sdk::identifiers::RoomId;
use serde::{Deserialize, Serialize};
use sled::{IVec, Iter};
use std::{
    borrow::Cow,
    convert::{TryFrom, TryInto},
};
use tracing::info;

type RelationsMix = Vec<((u128, String), Vec<u128>)>;

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

        // TODO send data
        if let Some(client) = crate::MATRIX_CLIENT.get() {
            if let Some(room) = client.get_joined_room(&RoomId::try_from(child).unwrap()) {
                let alias = if let Some(alias) = room.canonical_alias() {
                    alias.to_string()
                } else {
                    child.to_string()
                };
                let name = if let Ok(name) = room.display_name().await {
                    name
                } else {
                    child.to_string()
                };

                let topic = if let Some(topic) = room.topic() {
                    topic
                } else {
                    "".to_string()
                };
                let avatar_url = if let Some(avatar_url) = room.avatar_url() {
                    avatar_url.to_string()
                } else {
                    "".to_string()
                };
                //TODO remove seeding rooms
                if name == "MTRNord" {
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
                    },
                    link: Link {
                        source: base64::encode(parent_hash.to_le_bytes()),
                        target: base64::encode(child_hash.to_le_bytes()),
                        value: 1,
                    },
                };

                if let Ok(mut websocket) = crate::WEBSOCKET.write() {
                    websocket.insert(sse_json);
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
        let mut nodes = Vec::new();
        let mut all_links = Vec::new();

        fn fix_size(raw: &[u8]) -> [u8; 16] {
            raw.try_into().expect("slice with incorrect length")
        }

        let room_id_relations: RelationsMix = self
            .get_all_parent_child()
            .filter_map(|s| s.ok())
            .map(|(key, val)| {
                let parent_hash = fix_size(key.as_ref());
                let child_hashes: Vec<u128> = bincode::deserialize(val.as_ref()).unwrap();
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

            // TODO fix that all links work. This might be missing childs that arent parenty anywhere

            if let Some(client) = crate::MATRIX_CLIENT.get() {
                if let Some(room) =
                    client.get_joined_room(&RoomId::try_from(parent.clone()).unwrap())
                {
                    let alias = if let Some(alias) = room.canonical_alias() {
                        alias.to_string()
                    } else {
                        parent.clone()
                    };
                    let name = if let Ok(name) = room.display_name().await {
                        name
                    } else {
                        parent
                    };

                    let topic = if let Some(topic) = room.topic() {
                        topic.to_string()
                    } else {
                        "".to_string()
                    };
                    let avatar_url = if let Some(avatar_url) = room.avatar_url() {
                        avatar_url.to_string()
                    } else {
                        "".to_string()
                    };
                    //TODO remove seeding rooms
                    if name == "MTRNord" {
                        continue;
                    }
                    let mut links: Vec<Link> = child_hashes
                        .iter()
                        .map(|child_hash| base64::encode(child_hash.to_le_bytes()))
                        .filter(|reference| *reference != parent_hash)
                        .map(|child| Link {
                            source: parent_hash.clone(),
                            target: child,
                            value: 1,
                        })
                        .collect();
                    all_links.append(&mut links);
                    nodes.push(RoomRelation {
                        id: parent_hash,
                        name: name.to_string(),
                        alias,
                        avatar: avatar_url,
                        topic,
                        weight: None,
                    });
                }
            }
        }

        // Remove broken links
        let mut indices = Vec::new();
        for (index, link) in all_links.iter_mut().enumerate() {
            if !nodes.iter().any(|x| x.id == link.target) {
                indices.push(index);
            }
            if !nodes.iter().any(|x| x.id == link.source) {
                indices.push(index);
            }
        }

        for (often, index) in indices.iter().enumerate() {
            all_links.remove(index - often);
        }

        for mut node in &mut nodes {
            let links = all_links
                .iter()
                .filter(|x| x.source == node.id || x.target == node.id)
                .count();
            node.weight = Some(links);
        }

        info!("room_id_relations length: {}", nodes.len());
        RelationsJson {
            nodes,
            links: all_links,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RelationsJson {
    pub nodes: Vec<RoomRelation>,
    pub links: Vec<Link>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Clone)]
pub struct SSEJson {
    pub node: RoomRelation,
    pub link: Link,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Clone)]
pub struct Link {
    pub source: String,
    pub target: String,
    pub value: i64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Clone)]
pub struct RoomRelation {
    pub id: String,
    pub name: String,
    pub alias: String,
    pub avatar: String,
    pub topic: String,
    pub weight: Option<usize>,
}
