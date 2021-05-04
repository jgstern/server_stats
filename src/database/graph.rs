use color_eyre::Result;
use matrix_sdk::identifiers::RoomId;
use serde::{Deserialize, Serialize};
use sled::{IVec, Iter};
use std::{
    borrow::Cow,
    collections::HashMap,
    convert::{TryFrom, TryInto},
};
use tracing::info;

type RelationsMix = Vec<((u128, String), (Vec<u128>, Vec<String>))>;

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
    pub fn get_parent(&self, child: &str) -> Option<Cow<str>> {
        let child_hash = GraphDb::hash(child);
        if let Ok(Some(parent)) = self.child_parent.get(child_hash.to_le_bytes()) {
            let hash = parent.as_ref();
            if let Ok(Some(parent)) = self.hash_map.get(hash) {
                let parent_id = String::from_utf8_lossy(parent.as_ref());
                return Some(parent_id.to_string().into());
            }
        }
        None
    }

    pub fn add_child(&self, parent: &str, child: &str) -> Result<()> {
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
                    let mut backing_bytes = sled::IVec::from(existing);

                    let mut decoded: Vec<u128> =
                        bincode::deserialize(&backing_bytes.as_mut()).unwrap();

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
        Ok(())
    }

    fn add_parent(&self, parent: u128, child: u128) -> Result<()> {
        self.parent_child
            .update_and_fetch(child.to_le_bytes(), |value_opt| {
                if let Some(existing) = value_opt {
                    let mut backing_bytes = sled::IVec::from(existing);

                    let mut decoded: Vec<u128> =
                        bincode::deserialize(&backing_bytes.as_mut()).unwrap();
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
        let mut rooms = HashMap::new();

        fn fix_size(raw: &[u8]) -> [u8; 16] {
            raw.try_into().expect("slice with incorrect length")
        }

        let room_id_relations: RelationsMix = self
            .get_all_parent_child()
            .filter_map(|s| s.ok())
            .map(|(key, val)| {
                let key = fix_size(key.as_ref());
                let decoded_val: Vec<u128> = bincode::deserialize(val.as_ref()).unwrap();
                let key = u128::from_le_bytes(key);

                (key, decoded_val)
            })
            .map(|(parent_hash, child_hashes)| {
                let parent = self.get_room_id_from_hash(&parent_hash);
                let childs: Vec<Option<IVec>> = child_hashes
                    .iter()
                    .map(|child_hash| self.get_room_id_from_hash(child_hash))
                    .collect();
                ((parent_hash, parent), (child_hashes, childs))
            })
            .map(
                |((parent_hash, parent_bytes), (child_hashes, childs_bytes))| {
                    let mut parent = "".to_string();
                    if let Some(parent_bytes) = parent_bytes {
                        parent = std::str::from_utf8(parent_bytes.as_ref())
                            .unwrap_or_default()
                            .to_string();
                    }
                    let childs: Vec<String> = childs_bytes
                        .iter()
                        .map(|child_bytes| {
                            let mut child = "".to_string();
                            if let Some(child_bytes) = child_bytes {
                                child = std::str::from_utf8(child_bytes.as_ref())
                                    .unwrap_or_default()
                                    .to_string();
                            }
                            child
                        })
                        .collect();

                    ((parent_hash, parent), (child_hashes, childs))
                },
            )
            .collect();

        info!("room_id_relations length: {}", room_id_relations.len());
        for ((parent_hash, parent), (child_hashes, _)) in room_id_relations {
            let parent_hash = base64::encode(parent_hash.to_le_bytes());
            let child_refs: Vec<Ref> = child_hashes
                .iter()
                .map(|child_hash| {
                    let child_hash = base64::encode(child_hash.to_le_bytes());
                    Ref { ref_id: child_hash }
                })
                .filter(|reference| reference.ref_id != parent_hash)
                .collect();
            // TODO fix that all links work. This might be missing childs that arent parenty anywhere

            if let Some(client) = crate::MATRIX_CLIENT.get() {
                if let Some(room) =
                    client.get_joined_room(&RoomId::try_from(parent.clone()).unwrap())
                {
                    let alias = if let Some(alias) = room.canonical_alias() {
                        Some(alias.to_string())
                    } else {
                        None
                    };
                    let name = if let Ok(name) = room.display_name().await {
                        name
                    } else {
                        parent
                    };
                    rooms.insert(
                        parent_hash,
                        RoomRelation {
                            name: name.to_string(),
                            alias,
                            links: child_refs,
                        },
                    );
                }
            }
        }
        RelationsJson { rooms }
    }
}

/*
{
  "rooms": {
    "ajdjfiojeioj": {"name": "Watercooler", "links": [{"$ref": "#/rooms/gejiogjio"}]},
    "gejiogjio": {"name": "#offtopic", "links": [{"$ref": "#/rooms/ajdjfiojeioj"}]},
  }
}
*/
#[derive(Serialize, Deserialize)]
pub struct RelationsJson {
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub rooms: HashMap<String, RoomRelation>,
}

#[derive(Serialize, Deserialize)]
pub struct RoomRelation {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Ref>,
}

#[derive(Serialize, Deserialize)]
pub struct Ref {
    #[serde(rename = "$ref")]
    pub ref_id: String,
}

impl PartialEq for Ref {
    fn eq(&self, other: &Self) -> bool {
        self.ref_id == other.ref_id
    }
}
