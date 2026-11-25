//! Discord notifications via an incoming webhook.
//!
//! Posts a short message to a Discord channel when a clip is uploaded or a
//! label's wiki page is edited. Wiring is intentionally loose: the
//! notifier is cloneable, lives in `AppState`, and is a no-op when no
//! webhook URL applies (local dev, tests) — so handlers can call it
//! unconditionally without caring whether Discord is set up.
//!
//! Which channel a post goes to is the band's business. A band with its
//! own webhook uses it; a band without one falls back to the
//! instance-wide `IGGYBILLY_DISCORD_WEBHOOK_URL`, which is what every
//! band did before there was more than one of them. Defaulting rather
//! than requiring is what keeps an existing instance behaving exactly as
//! it did, and a band that wants its own channel says so with
//! `set-band-webhook`.
//!
//! Sends are fire-and-forget on a spawned task: a slow or down Discord
//! must never block, slow, or fail an upload or a wiki save. Failures are
//! logged at WARN, not surfaced to the user.

use std::sync::Arc;

use serde::Serialize;

use crate::models::Band;

#[derive(Clone)]
pub struct Discord {
    inner: Arc<Inner>,
}

struct Inner {
    /// Where a band with no webhook of its own posts. `None` disables
    /// notifications for every such band.
    default_webhook_url: Option<String>,
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
    /// Build from the resolved config. The notifier exists either way —
    /// a band may carry its own webhook when the instance has none — and
    /// each post decides for itself whether it has anywhere to go.
    pub fn new(default_webhook_url: Option<String>, base_url: Option<String>) -> Self {
        Self {
            inner: Arc::new(Inner {
                default_webhook_url,
                base_url,
                client: reqwest::Client::new(),
            }),
        }
    }

    /// Announce one or more freshly uploaded clips, as `(id, name)`. A
    /// batch upload posts a single message rather than one per file.
    pub fn clips_uploaded(&self, band: &Band, uploader: &str, clips: &[(i64, String)]) {
        if clips.is_empty() {
            return;
        }
        let Some(webhook) = self.inner.webhook_for(band) else {
            return;
        };
        let content = self.inner.upload_message(band, uploader, clips);
        self.inner.clone().send(webhook, content);
    }

    /// Announce that `editor` saved a new revision of `label`'s wiki page.
    pub fn wiki_edited(&self, band: &Band, editor: &str, label: &str) {
        let Some(webhook) = self.inner.webhook_for(band) else {
            return;
        };
        let content = self.inner.wiki_message(band, editor, label);
        self.inner.clone().send(webhook, content);
    }
}

impl Inner {
    /// The band's own channel, else the instance's, else nowhere.
    fn webhook_for(&self, band: &Band) -> Option<String> {
        band.discord_webhook_url
            .clone()
            .or_else(|| self.default_webhook_url.clone())
    }

    /// Spawn the actual POST so the caller (a request handler) returns
    /// immediately. Errors are logged, never propagated.
    fn send(self: Arc<Self>, webhook_url: String, content: String) {
        tokio::spawn(async move {
            let res = self
                .client
                .post(&webhook_url)
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

    fn upload_message(&self, band: &Band, uploader: &str, clips: &[(i64, String)]) -> String {
        if let [(id, name)] = clips {
            format!(
                "🎵 **{}** uploaded a clip: {}",
                md_escape(uploader),
                self.clip_link(band, *id, name)
            )
        } else {
            let mut msg = format!(
                "🎵 **{}** uploaded {} clips:",
                md_escape(uploader),
                clips.len()
            );
            for (id, name) in clips {
                msg.push_str("\n• ");
                msg.push_str(&self.clip_link(band, *id, name));
            }
            msg
        }
    }

    fn wiki_message(&self, band: &Band, editor: &str, label: &str) -> String {
        // Labels are validated lower-kebab-case, so they need no markdown
        // escaping; they can carry Unicode letters, hence url-encoding for
        // the query value.
        let label_part = match &self.base_url {
            Some(base) => format!(
                "[`{label}`]({base}/{}/?{})",
                band.slug,
                encode_label_query(label)
            ),
            None => format!("`{label}`"),
        };
        format!(
            "📝 **{}** edited the wiki for {label_part}",
            md_escape(editor)
        )
    }

    /// A clip as a Discord masked link when we know the public origin, or
    /// just its bolded name otherwise. The link carries the band's slug,
    /// like every other link to a clip.
    fn clip_link(&self, band: &Band, id: i64, name: &str) -> String {
        match &self.base_url {
            Some(base) => format!("[{}]({base}/{}/clips/{id})", md_escape(name), band.slug),
            None => format!("**{}**", md_escape(name)),
        }
    }
}

/// `label=<percent-encoded>` matching the band list's `?label=` filter,
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
        if matches!(
            c,
            '\\' | '*' | '_' | '~' | '`' | '|' | '>' | '[' | ']' | '(' | ')' | '@'
        ) {
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
            default_webhook_url: Some("https://example.invalid/webhook".into()),
            base_url: base.map(str::to_string),
            client: reqwest::Client::new(),
        }
    }

    fn band(slug: &str, webhook: Option<&str>) -> Band {
        Band {
            id: 1,
            slug: slug.into(),
            discord_webhook_url: webhook.map(str::to_string),
        }
    }

    #[test]
    fn single_upload_with_base_url_links_the_clip() {
        let msg = inner(Some("https://i.test")).upload_message(
            &band("bonband", None),
            "simon",
            &[(7, "Riff".into())],
        );
        assert_eq!(
            msg,
            "🎵 **simon** uploaded a clip: [Riff](https://i.test/bonband/clips/7)"
        );
    }

    #[test]
    fn batch_upload_lists_each_clip() {
        let clips = [(1, "A".into()), (2, "B".into())];
        let msg =
            inner(Some("https://i.test")).upload_message(&band("bonband", None), "simon", &clips);
        assert_eq!(
            msg,
            "🎵 **simon** uploaded 2 clips:\n\
             • [A](https://i.test/bonband/clips/1)\n\
             • [B](https://i.test/bonband/clips/2)"
        );
    }

    #[test]
    fn without_base_url_clips_are_plain_names() {
        let msg =
            inner(None).upload_message(&band("bonband", None), "simon", &[(7, "Riff".into())]);
        assert_eq!(msg, "🎵 **simon** uploaded a clip: **Riff**");
    }

    #[test]
    fn wiki_edit_links_the_filtered_view() {
        let msg =
            inner(Some("https://i.test")).wiki_message(&band("bonband", None), "simon", "verse-1");
        assert_eq!(
            msg,
            "📝 **simon** edited the wiki for [`verse-1`](https://i.test/bonband/?label=verse-1)"
        );
    }

    #[test]
    fn names_are_escaped_against_markdown_and_pings() {
        // A malicious clip name can't inject a masked link or ping.
        let msg = inner(None).upload_message(
            &band("bonband", None),
            "@everyone",
            &[(1, "[x](http://evil)".into())],
        );
        assert_eq!(
            msg,
            "🎵 **\\@everyone** uploaded a clip: **\\[x\\]\\(http://evil\\)**"
        );
    }

    /// A band's own webhook wins; without one it falls back to the
    /// instance's; with neither there is nowhere to post.
    #[test]
    fn band_webhook_overrides_the_instance_default() {
        let i = inner(None);
        assert_eq!(
            i.webhook_for(&band("a", Some("https://own.invalid/hook")))
                .as_deref(),
            Some("https://own.invalid/hook")
        );
        assert_eq!(
            i.webhook_for(&band("a", None)).as_deref(),
            Some("https://example.invalid/webhook")
        );

        let none = Inner {
            default_webhook_url: None,
            base_url: None,
            client: reqwest::Client::new(),
        };
        assert_eq!(none.webhook_for(&band("a", None)), None);
    }
}
