use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, sync::Arc};

#[derive(Serialize, Deserialize, Debug)]
pub struct RelationsJson {
    pub nodes: BTreeSet<RoomRelation>,
    pub links: BTreeSet<Link>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SSEJson {
    pub node: Arc<RoomRelation>,
    pub link: Arc<Link>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Ord, Eq, PartialOrd, Hash)]
pub struct Link {
    pub source: String,
    pub target: String,
    pub value: i32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Ord, Eq, PartialOrd, Hash)]
pub struct RoomRelation {
    pub id: String,
    pub room_id: String,
    pub name: String,
    pub alias: String,
    pub avatar: String,
    pub members: i32,
    pub topic: String,
    pub weight: Option<i32>,
    pub incoming_links: Option<i32>,
    pub outgoing_links: Option<i32>,
    pub is_space: bool,
}

#[derive(Serialize, Deserialize)]
pub enum Jsonline {
    RoomRelation(RoomRelation),
    Links(BTreeSet<Link>),
}
