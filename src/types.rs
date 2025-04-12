use serde::{Serialize, Deserialize};

/// Info about this chatter to include in announce packet
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AnnounceInfo {
    pub nick: String
}

/// Info about this chatter to include in announce packet
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message(pub String);