//! Discord notifications via an incoming webhook.
//!
//! Posts a short message to a Discord channel when a clip is uploaded or a
//! label's wiki page is edited. Wiring is intentionally loose: the
//! notifier is cloneable, lives in `AppState`, and is a no-op when no
//! webhook URL is configured (local dev, tests) — so handlers can call it
//! unconditionally without caring whether Discord is set up.
//!
//! Sends are fire-and-forget on a spawned task: a slow or down Discord
//! must never block, slow, or fail an upload or a wiki save. Failures are
//! logged at WARN, not surfaced to the user.

use std::sync::Arc;

use serde::Serialize;

#[derive(Clone)]
pub struct Discord {
    /// `None` => disabled; every method short-circuits to a no-op.
    inner: Option<Arc<Inner>>,
}

struct Inner {
    webhook_url: String,
    /// Public origin (no trailing slash) for building links, or `None` to
    /// post plain names.
    base_url: Option<String>,
    /// Holds a connection pool; cheap to clone into each spawned send.
    client: reqwest::Client,
}

#[derive(Serialize)]
struct WebhookPayload<'a> {
    content: &'a str,
}

impl Discord {
    /// Build from the resolved config. A `None` webhook URL yields a
    /// disabled notifier.
    pub fn new(webhook_url: Option<String>, base_url: Option<String>) -> Self {
        let inner = webhook_url.map(|webhook_url| {
            Arc::new(Inner { webhook_url, base_url, client: reqwest::Client::new() })
        });
        Self { inner }
    }

    /// Announce one or more freshly uploaded clips, as `(id, name)`. A
    /// batch upload posts a single message rather than one per file.
    pub fn clips_uploaded(&self, uploader: &str, clips: &[(i64, String)]) {
        let Some(inner) = &self.inner else { return };
        if clips.is_empty() {
            return;
        }
        let content = inner.upload_message(uploader, clips);
        inner.clone().send(content);
    }

    /// Announce that `editor` saved a new revision of `label`'s wiki page.
    pub fn wiki_edited(&self, editor: &str, label: &str) {
        let Some(inner) = &self.inner else { return };
        let content = inner.wiki_message(editor, label);
        inner.clone().send(content);
    }
}

impl Inner {
    /// Spawn the actual POST so the caller (a request handler) returns
    /// immediately. Errors are logged, never propagated.
    fn send(self: Arc<Self>, content: String) {
        tokio::spawn(async move {
            let res = self
                .client
                .post(&self.webhook_url)
                .json(&WebhookPayload { content: &content })
                .send()
                .await;
            match res {
                Ok(r) if r.status().is_success() => {}
                Ok(r) => {
                    tracing::warn!(status = %r.status(), "Discord webhook returned non-success")
                }
                Err(e) => tracing::warn!(error = ?e, "failed to post to Discord webhook"),
            }
        });
    }

    fn upload_message(&self, uploader: &str, clips: &[(i64, String)]) -> String {
        if let [(id, name)] = clips {
            format!("🎵 **{}** uploaded a clip: {}", md_escape(uploader), self.clip_link(*id, name))
        } else {
            let mut msg =
                format!("🎵 **{}** uploaded {} clips:", md_escape(uploader), clips.len());
            for (id, name) in clips {
                msg.push_str("\n• ");
                msg.push_str(&self.clip_link(*id, name));
            }
            msg
        }
    }

    fn wiki_message(&self, editor: &str, label: &str) -> String {
        // Labels are validated lower-kebab-case, so they need no markdown
        // escaping; they can carry Unicode letters, hence url-encoding for
        // the query value.
        let label_part = match &self.base_url {
            Some(base) => format!("[`{label}`]({base}/?{})", encode_label_query(label)),
            None => format!("`{label}`"),
        };
        format!("📝 **{}** edited the wiki for {label_part}", md_escape(editor))
    }

    /// A clip as a Discord masked link when we know the public origin, or
    /// just its bolded name otherwise.
    fn clip_link(&self, id: i64, name: &str) -> String {
        match &self.base_url {
            Some(base) => format!("[{}]({base}/clips/{id})", md_escape(name)),
            None => format!("**{}**", md_escape(name)),
        }
    }
}

/// `label=<percent-encoded>` matching the home page's `?label=` filter,
/// which is exactly the URL that surfaces a label's wiki page.
fn encode_label_query(label: &str) -> String {
    serde_urlencoded::to_string([("label", label)]).unwrap_or_default()
}

/// Escape the characters Discord treats as markdown so a clip name or
/// username can't break our formatting or smuggle in a masked link. Also
/// neutralises `@` so a name like `@everyone` can't ping the channel.
fn md_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '*' | '_' | '~' | '`' | '|' | '>' | '[' | ']' | '(' | ')' | '@') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inner(base: Option<&str>) -> Inner {
        Inner {
            webhook_url: "https://example.invalid/webhook".into(),
            base_url: base.map(str::to_string),
            client: reqwest::Client::new(),
        }
    }

    #[test]
    fn single_upload_with_base_url_links_the_clip() {
        let msg = inner(Some("https://i.test")).upload_message("simon", &[(7, "Riff".into())]);
        assert_eq!(msg, "🎵 **simon** uploaded a clip: [Riff](https://i.test/clips/7)");
    }

    #[test]
    fn batch_upload_lists_each_clip() {
        let clips = [(1, "A".into()), (2, "B".into())];
        let msg = inner(Some("https://i.test")).upload_message("simon", &clips);
        assert_eq!(
            msg,
            "🎵 **simon** uploaded 2 clips:\n\
             • [A](https://i.test/clips/1)\n\
             • [B](https://i.test/clips/2)"
        );
    }

    #[test]
    fn without_base_url_clips_are_plain_names() {
        let msg = inner(None).upload_message("simon", &[(7, "Riff".into())]);
        assert_eq!(msg, "🎵 **simon** uploaded a clip: **Riff**");
    }

    #[test]
    fn wiki_edit_links_the_filtered_view() {
        let msg = inner(Some("https://i.test")).wiki_message("simon", "verse-1");
        assert_eq!(msg, "📝 **simon** edited the wiki for [`verse-1`](https://i.test/?label=verse-1)");
    }

    #[test]
    fn names_are_escaped_against_markdown_and_pings() {
        // A malicious clip name can't inject a masked link or ping.
        let msg = inner(None).upload_message("@everyone", &[(1, "[x](http://evil)".into())]);
        assert_eq!(
            msg,
            "🎵 **\\@everyone** uploaded a clip: **\\[x\\]\\(http://evil\\)**"
        );
    }
}
