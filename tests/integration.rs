//! Integration tests. Each test spins up a fresh axum app on a random
//! port, against a fresh tempfile-backed SQLite DB, and drives it over
//! real HTTP with reqwest. That's heavier than oneshot-against-the-
//! router but lets us exercise the session cookie path, multipart
//! upload, redirects, and 409 responses exactly as a browser would.
//!
//! Page routes return an HTML shell whose only interesting content is
//! the JSON props blob the React bundle reads, so assertions about what
//! a page shows go through [`page_envelope`] rather than matching markup.
//! No frontend build is needed: the asset manifest is optional, and the
//! shell falls back to unhashed bundle URLs without it.

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
        // Point away from the real build output so the tests assert on
        // the no-manifest fallback regardless of whether this checkout
        // has run the frontend build.
        static_dir: data_dir.join("static"),
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
        .post(format!("{}/api/login", srv.base))
        .json(&serde_json::json!({ "username": user, "password": pass }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204, "login should succeed");
}

/// Fetch a page as a browser would and return just its props.
async fn get_props(srv: &Server, c: &reqwest::Client, path: &str) -> serde_json::Value {
    let r = c.get(format!("{}{path}", srv.base)).send().await.unwrap();
    assert_eq!(r.status(), 200, "GET {path} should render a page");
    page_envelope(&r.text().await.unwrap())["props"].clone()
}

/// Fetch a page the way the client router does, and return the envelope
/// it gets back — no HTML involved.
async fn get_envelope(srv: &Server, c: &reqwest::Client, path: &str) -> serde_json::Value {
    let r = c
        .get(format!("{}{path}", srv.base))
        .header("accept", "application/json")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "GET {path} should answer the router");
    r.json().await.unwrap()
}

/// Pull the `{entry, title, props}` envelope out of a rendered shell.
///
/// The server writes every `<` in the JSON as `<` so the payload
/// can't close the script element; serde_json turns those back into real
/// characters, exactly as the browser's JSON.parse does.
fn page_envelope(html: &str) -> serde_json::Value {
    const OPEN: &str = r#"<script id="page-data" type="application/json">"#;
    let start = html.find(OPEN).expect("page shell should carry data") + OPEN.len();
    let end = start + html[start..].find("</script>").expect("unclosed data");
    serde_json::from_str(&html[start..end]).expect("page data should be valid JSON")
}

async fn upload_one(srv: &Server, c: &reqwest::Client, filename: &str) {
    upload_many(srv, c, &[filename]).await;
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
        .post(format!("{}/api/clips", srv.base))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
}

async fn add_label(srv: &Server, c: &reqwest::Client, clip_id: i64, name: &str) {
    let r = c
        .post(format!("{}/api/clips/{clip_id}/labels", srv.base))
        .json(&serde_json::json!({ "name": name }))
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
    let html = r.text().await.unwrap();
    // Every page boots the same entry; which module it then loads comes
    // from the envelope's "entry". Tests point static_dir at an empty
    // directory, so this is the no-manifest fallback URL.
    assert!(
        html.contains(r#"src="/static/dist/main.js""#),
        "got: {html}"
    );
    let envelope = page_envelope(&html);
    assert_eq!(envelope["entry"], "index");
    assert_eq!(envelope["title"], "Clips — iggybilly");
    assert_eq!(envelope["props"]["username"], "alice");
}

/// The client router asks for the same URLs with Accept: application/
/// json and gets the envelope alone. Same handler, same data — only the
/// wrapper differs, which is what lets navigation swap a page without
/// tearing down the document (and the playing audio with it).
#[tokio::test]
async fn pages_serve_json_to_the_client_router() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;
    upload_one(&srv, &c, "Riff.mp3").await;

    for (path, entry) in [
        ("/", "index"),
        ("/clips/1", "clip"),
        ("/account", "account"),
    ] {
        let envelope = get_envelope(&srv, &c, path).await;
        assert_eq!(envelope["entry"], entry, "wrong entry for {path}");
        assert!(
            envelope["title"].as_str().unwrap().contains("iggybilly"),
            "{path} should carry a document title"
        );
        assert_eq!(envelope["props"]["username"], "alice");
    }

    // And the browser's own Accept header still gets the full shell.
    let r = c.get(format!("{}/clips/1", srv.base)).send().await.unwrap();
    let html = r.text().await.unwrap();
    assert!(html.starts_with("<!doctype html>"));
    assert_eq!(page_envelope(&html)["entry"], "clip");
}

/// An expired session has to reach the browser as a redirect even on a
/// router fetch, so the client can tell it needs a real navigation to
/// /login rather than rendering a page in place.
#[tokio::test]
async fn router_fetch_without_a_session_redirects() {
    let srv = start().await;
    let r = client()
        .get(format!("{}/", srv.base))
        .header("accept", "application/json")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 303);
    assert_eq!(r.headers().get("location").unwrap(), "/login");
}

#[tokio::test]
async fn login_with_wrong_password_fails() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "right").await;
    let c = client();
    let r = c
        .post(format!("{}/api/login", srv.base))
        .json(&serde_json::json!({ "username": "alice", "password": "wrong" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["error"], "Invalid username or password.");
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
        .post(format!("{}/api/labels/{label_id}/wiki", srv.base))
        .json(&serde_json::json!({
            "content": "# Verse\n\nLyrics **here** <script>alert(1)</script>",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let saved: serde_json::Value = r.json().await.unwrap();
    let html = saved["contentHtml"].as_str().unwrap();
    assert!(html.contains("<strong>here</strong>"), "markdown renders");
    assert!(!html.contains("<script>"), "raw HTML must be stripped");
    assert!(
        saved["content"].as_str().unwrap().starts_with("# Verse"),
        "the raw source comes back too, for the editor"
    );

    // The filtered clip list carries the rendered wiki in its props. The
    // markup is escaped on the way into the <script> block, so a literal
    // "<h1>" in the document would mean the escaping had failed.
    let raw = c
        .get(format!("{}/?label=verse", srv.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        !raw.contains("<h1>Verse</h1>"),
        "wiki HTML must be escaped inside the props script block"
    );
    let idx = page_envelope(&raw)["props"].clone();
    let wiki = &idx["activeWikis"][0];
    assert_eq!(wiki["labelName"], "verse");
    assert!(wiki["contentHtml"]
        .as_str()
        .unwrap()
        .contains("<h1>Verse</h1>"));

    // A second edit appends a revision rather than overwriting.
    c.post(format!("{}/api/labels/{label_id}/wiki", srv.base))
        .json(&serde_json::json!({ "content": "# Verse v2" }))
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

    // History lists revisions newest first, with the newest marked
    // current so only the older ones offer a restore.
    let hist = get_props(&srv, &c, &format!("/labels/{label_id}/wiki/history")).await;
    let revisions = hist["revisions"].as_array().unwrap();
    assert_eq!(revisions.len(), 2);
    assert_eq!(revisions[0]["isCurrent"], true);
    assert_eq!(revisions[1]["isCurrent"], false);

    // Restoring the oldest revision appends it as a new (third) revision.
    let (oldest,): (i64,) =
        sqlx::query_as("SELECT min(id) FROM label_wiki_revisions WHERE label_id = ?")
            .bind(label_id)
            .fetch_one(&srv.pool)
            .await
            .unwrap();
    let r = c
        .post(format!(
            "{}/api/labels/{label_id}/wiki/restore/{oldest}",
            srv.base
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);
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
        .delete(format!("{}/api/clips/1", srv.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);

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
        .delete(format!("{}/api/clips/1", srv.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403, "non-uploader should be forbidden");

    let (clips,): (i64,) = sqlx::query_as("SELECT count(*) FROM clips")
        .fetch_one(&srv.pool)
        .await
        .unwrap();
    assert_eq!(clips, 1, "clip must survive a forbidden delete");

    // Bob's copy of the page is told not to render a delete control;
    // alice's is. The 403 above is what actually enforces it.
    let bob_view = get_props(&srv, &bob, "/clips/1").await;
    assert_eq!(bob_view["clip"]["canDelete"], false);
    let alice_view = get_props(&srv, &alice, "/clips/1").await;
    assert_eq!(alice_view["clip"]["canDelete"], true);
}

#[tokio::test]
async fn deleting_a_missing_clip_404s() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;

    let r = c
        .delete(format!("{}/api/clips/999", srv.base))
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
        .post(format!("{}/api/clips/1/name", srv.base))
        .json(&serde_json::json!({ "name": "Fresh" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["name"], "Fresh");

    let r = c
        .post(format!("{}/api/clips/2/name", srv.base))
        .json(&serde_json::json!({ "name": "Fresh" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);
    let body: serde_json::Value = r.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("already exists"));
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
            .post(format!("{}/api/clips/1/labels", srv.base))
            .json(&serde_json::json!({ "name": input }))
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
            .post(format!("{}/api/clips/1/labels", srv.base))
            .json(&serde_json::json!({ "name": bad }))
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

    let props = get_props(&srv, &c, "/?label=verse").await;
    assert_eq!(
        props["clips"].as_array().unwrap().len(),
        2,
        "?label=verse should match 2 clips"
    );

    let props = get_props(&srv, &c, "/?label=verse&label=chorus").await;
    let clips = props["clips"].as_array().unwrap();
    assert_eq!(
        clips.len(),
        1,
        "?label=verse&label=chorus (AND) should match exactly 1 clip"
    );
    assert_eq!(
        clips[0]["name"], "two",
        "the AND match should be clip 'two'"
    );
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

    let body: serde_json::Value = c
        .get(format!("{}/api/labels/search?q=", srv.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let matches = body["matches"].as_array().unwrap();
    assert!(matches.iter().any(|m| m == "verse"));
    assert!(matches.iter().any(|m| m == "chorus"));
    assert_eq!(
        body["canCreate"], false,
        "empty query shouldn't offer 'create new'"
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

    let body: serde_json::Value = c
        .get(format!("{}/api/labels/search?q=ver", srv.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        body["matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m == "verse"),
        "should list the matching existing label"
    );
    assert_eq!(
        body["canCreate"], true,
        "non-exact match should offer to create"
    );
    assert_eq!(body["query"], "ver", "create uses the normalised query");
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
        .post(format!("{}/api/account/password", srv.base))
        .json(&serde_json::json!({
            "currentPassword": "old-pw-123",
            "newPassword": "new-pw-456789",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);

    // A fresh client (no carried session) shouldn't be able to log in
    // with the old password any more.
    let c2 = client();
    let r = c2
        .post(format!("{}/api/login", srv.base))
        .json(&serde_json::json!({ "username": "alice", "password": "old-pw-123" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401, "old password should now be rejected");

    login(&srv, &c2, "alice", "new-pw-456789").await;
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

    c.post(format!("{}/api/labels/{label_id}/wiki", srv.base))
        .json(&serde_json::json!({ "content": "# Verse" }))
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

// ---------------------------------------------------------------------------
// Playlists
// ---------------------------------------------------------------------------
//
// A label's clips are a playlist, and the order lives on the membership
// row rather than on the clip — so these tests care about two things:
// what the list comes back as, and that a move under one label leaves
// the same clip's place under another alone.

async fn label_id(srv: &Server, name: &str) -> i64 {
    let (id,): (i64,) = sqlx::query_as("SELECT id FROM labels WHERE name = ?")
        .bind(name)
        .fetch_one(&srv.pool)
        .await
        .unwrap();
    id
}

/// The clip ids a page lists, in the order it lists them.
async fn listed_ids(srv: &Server, c: &reqwest::Client, path: &str) -> Vec<i64> {
    get_props(srv, c, path).await["clips"]
        .as_array()
        .unwrap()
        .iter()
        .map(|clip| clip["id"].as_i64().unwrap())
        .collect()
}

/// Move a clip behind another — or to the front, with `None` — and
/// return the order the server answers with.
async fn reorder(
    srv: &Server,
    c: &reqwest::Client,
    label: i64,
    clip_id: i64,
    after_clip_id: Option<i64>,
) -> Vec<i64> {
    let r = c
        .post(format!("{}/api/labels/{label}/order", srv.base))
        .json(&serde_json::json!({ "clipId": clip_id, "afterClipId": after_clip_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "reorder should succeed");
    let body: serde_json::Value = r.json().await.unwrap();
    body["order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect()
}

async fn positions(srv: &Server, label: i64) -> Vec<(i64, i64)> {
    sqlx::query_as(
        "SELECT clip_id, position FROM clip_labels WHERE label_id = ? ORDER BY position, clip_id",
    )
    .bind(label)
    .fetch_all(&srv.pool)
    .await
    .unwrap()
}

/// Three clips, all carrying `name`, labelled oldest first — so the
/// playlist starts as 1, 2, 3 while the unfiltered list is 3, 2, 1.
async fn playlist_of_three(srv: &Server, c: &reqwest::Client, name: &str) -> i64 {
    upload_many(srv, c, &["one.mp3", "two.mp3", "three.mp3"]).await;
    for clip in 1..=3 {
        add_label(srv, c, clip, name).await;
    }
    label_id(srv, name).await
}

/// The migration's backfill statement, read out of the file that ships
/// it so the test exercises the real SQL rather than a copy.
fn backfill_sql() -> String {
    let src = include_str!("../migrations/0005_playlist_order.sql");
    let start = src
        .find("UPDATE clip_labels")
        .expect("a backfill statement");
    let end = start + src[start..].find(';').expect("terminated") + 1;
    src[start..end].to_string()
}

#[tokio::test]
async fn the_backfill_orders_existing_memberships_newest_upload_first() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;
    let label = playlist_of_three(&srv, &c, "set").await;

    // Rows written before the migration carry the column's default, so
    // put them all back to 0 and run the shipped backfill over them.
    sqlx::query("UPDATE clip_labels SET position = 0")
        .execute(&srv.pool)
        .await
        .unwrap();
    sqlx::query(&backfill_sql())
        .execute(&srv.pool)
        .await
        .unwrap();

    assert_eq!(
        listed_ids(&srv, &c, "/?label=set").await,
        vec![3, 2, 1],
        "the day it ships, the playlist is the list people already saw"
    );
    assert_eq!(
        positions(&srv, label).await,
        vec![(3, 0), (2, 1024), (1, 2048)],
        "and the backfilled positions are a gap apart"
    );
}

#[tokio::test]
async fn a_playlist_lists_in_its_own_order_and_a_clip_moves_to_the_front() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;
    let label = playlist_of_three(&srv, &c, "set").await;

    // A newly labelled clip lands at the end, so the playlist is the
    // order they were labelled in — not the reverse-chronological list.
    assert_eq!(listed_ids(&srv, &c, "/?label=set").await, vec![1, 2, 3]);
    assert_eq!(listed_ids(&srv, &c, "/").await, vec![3, 2, 1]);

    let props = get_props(&srv, &c, "/?label=set").await;
    assert_eq!(props["playlist"]["labelId"], label);
    assert_eq!(props["playlist"]["labelName"], "set");

    assert_eq!(reorder(&srv, &c, label, 3, None).await, vec![3, 1, 2]);
    assert_eq!(listed_ids(&srv, &c, "/?label=set").await, vec![3, 1, 2]);
    // One row written: the two it was dropped in front of are untouched.
    assert_eq!(
        positions(&srv, label).await,
        vec![(3, -1024), (1, 0), (2, 1024)]
    );
}

#[tokio::test]
async fn a_clip_moves_to_the_end_of_a_playlist() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;
    let label = playlist_of_three(&srv, &c, "set").await;

    assert_eq!(reorder(&srv, &c, label, 1, Some(3)).await, vec![2, 3, 1]);
    assert_eq!(listed_ids(&srv, &c, "/?label=set").await, vec![2, 3, 1]);
    assert_eq!(
        positions(&srv, label).await,
        vec![(2, 1024), (3, 2048), (1, 3072)]
    );
}

#[tokio::test]
async fn a_used_up_gap_renumbers_the_whole_label() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;
    let label = playlist_of_three(&srv, &c, "set").await;

    // Squeeze clips 1 and 2 together, as a long run of drags into the
    // same gap eventually would.
    for (clip, position) in [(1, 0), (2, 1), (3, 2048)] {
        sqlx::query("UPDATE clip_labels SET position = ? WHERE label_id = ? AND clip_id = ?")
            .bind(position)
            .bind(label)
            .bind(clip)
            .execute(&srv.pool)
            .await
            .unwrap();
    }

    // There is no room between 0 and 1, so the drop renumbers instead of
    // failing — and the order it renumbers to is the order asked for.
    assert_eq!(reorder(&srv, &c, label, 3, Some(1)).await, vec![1, 3, 2]);
    assert_eq!(
        positions(&srv, label).await,
        vec![(1, 0), (3, 1024), (2, 2048)],
        "every row of the label is spaced out again"
    );
    assert_eq!(listed_ids(&srv, &c, "/?label=set").await, vec![1, 3, 2]);
}

#[tokio::test]
async fn moving_a_clip_in_one_playlist_leaves_its_other_playlists_alone() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;
    let set = playlist_of_three(&srv, &c, "set").await;
    // The same three clips in another playlist, labelled the other way
    // round so the two orders can't agree by accident.
    for clip in (1..=3).rev() {
        add_label(&srv, &c, clip, "takes").await;
    }
    let takes = label_id(&srv, "takes").await;

    assert_eq!(reorder(&srv, &c, set, 3, None).await, vec![3, 1, 2]);

    assert_eq!(
        positions(&srv, takes).await,
        vec![(3, 0), (2, 1024), (1, 2048)],
        "a clip's place is a property of the membership, not of the clip"
    );
    assert_eq!(listed_ids(&srv, &c, "/?label=takes").await, vec![3, 2, 1]);
}

#[tokio::test]
async fn reordering_refuses_a_list_that_no_longer_exists() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;
    let label = playlist_of_three(&srv, &c, "set").await;
    upload_one(&srv, &c, "outsider.mp3").await;

    let post = async |label: i64, body: serde_json::Value| {
        c.post(format!("{}/api/labels/{label}/order", srv.base))
            .json(&body)
            .send()
            .await
            .unwrap()
            .status()
    };

    assert_eq!(
        post(label, serde_json::json!({ "clipId": 4, "afterClipId": 1 })).await,
        400,
        "a clip that doesn't carry the label"
    );
    assert_eq!(
        post(label, serde_json::json!({ "clipId": 1, "afterClipId": 4 })).await,
        400,
        "a neighbour that doesn't carry the label"
    );
    assert_eq!(
        post(label, serde_json::json!({ "clipId": 1, "afterClipId": 1 })).await,
        400,
        "a clip moved after itself"
    );
    assert_eq!(
        post(
            9999,
            serde_json::json!({ "clipId": 1, "afterClipId": null })
        )
        .await,
        404,
        "a label that doesn't exist"
    );

    assert_eq!(
        listed_ids(&srv, &c, "/?label=set").await,
        vec![1, 2, 3],
        "and none of that moved anything"
    );
}

#[tokio::test]
async fn an_intersection_of_labels_stays_reverse_chronological() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;
    let set = playlist_of_three(&srv, &c, "set").await;
    for clip in 1..=3 {
        add_label(&srv, &c, clip, "live").await;
    }
    reorder(&srv, &c, set, 3, None).await;

    let props = get_props(&srv, &c, "/?label=set&label=live").await;
    assert!(
        props["playlist"].is_null(),
        "two filters is an intersection, and an intersection has no order"
    );
    let ids: Vec<i64> = props["clips"]
        .as_array()
        .unwrap()
        .iter()
        .map(|clip| clip["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, vec![3, 2, 1]);
}
