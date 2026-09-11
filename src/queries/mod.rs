//! The database reads, in one place.
//!
//! Every read used to sit inline in the page handler that needed it,
//! shaped for that page: dates already formatted, labels already turned
//! into "/?label=…" hrefs. That was fine while the React frontend was
//! the only caller. It isn't: the mobile app needs the same rows without
//! any of the web's link-building, and neither surface should own a
//! second copy of the SQL.
//!
//! So the queries live here and return plain domain types — ids, names,
//! and the timestamps exactly as SQLite stores them. Each surface turns
//! those into its own wire shape: `handlers::` builds the page props the
//! frontend already expects (formatted dates, hrefs), and `api::v1`
//! builds JSON for the app (raw RFC 3339, no hrefs). Neither one is the
//! canonical shape, which is the point.

pub mod clips;
pub mod labels;
pub mod wiki;
