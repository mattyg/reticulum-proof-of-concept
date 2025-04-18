use serde::{Deserialize, Serialize};

/// Info about this chatter to include in announce packet
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AnnounceInfo {
    pub nick: String,
}
