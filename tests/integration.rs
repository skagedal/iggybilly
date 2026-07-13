//! Integration tests. Each test spins up a fresh axum app on a random
//! port, against a fresh tempfile-backed SQLite DB, and drives it over
//! real HTTP with reqwest. That's heavier than oneshot-against-the-
//! router but lets us exercise the session cookie path, multipart
//! upload, redirects, and 409 responses exactly as a browser would.

use std::sync::Arc;

use tokio::net::TcpListener;

struct Server {
    base: String,
    pool: sqlx::SqlitePool,
    // Keeping the TempDir and JoinHandle alive for the lifetime of the
    // Server keeps the data dir on disk and the axum task running.
    _temp: tempfile::TempDir,
    _handle: tokio::task::JoinHandle<()>,
}

async fn start() -> Server {
    start_with(None, None).await
}

/// Like `start`, but lets a test wire up the Discord notifier with a
/// webhook URL (e.g. a `mock_webhook` receiver) and a public base URL.
async fn start_with(discord_webhook_url: Option<String>, base_url: Option<String>) -> Server {
    // Each test gets its own tempdir + DB + port → no shared state.
    let temp = tempfile::tempdir().expect("tempdir");
    let data_dir = temp.path().to_path_buf();
    let audio_dir = data_dir.join("audio");
    tokio::fs::create_dir_all(&audio_dir).await.unwrap();
    let db_path = data_dir.join("test.db");

    let config = iggybilly::config::Config {
        listen_addr: "127.0.0.1:0".into(),
        data_dir: data_dir.clone(),
        db_path: db_path.clone(),
        audio_dir,
        // Tests speak plain HTTP to 127.0.0.1; the Secure flag would
        // make reqwest's cookie store drop the session cookie.
        secure_cookies: false,
        discord_webhook_url,
        base_url,
    };

    let pool = iggybilly::db::connect(&db_path).await.unwrap();
    let app = iggybilly::web::build_app(pool.clone(), Arc::new(config))
        .await
        .unwrap();

    // Bind :0 so the kernel picks a free port; read it back from the
    // listener after bind so the test client knows where to connect.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    Server {
        base: format!("http://{addr}"),
        pool,
        _temp: temp,
        _handle: handle,
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .cookie_store(true)
        // Don't follow 303s automatically — we want to assert the
        // redirect target as part of behaviour.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

async fn create_user(pool: &sqlx::SqlitePool, username: &str, password: &str) {
    let hash = iggybilly::auth::hash_password(password).unwrap();
    sqlx::query("INSERT INTO users (username, password_hash, is_admin) VALUES (?, ?, 0)")
        .bind(username)
        .bind(&hash)
        .execute(pool)
        .await
        .unwrap();
}

async fn login(srv: &Server, c: &reqwest::Client, user: &str, pass: &str) {
    let r = c
        .post(format!("{}/login", srv.base))
        .form(&[("username", user), ("password", pass)])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 303, "login should redirect on success");
}

async fn upload_one(srv: &Server, c: &reqwest::Client, filename: &str) {
    let part = reqwest::multipart::Part::bytes(b"fake-audio-bytes".to_vec())
        .file_name(filename.to_string())
        .mime_str("audio/mpeg")
        .unwrap();
    let form = reqwest::multipart::Form::new().part("audio", part);
    let r = c
        .post(format!("{}/clips", srv.base))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 303);
}

/// Post several files in one request, each as its own "audio" part —
/// exactly what the browser sends for a multi-select or drag-and-drop.
async fn upload_many(srv: &Server, c: &reqwest::Client, filenames: &[&str]) {
    let mut form = reqwest::multipart::Form::new();
    for name in filenames {
        let part = reqwest::multipart::Part::bytes(b"fake-audio-bytes".to_vec())
            .file_name(name.to_string())
            .mime_str("audio/mpeg")
            .unwrap();
        form = form.part("audio", part);
    }
    let r = c
        .post(format!("{}/clips", srv.base))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 303);
}

async fn add_label(srv: &Server, c: &reqwest::Client, clip_id: i64, name: &str) {
    let r = c
        .post(format!("{}/clips/{clip_id}/labels", srv.base))
        .form(&[("name", name)])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "add label '{name}' should succeed");
}

#[tokio::test]
async fn unauthenticated_request_redirects_to_login() {
    let srv = start().await;
    let r = client().get(format!("{}/", srv.base)).send().await.unwrap();
    assert_eq!(r.status(), 303);
    assert_eq!(r.headers().get("location").unwrap(), "/login");
}

#[tokio::test]
async fn login_with_correct_password_grants_access() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "passw0rd!").await;
    let c = client();
    login(&srv, &c, "alice", "passw0rd!").await;

    let r = c.get(format!("{}/", srv.base)).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert!(r.text().await.unwrap().contains("Upload"));
}

#[tokio::test]
async fn login_with_wrong_password_fails() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "right").await;
    let c = client();
    let r = c
        .post(format!("{}/login", srv.base))
        .form(&[("username", "alice"), ("password", "wrong")])
        .send()
        .await
        .unwrap();
    // Re-renders the login form with an error rather than redirecting.
    assert_eq!(r.status(), 200);
    assert!(r
        .text()
        .await
        .unwrap()
        .contains("Invalid username or password"));
}

#[tokio::test]
async fn upload_derives_name_and_auto_suffixes_on_conflict() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;

    for _ in 0..3 {
        upload_one(&srv, &c, "Rehearsal.mp3").await;
    }

    let rows: Vec<(String,)> = sqlx::query_as("SELECT name FROM clips ORDER BY id")
        .fetch_all(&srv.pool)
        .await
        .unwrap();
    let names: Vec<&str> = rows.iter().map(|(n,)| n.as_str()).collect();
    assert_eq!(names, vec!["Rehearsal", "Rehearsal (2)", "Rehearsal (3)"]);
}

#[tokio::test]
async fn upload_accepts_multiple_files_in_one_request() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;

    // One POST carrying three files, including a name clash to confirm
    // the per-file unique-name suffixing still applies within a batch.
    upload_many(&srv, &c, &["Verse.mp3", "Chorus.mp3", "Verse.mp3"]).await;

    let rows: Vec<(String,)> = sqlx::query_as("SELECT name FROM clips ORDER BY id")
        .fetch_all(&srv.pool)
        .await
        .unwrap();
    let names: Vec<&str> = rows.iter().map(|(n,)| n.as_str()).collect();
    assert_eq!(names, vec!["Verse", "Chorus", "Verse (2)"]);
}

#[tokio::test]
async fn label_wiki_saves_renders_history_and_restores() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;

    upload_one(&srv, &c, "Song.mp3").await;
    add_label(&srv, &c, 1, "verse").await;
    let (label_id,): (i64,) = sqlx::query_as("SELECT id FROM labels WHERE name = 'verse'")
        .fetch_one(&srv.pool)
        .await
        .unwrap();

    // Save a wiki page; the <script> must be neutralised in the render.
    let r = c
        .post(format!("{}/labels/{label_id}/wiki", srv.base))
        .form(&[(
            "content",
            "# Verse\n\nLyrics **here** <script>alert(1)</script>",
        )])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let body = r.text().await.unwrap();
    assert!(
        body.contains("<strong>here</strong>"),
        "markdown should render"
    );
    assert!(!body.contains("<script>"), "raw HTML must be stripped");

    // The filtered clip list shows the rendered wiki above the clips.
    let idx = c
        .get(format!("{}/?label=verse", srv.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(idx.contains(r#"class="label-wiki""#));
    assert!(idx.contains("<h1>Verse</h1>"));

    // A second edit appends a revision rather than overwriting.
    c.post(format!("{}/labels/{label_id}/wiki", srv.base))
        .form(&[("content", "# Verse v2")])
        .send()
        .await
        .unwrap();
    let (count,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM label_wiki_revisions WHERE label_id = ?")
            .bind(label_id)
            .fetch_one(&srv.pool)
            .await
            .unwrap();
    assert_eq!(count, 2, "each save is a new revision");

    // History lists revisions, with the newest marked current.
    let hist = c
        .get(format!("{}/labels/{label_id}/wiki/history", srv.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(hist.contains("Restore this version"));
    assert!(hist.contains("rev-current"));

    // Restoring the oldest revision appends it as a new (third) revision.
    let (oldest,): (i64,) =
        sqlx::query_as("SELECT min(id) FROM label_wiki_revisions WHERE label_id = ?")
            .bind(label_id)
            .fetch_one(&srv.pool)
            .await
            .unwrap();
    let r = c
        .post(format!(
            "{}/labels/{label_id}/wiki/restore/{oldest}",
            srv.base
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 303, "restore redirects back to history");
    let (count, latest): (i64, String) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM label_wiki_revisions WHERE label_id = ?1),
                (SELECT content FROM label_wiki_revisions WHERE label_id = ?1 ORDER BY id DESC LIMIT 1)",
    )
    .bind(label_id)
    .fetch_one(&srv.pool)
    .await
    .unwrap();
    assert_eq!(count, 3);
    assert!(
        latest.starts_with("# Verse\n\nLyrics"),
        "restored content is now current"
    );
}

#[tokio::test]
async fn uploader_can_delete_own_clip_and_bytes_are_removed() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;

    upload_one(&srv, &c, "Take.mp3").await;
    add_label(&srv, &c, 1, "verse").await;

    // The audio file exists on disk before the delete.
    let (storage,): (String,) = sqlx::query_as("SELECT storage_filename FROM clips WHERE id = 1")
        .fetch_one(&srv.pool)
        .await
        .unwrap();
    let audio_path = srv._temp.path().join("audio").join(&storage);
    assert!(audio_path.exists(), "audio file should exist before delete");

    let r = c
        .delete(format!("{}/clips/1", srv.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    // HTMX is told to navigate back to the clip list.
    assert_eq!(r.headers().get("hx-redirect").unwrap(), "/");

    // Row is gone, its label link is gone (ON DELETE CASCADE), and so
    // are the bytes on disk.
    let (clips,): (i64,) = sqlx::query_as("SELECT count(*) FROM clips")
        .fetch_one(&srv.pool)
        .await
        .unwrap();
    assert_eq!(clips, 0, "clip row should be deleted");
    let (links,): (i64,) = sqlx::query_as("SELECT count(*) FROM clip_labels")
        .fetch_one(&srv.pool)
        .await
        .unwrap();
    assert_eq!(links, 0, "clip_labels rows should cascade-delete");
    assert!(
        !audio_path.exists(),
        "audio file should be removed from disk"
    );
}

#[tokio::test]
async fn cannot_delete_another_users_clip() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    create_user(&srv.pool, "bob", "pw").await;

    // Alice uploads a clip.
    let alice = client();
    login(&srv, &alice, "alice", "pw").await;
    upload_one(&srv, &alice, "Take.mp3").await;

    // Bob may not delete it.
    let bob = client();
    login(&srv, &bob, "bob", "pw").await;
    let r = bob
        .delete(format!("{}/clips/1", srv.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403, "non-uploader should be forbidden");

    let (clips,): (i64,) = sqlx::query_as("SELECT count(*) FROM clips")
        .fetch_one(&srv.pool)
        .await
        .unwrap();
    assert_eq!(clips, 1, "clip must survive a forbidden delete");

    // Bob's own detail page shows no delete control; alice's does.
    let bob_view = bob
        .get(format!("{}/clips/1", srv.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        !bob_view.contains("hx-delete"),
        "bob shouldn't see a delete button on alice's clip"
    );
    let alice_view = alice
        .get(format!("{}/clips/1", srv.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        alice_view.contains("hx-delete"),
        "alice should see a delete button on her own clip"
    );
}

#[tokio::test]
async fn deleting_a_missing_clip_404s() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;

    let r = c
        .delete(format!("{}/clips/999", srv.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);
}

#[tokio::test]
async fn rename_succeeds_and_409s_on_conflict() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;

    upload_one(&srv, &c, "Take.mp3").await;
    upload_one(&srv, &c, "Take.mp3").await;

    let r = c
        .post(format!("{}/clips/1/name", srv.base))
        .form(&[("name", "Fresh")])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    let r = c
        .post(format!("{}/clips/2/name", srv.base))
        .form(&[("name", "Fresh")])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);
    assert!(r.text().await.unwrap().contains("already exists"));
}

#[tokio::test]
async fn add_label_accepts_valid_and_normalises_case() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;
    upload_one(&srv, &c, "a.mp3").await;

    for input in ["verse-1", "Chorus", "pålägg"] {
        let r = c
            .post(format!("{}/clips/1/labels", srv.base))
            .form(&[("name", input)])
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200, "expected 200 for label '{input}'");
    }

    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT name FROM labels ORDER BY name COLLATE NOCASE")
            .fetch_all(&srv.pool)
            .await
            .unwrap();
    let names: Vec<&str> = rows.iter().map(|(n,)| n.as_str()).collect();
    // All stored lowercase, including the previously-uppercase "Chorus".
    assert_eq!(names, vec!["chorus", "pålägg", "verse-1"]);
}

#[tokio::test]
async fn add_label_rejects_invalid_shapes() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;
    upload_one(&srv, &c, "a.mp3").await;

    for bad in ["verse 1", "verse_1", "--verse", "verse-", "verse.1", ""] {
        let r = c
            .post(format!("{}/clips/1/labels", srv.base))
            .form(&[("name", bad)])
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400, "expected 400 for label '{bad}'");
    }

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM labels")
        .fetch_one(&srv.pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0, "no labels should have been created");
}

#[tokio::test]
async fn filter_by_labels_uses_and_semantics() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;

    upload_one(&srv, &c, "one.mp3").await;
    upload_one(&srv, &c, "two.mp3").await;
    upload_one(&srv, &c, "three.mp3").await;

    // clip 1: verse;  clip 2: verse + chorus;  clip 3: chorus
    add_label(&srv, &c, 1, "verse").await;
    add_label(&srv, &c, 2, "verse").await;
    add_label(&srv, &c, 2, "chorus").await;
    add_label(&srv, &c, 3, "chorus").await;

    let body = c
        .get(format!("{}/?label=verse", srv.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(
        body.matches(r#"class="clip-name""#).count(),
        2,
        "?label=verse should match 2 clips"
    );

    let body = c
        .get(format!("{}/?label=verse&label=chorus", srv.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(
        body.matches(r#"class="clip-name""#).count(),
        1,
        "?label=verse&label=chorus (AND) should match exactly 1 clip"
    );
    assert!(body.contains(">two<"), "the AND match should be clip 'two'");
}

#[tokio::test]
async fn label_autocomplete_returns_recent_for_empty_query() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;
    upload_one(&srv, &c, "a.mp3").await;
    add_label(&srv, &c, 1, "verse").await;
    add_label(&srv, &c, 1, "chorus").await;

    let body = c
        .get(format!("{}/labels/search?q=", srv.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("verse"));
    assert!(body.contains("chorus"));
    assert!(
        !body.contains("Create new label"),
        "empty query shouldn't offer 'Create new'"
    );
}

#[tokio::test]
async fn label_autocomplete_offers_create_for_new_query() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;
    upload_one(&srv, &c, "a.mp3").await;
    add_label(&srv, &c, 1, "verse").await;

    let body = c
        .get(format!("{}/labels/search?q=ver", srv.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        body.contains("verse"),
        "should list the matching existing label"
    );
    assert!(
        body.contains("Create new label"),
        "non-exact match should offer to create"
    );
}

#[tokio::test]
async fn audio_download_sets_content_disposition() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;
    upload_one(&srv, &c, "Pålägg.mp3").await;

    // Plain ?audio — inline playback, no disposition header.
    let r = c
        .get(format!("{}/clips/1/audio", srv.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert!(
        r.headers().get("content-disposition").is_none(),
        "inline playback shouldn't force a download"
    );

    // ?download=1 — attachment + RFC 6266 UTF-8 filename* parameter.
    let r = c
        .get(format!("{}/clips/1/audio?download=1", srv.base))
        .send()
        .await
        .unwrap();
    let cd = r
        .headers()
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(cd.starts_with("attachment"), "got: {cd}");
    assert!(
        cd.contains("filename*=UTF-8''"),
        "missing UTF-8 filename: {cd}"
    );
}

#[tokio::test]
async fn change_password_updates_credentials() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "old-pw-123").await;
    let c = client();
    login(&srv, &c, "alice", "old-pw-123").await;

    let r = c
        .post(format!("{}/account", srv.base))
        .form(&[
            ("current_password", "old-pw-123"),
            ("new_password", "new-pw-456789"),
            ("new_password_confirm", "new-pw-456789"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    // A fresh client (no carried session) shouldn't be able to log in
    // with the old password any more.
    let c2 = client();
    let r = c2
        .post(format!("{}/login", srv.base))
        .form(&[("username", "alice"), ("password", "old-pw-123")])
        .send()
        .await
        .unwrap();
    assert_eq!(
        r.status(),
        200,
        "old password should now be rejected (re-renders form)"
    );

    let r = c2
        .post(format!("{}/login", srv.base))
        .form(&[("username", "alice"), ("password", "new-pw-456789")])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 303, "new password should authenticate");
}

/// Spin up a throwaway HTTP server that captures the JSON body of every
/// POST it receives and forwards it down a channel. Stands in for a
/// Discord webhook endpoint so we can assert on what the app would post.
async fn mock_webhook() -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
) {
    use axum::{routing::post, Router};

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let app = Router::new().route(
        "/webhook",
        post(move |body: axum::Json<serde_json::Value>| {
            let tx = tx.clone();
            async move {
                let _ = tx.send(body.0);
                axum::http::StatusCode::NO_CONTENT
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}/webhook"), rx)
}

/// Wait for the next captured webhook post, failing the test if none
/// arrives — the send is fire-and-forget on a spawned task, so we can't
/// just check synchronously after the request returns.
async fn next_post(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
) -> serde_json::Value {
    tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("a webhook post within 5s")
        .expect("webhook channel open")
}

#[tokio::test]
async fn uploading_clips_posts_to_discord() {
    let (webhook, mut rx) = mock_webhook().await;
    let srv = start_with(Some(webhook), Some("https://iggybilly.test".into())).await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;

    // A single upload: one post, naming the clip and linking it.
    upload_one(&srv, &c, "Riff.mp3").await;
    let content = next_post(&mut rx).await["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(content.contains("alice"), "names the uploader: {content}");
    assert!(
        content.contains("[Riff](https://iggybilly.test/clips/1)"),
        "links the clip: {content}"
    );

    // A batch upload: a single post listing both clips, not one each.
    upload_many(&srv, &c, &["Verse.mp3", "Chorus.mp3"]).await;
    let content = next_post(&mut rx).await["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        content.contains("2 clips"),
        "summarises the batch: {content}"
    );
    assert!(
        content.contains("Verse") && content.contains("Chorus"),
        "lists both: {content}"
    );
}

#[tokio::test]
async fn editing_a_wiki_posts_to_discord() {
    let (webhook, mut rx) = mock_webhook().await;
    let srv = start_with(Some(webhook), Some("https://iggybilly.test".into())).await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;

    upload_one(&srv, &c, "Song.mp3").await;
    next_post(&mut rx).await; // drain the upload notification
    add_label(&srv, &c, 1, "verse").await;
    let (label_id,): (i64,) = sqlx::query_as("SELECT id FROM labels WHERE name = 'verse'")
        .fetch_one(&srv.pool)
        .await
        .unwrap();

    c.post(format!("{}/labels/{label_id}/wiki", srv.base))
        .form(&[("content", "# Verse")])
        .send()
        .await
        .unwrap();

    let content = next_post(&mut rx).await["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(content.contains("alice"), "names the editor: {content}");
    assert!(content.contains("verse"), "names the label: {content}");
    assert!(
        content.contains("?label=verse"),
        "links the wiki view: {content}"
    );
}
