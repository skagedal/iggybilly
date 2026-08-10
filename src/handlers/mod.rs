pub mod account;
pub mod auth;
pub mod clips;
pub mod labels;

use std::convert::Infallible;

use askama::Template;
use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts},
    response::{Html, IntoResponse, Response},
    Json,
};
use serde::Serialize;

use crate::{error::AppResult, web::AppState};

/// What a page route describes: which frontend module renders it, what
/// the document is called, and the data that module needs.
///
/// The same envelope is delivered two ways — embedded in the HTML shell
/// on a cold load, and as a bare JSON body when the client router asks
/// for a page it's about to swap in. One shape, one code path on each
/// side.
#[derive(Serialize)]
struct PageEnvelope<'a, T: Serialize> {
    entry: &'a str,
    title: &'a str,
    props: &'a T,
}

/// The HTML shell. It boots the single frontend entry, which reads the
/// envelope below it and renders the right page — so the server still
/// decides what every URL means, and every URL is directly loadable.
#[derive(Template)]
#[template(path = "page.html")]
struct PageShell<'a> {
    title: &'a str,
    css_href: &'a str,
    script_href: &'a str,
    /// This page's own chunk. The entry would otherwise only discover it
    /// after parsing and evaluating, costing a round trip on every cold
    /// load; preloading lets both download together. `None` when there's
    /// no manifest to look it up in.
    preload_href: Option<&'a str>,
    /// The serialised envelope, already escaped for a <script> block.
    payload: String,
}

/// How a page route should answer.
///
/// A browser navigating to a URL sends `Accept: text/html,…` and gets
/// the shell. The client router sends `Accept: application/json` and
/// gets just the envelope, which is all it needs to swap the page
/// without tearing down the document — and so without interrupting
/// whatever the global player is playing.
#[derive(Clone, Copy, Debug)]
pub enum PageFormat {
    Html,
    Json,
}

impl<S> FromRequestParts<S> for PageFormat
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Infallible> {
        let wants_json = parts
            .headers
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("application/json"));
        Ok(if wants_json {
            PageFormat::Json
        } else {
            PageFormat::Html
        })
    }
}

/// Answer a page route: `entry` names the frontend module
/// (`web/src/pages/<entry>.tsx`), `props` is its data.
pub fn page<T: Serialize>(
    state: &AppState,
    format: PageFormat,
    title: &str,
    entry: &str,
    props: &T,
) -> AppResult<Response> {
    let envelope = PageEnvelope {
        entry,
        title,
        props,
    };

    match format {
        PageFormat::Json => Ok(Json(envelope).into_response()),
        PageFormat::Html => {
            // "<" is the only character that can close a <script>
            // element or open a comment, and JSON never needs it
            // structurally — so escaping every occurrence closes the
            // injection hole without changing what the data means.
            let payload = serde_json::to_string(&envelope)?.replace('<', "\\u003c");

            let css_href = state.assets.url("app.css");
            let script_href = state.assets.url("main.js");
            let page_chunk = format!("{entry}.js");

            let shell = PageShell {
                title,
                css_href: &css_href,
                script_href: &script_href,
                preload_href: state.assets.lookup(&page_chunk),
                payload,
            };
            Ok(Html(shell.render()?).into_response())
        }
    }
}
