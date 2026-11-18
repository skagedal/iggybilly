use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionUser {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
}

/// A band, as a request has resolved it.
///
/// Reaching a handler with one of these in hand is the proof that the
/// caller is a member: the only way to build one from a request is the
/// extractor that checks membership, and every query that reads a band's
/// content takes its id. A handler therefore cannot forget the check —
/// there is no way to name a band it hasn't done.
#[derive(Debug, Clone)]
pub struct Band {
    pub id: i64,
    pub slug: String,
    /// This band's own Discord webhook, or `None` to post to the
    /// instance-wide one. Carried here rather than looked up when a post
    /// goes out, because resolving the band already read the row.
    pub discord_webhook_url: Option<String>,
}
