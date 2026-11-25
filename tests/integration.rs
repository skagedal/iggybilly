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
        soundfont: None,
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
    // Every test user is in `default`, the band the migration creates.
    // A user in no band can sign in and reach nothing, which is its own
    // test rather than the starting point for every other one.
    add_to_band(pool, username, "default").await;
}

/// Put a user in a band, creating the band if this is its first use.
async fn add_to_band(pool: &sqlx::SqlitePool, username: &str, slug: &str) {
    sqlx::query("INSERT OR IGNORE INTO bands (slug) VALUES (?)")
        .bind(slug)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT OR IGNORE INTO band_members (band_id, user_id)
         SELECT (SELECT id FROM bands WHERE slug = ?), (SELECT id FROM users WHERE username = ?)",
    )
    .bind(slug)
    .bind(username)
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
        let part = reqwest::multipart::Part::bytes(audio_bytes(name))
            .file_name(name.to_string())
            .mime_str("audio/mpeg")
            .unwrap();
        form = form.part("audio", part);
    }
    let r = c
        .post(format!("{}/api/default/clips", srv.base))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
}

async fn add_label(srv: &Server, c: &reqwest::Client, clip_id: i64, name: &str) {
    let r = c
        .post(format!("{}/api/default/clips/{clip_id}/labels", srv.base))
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

    // `/` is nobody's band, so it sends you to yours.
    let r = c.get(format!("{}/", srv.base)).send().await.unwrap();
    assert_eq!(r.status(), 303);
    assert_eq!(r.headers().get("location").unwrap(), "/default/");

    let r = c
        .get(format!("{}/default/", srv.base))
        .send()
        .await
        .unwrap();
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
        ("/default/", "index"),
        ("/default/clips/1", "clip"),
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
    let r = c
        .get(format!("{}/default/clips/1", srv.base))
        .send()
        .await
        .unwrap();
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
        .post(format!("{}/api/default/labels/{label_id}/wiki", srv.base))
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
        .get(format!("{}/default/?label=verse", srv.base))
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
    c.post(format!("{}/api/default/labels/{label_id}/wiki", srv.base))
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
    let hist = get_props(
        &srv,
        &c,
        &format!("/default/labels/{label_id}/wiki/history"),
    )
    .await;
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
            "{}/api/default/labels/{label_id}/wiki/restore/{oldest}",
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
        .delete(format!("{}/api/default/clips/1", srv.base))
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
        .delete(format!("{}/api/default/clips/1", srv.base))
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
    let bob_view = get_props(&srv, &bob, "/default/clips/1").await;
    assert_eq!(bob_view["clip"]["canDelete"], false);
    let alice_view = get_props(&srv, &alice, "/default/clips/1").await;
    assert_eq!(alice_view["clip"]["canDelete"], true);
}

#[tokio::test]
async fn deleting_a_missing_clip_404s() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    let c = client();
    login(&srv, &c, "alice", "pw").await;

    let r = c
        .delete(format!("{}/api/default/clips/999", srv.base))
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
        .post(format!("{}/api/default/clips/1/name", srv.base))
        .json(&serde_json::json!({ "name": "Fresh" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["name"], "Fresh");

    let r = c
        .post(format!("{}/api/default/clips/2/name", srv.base))
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
            .post(format!("{}/api/default/clips/1/labels", srv.base))
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
            .post(format!("{}/api/default/clips/1/labels", srv.base))
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

    let props = get_props(&srv, &c, "/default/?label=verse").await;
    assert_eq!(
        props["clips"].as_array().unwrap().len(),
        2,
        "?label=verse should match 2 clips"
    );

    let props = get_props(&srv, &c, "/default/?label=verse&label=chorus").await;
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
        .get(format!("{}/api/default/labels/search?q=", srv.base))
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
        .get(format!("{}/api/default/labels/search?q=ver", srv.base))
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
        .get(format!("{}/default/clips/1/audio", srv.base))
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
        .get(format!("{}/default/clips/1/audio?download=1", srv.base))
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
        content.contains("[Riff](https://iggybilly.test/default/clips/1)"),
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

    c.post(format!("{}/api/default/labels/{label_id}/wiki", srv.base))
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
        .post(format!("{}/api/default/labels/{label}/order", srv.base))
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
        listed_ids(&srv, &c, "/default/?label=set").await,
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
    assert_eq!(
        listed_ids(&srv, &c, "/default/?label=set").await,
        vec![1, 2, 3]
    );
    assert_eq!(listed_ids(&srv, &c, "/default/").await, vec![3, 2, 1]);

    let props = get_props(&srv, &c, "/default/?label=set").await;
    assert_eq!(props["playlist"]["labelId"], label);
    assert_eq!(props["playlist"]["labelName"], "set");

    assert_eq!(reorder(&srv, &c, label, 3, None).await, vec![3, 1, 2]);
    assert_eq!(
        listed_ids(&srv, &c, "/default/?label=set").await,
        vec![3, 1, 2]
    );
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
    assert_eq!(
        listed_ids(&srv, &c, "/default/?label=set").await,
        vec![2, 3, 1]
    );
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
    assert_eq!(
        listed_ids(&srv, &c, "/default/?label=set").await,
        vec![1, 3, 2]
    );
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
    assert_eq!(
        listed_ids(&srv, &c, "/default/?label=takes").await,
        vec![3, 2, 1]
    );
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
        c.post(format!("{}/api/default/labels/{label}/order", srv.base))
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
        listed_ids(&srv, &c, "/default/?label=set").await,
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

    let props = get_props(&srv, &c, "/default/?label=set&label=live").await;
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

/// Real audio for an upload named `name`, chosen by its extension: the
/// server identifies files by content and refuses anything else.
fn audio_bytes(name: &str) -> Vec<u8> {
    let fixture = match name.rsplit('.').next() {
        Some("m4a") => "guitar-clip.m4a",
        Some("aac") => "audio/clip.aac",
        _ => "audio/clip.mp3",
    };
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture),
    )
    .unwrap()
}

// ---------------------------------------------------------------------
// Bands
//
// The tests above all run in `default` and say what one band does. These
// say what two bands do, which is almost entirely: not see each other.
// ---------------------------------------------------------------------

/// Upload a clip into a named band and return its id.
async fn upload_into(srv: &Server, c: &reqwest::Client, band: &str, filename: &str) -> i64 {
    let part = reqwest::multipart::Part::bytes(audio_bytes(filename))
        .file_name(filename.to_string())
        .mime_str("audio/mpeg")
        .unwrap();
    let form = reqwest::multipart::Form::new().part("audio", part);
    let r = c
        .post(format!("{}/api/{band}/clips", srv.base))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "upload into {band} should succeed");
    let body: serde_json::Value = r.json().await.unwrap();
    body["clips"][0]["id"]
        .as_i64()
        .expect("an uploaded clip id")
}

/// A band the caller is not a member of is indistinguishable from one
/// that was never created. Both 404, so trying slugs tells a member of
/// one band nothing about who else is on the instance.
#[tokio::test]
async fn a_band_you_are_not_in_looks_exactly_like_one_that_does_not_exist() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    create_user(&srv.pool, "bob", "pw").await;
    add_to_band(&srv.pool, "bob", "bonband").await;

    let c = client();
    login(&srv, &c, "alice", "pw").await;

    for path in ["/bonband/", "/nosuchband/", "/bonband/clips/1"] {
        let r = c.get(format!("{}{path}", srv.base)).send().await.unwrap();
        assert_eq!(r.status(), 404, "GET {path} should be a plain 404");
    }
}

/// Two bands can use the same words without meeting: the same clip name
/// and the same label name are different rows, and neither list shows
/// the other's.
#[tokio::test]
async fn two_bands_can_hold_the_same_names_and_see_none_of_each_other() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    add_to_band(&srv.pool, "alice", "bonband").await;

    let c = client();
    login(&srv, &c, "alice", "pw").await;

    let in_default = upload_into(&srv, &c, "default", "Riff.mp3").await;
    let in_bonband = upload_into(&srv, &c, "bonband", "Riff.mp3").await;
    assert_ne!(in_default, in_bonband, "two clips, not one");

    // The same name landed in both, with no " (2)" suffix in sight: the
    // suffixing resolves within a band.
    for (band, id) in [("default", in_default), ("bonband", in_bonband)] {
        let props = get_props(&srv, &c, &format!("/{band}/clips/{id}")).await;
        assert_eq!(props["clip"]["name"], "Riff", "{band} kept the plain name");
        assert_eq!(props["band"], band);
    }

    // And the same label name in both.
    for (band, id) in [("default", in_default), ("bonband", in_bonband)] {
        let r = c
            .post(format!("{}/api/{band}/clips/{id}/labels", srv.base))
            .json(&serde_json::json!({ "name": "verse-1" }))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200, "{band} should accept verse-1");
    }

    // Each band's list holds exactly its own clip.
    assert_eq!(listed_ids(&srv, &c, "/default/").await, vec![in_default]);
    assert_eq!(listed_ids(&srv, &c, "/bonband/").await, vec![in_bonband]);
    assert_eq!(
        listed_ids(&srv, &c, "/bonband/?label=verse-1").await,
        vec![in_bonband],
        "filtering by a shared label name stays inside the band"
    );
}

/// A clip id is global, so naming one from another band under this
/// band's URL is the interesting attack. Every route that takes a clip
/// id answers as it would for an id that was never used.
#[tokio::test]
async fn another_bands_clip_id_is_not_found_under_your_band() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    add_to_band(&srv.pool, "alice", "bonband").await;

    let c = client();
    login(&srv, &c, "alice", "pw").await;
    let theirs = upload_into(&srv, &c, "bonband", "Riff.mp3").await;

    // Alice is in both bands and uploaded this clip herself — so the
    // uploader check cannot be what refuses her. Only the band can.
    let r = c
        .get(format!("{}/default/clips/{theirs}", srv.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404, "the clip page");

    let r = c
        .get(format!("{}/default/clips/{theirs}/audio", srv.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404, "the audio bytes");

    let r = c
        .post(format!("{}/api/default/clips/{theirs}/name", srv.base))
        .json(&serde_json::json!({ "name": "Renamed" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404, "renaming");

    let r = c
        .post(format!("{}/api/default/clips/{theirs}/labels", srv.base))
        .json(&serde_json::json!({ "name": "verse-1" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404, "labelling");

    let r = c
        .delete(format!("{}/api/default/clips/{theirs}", srv.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404, "deleting");

    // And it is all still there under its own band.
    let props = get_props(&srv, &c, &format!("/bonband/clips/{theirs}")).await;
    assert_eq!(props["clip"]["name"], "Riff");
}

/// An account in no band can sign in — the password is right — and then
/// has nowhere to go. The web says so on a page; the JSON API says so
/// with 403, never 401, which the app would read as a dead token.
#[tokio::test]
async fn a_user_in_no_band_is_told_so_rather_than_signed_out() {
    let srv = start().await;
    // Deliberately not `create_user`, which would put them in a band.
    let hash = iggybilly::auth::hash_password("pw").unwrap();
    sqlx::query("INSERT INTO users (username, password_hash, is_admin) VALUES ('nomad', ?, 0)")
        .bind(&hash)
        .execute(&srv.pool)
        .await
        .unwrap();

    let c = client();
    login(&srv, &c, "nomad", "pw").await;

    let envelope = get_envelope(&srv, &c, "/").await;
    assert_eq!(envelope["entry"], "no-band");
    assert_eq!(envelope["props"]["username"], "nomad");

    // The app's un-banded route has no band to fall back on.
    let r = c
        .get(format!("{}/api/v1/clips", srv.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403, "403, so the app keeps its token");
    let body: serde_json::Value = r.json().await.unwrap();
    assert!(
        body["error"].as_str().unwrap().contains("not a member"),
        "says what is wrong: {body}"
    );
}

/// `/` sends you to the band you were last in, and following a link into
/// another band you belong to makes that the one you were last in — so
/// nobody has to switch bands before a link works.
#[tokio::test]
async fn the_landing_page_follows_the_band_you_last_looked_at() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    add_to_band(&srv.pool, "alice", "bonband").await;

    let c = client();
    login(&srv, &c, "alice", "pw").await;

    // Arriving in bonband is enough to make it the landing band.
    let r = c
        .get(format!("{}/bonband/", srv.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    let r = c.get(format!("{}/", srv.base)).send().await.unwrap();
    assert_eq!(r.status(), 303);
    assert_eq!(r.headers().get("location").unwrap(), "/bonband/");

    // And back again.
    c.get(format!("{}/default/", srv.base))
        .send()
        .await
        .unwrap();
    let r = c.get(format!("{}/", srv.base)).send().await.unwrap();
    assert_eq!(r.headers().get("location").unwrap(), "/default/");
}

/// Being removed from the band you were last in must not leave you
/// pointed at it. The landing query joins membership, so a stale
/// preference falls back to a band you can still see.
#[tokio::test]
async fn losing_your_last_band_lands_you_somewhere_you_can_still_see() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    add_to_band(&srv.pool, "alice", "bonband").await;

    let c = client();
    login(&srv, &c, "alice", "pw").await;
    c.get(format!("{}/bonband/", srv.base))
        .send()
        .await
        .unwrap();

    // Out of bonband, by the command line's own query.
    let band = iggybilly::queries::bands::find_by_slug(&srv.pool, "bonband")
        .await
        .unwrap()
        .unwrap();
    let user: (i64,) = sqlx::query_as("SELECT id FROM users WHERE username = 'alice'")
        .fetch_one(&srv.pool)
        .await
        .unwrap();
    iggybilly::queries::bands::remove_member(&srv.pool, band.id, user.0)
        .await
        .unwrap();

    let r = c.get(format!("{}/", srv.base)).send().await.unwrap();
    assert_eq!(r.status(), 303);
    assert_eq!(r.headers().get("location").unwrap(), "/default/");

    let r = c
        .get(format!("{}/bonband/", srv.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404, "and bonband is gone from view");
}

/// The account screen lists the bands you are in and which one you are
/// looking at, and switching is a post it makes before navigating.
#[tokio::test]
async fn the_account_screen_lists_your_bands_and_switches_between_them() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "pw").await;
    add_to_band(&srv.pool, "alice", "bonband").await;

    let c = client();
    login(&srv, &c, "alice", "pw").await;

    let props = get_props(&srv, &c, "/account").await;
    assert_eq!(
        props["bands"],
        serde_json::json!(["bonband", "default"]),
        "by slug"
    );

    let r = c
        .post(format!("{}/api/account/band", srv.base))
        .json(&serde_json::json!({ "band": "bonband" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);
    let props = get_props(&srv, &c, "/account").await;
    assert_eq!(props["band"], "bonband");

    // A band she is not in is not switchable to.
    let r = c
        .post(format!("{}/api/account/band", srv.base))
        .json(&serde_json::json!({ "band": "nosuchband" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);
}

/// A band posts to its own Discord channel when it has one, and to the
/// instance-wide webhook when it doesn't.
#[tokio::test]
async fn a_band_with_its_own_webhook_posts_there_instead() {
    let (instance_url, mut instance_rx) = mock_webhook().await;
    let (own_url, mut own_rx) = mock_webhook().await;
    let srv = start_with(
        Some(instance_url),
        Some("https://iggybilly.test".to_string()),
    )
    .await;
    create_user(&srv.pool, "alice", "pw").await;
    add_to_band(&srv.pool, "alice", "bonband").await;

    let band = iggybilly::queries::bands::find_by_slug(&srv.pool, "bonband")
        .await
        .unwrap()
        .unwrap();
    iggybilly::queries::bands::set_webhook(&srv.pool, band.id, Some(&own_url))
        .await
        .unwrap();

    let c = client();
    login(&srv, &c, "alice", "pw").await;

    upload_into(&srv, &c, "default", "Riff.mp3").await;
    let content = next_post(&mut instance_rx).await["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        content.contains("/default/clips/"),
        "the default band posts to the instance webhook, with its slug in the link: {content}"
    );

    upload_into(&srv, &c, "bonband", "Riff.mp3").await;
    let content = next_post(&mut own_rx).await["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        content.contains("/bonband/clips/"),
        "and bonband posts to its own: {content}"
    );
}
