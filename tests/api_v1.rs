//! Integration tests for `/api/v1`, the surface the mobile app uses.
//!
//! These drive the app's real path: sign in for a bearer token, then do
//! everything else with that token and no cookie jar at all. The client
//! here deliberately does *not* keep cookies, so a test that passes
//! cannot be passing because a session was quietly carrying it.

use std::sync::Arc;

use tokio::net::TcpListener;

struct Server {
    base: String,
    pool: sqlx::SqlitePool,
    _temp: tempfile::TempDir,
    _handle: tokio::task::JoinHandle<()>,
}

async fn start() -> Server {
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
        static_dir: data_dir.join("static"),
        secure_cookies: false,
        discord_webhook_url: None,
        base_url: None,
    };

    let pool = iggybilly::db::connect(&db_path).await.unwrap();
    let app = iggybilly::web::build_app(pool.clone(), Arc::new(config))
        .await
        .unwrap();

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

/// A client with no cookie store, so nothing but the bearer token can
/// authenticate these requests.
fn client() -> reqwest::Client {
    reqwest::Client::builder()
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

/// Sign in and return the bearer token.
async fn token_for(srv: &Server, c: &reqwest::Client, user: &str, pass: &str) -> String {
    let r = c
        .post(format!("{}/api/v1/tokens", srv.base))
        .json(&serde_json::json!({
            "username": user,
            "password": pass,
            "deviceName": "Test phone",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "sign-in should issue a token");
    let body: serde_json::Value = r.json().await.unwrap();
    body["token"].as_str().expect("a token").to_string()
}

/// Set up a user with a token, ready to use.
async fn signed_in(srv: &Server) -> (reqwest::Client, String) {
    create_user(&srv.pool, "alice", "passw0rd!").await;
    let c = client();
    let token = token_for(srv, &c, "alice", "passw0rd!").await;
    (c, token)
}

async fn get(
    srv: &Server,
    c: &reqwest::Client,
    token: &str,
    path: &str,
) -> (reqwest::StatusCode, serde_json::Value) {
    let r = c
        .get(format!("{}{path}", srv.base))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    let status = r.status();
    // A 204 has no body; give callers a null rather than a parse error.
    let body = r.text().await.unwrap();
    let json = if body.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(&body).unwrap_or(serde_json::Value::Null)
    };
    (status, json)
}

async fn upload(srv: &Server, c: &reqwest::Client, token: &str, filenames: &[&str]) -> Vec<i64> {
    let mut form = reqwest::multipart::Form::new();
    for name in filenames {
        let part = reqwest::multipart::Part::bytes(b"fake-audio-bytes".to_vec())
            .file_name(name.to_string())
            .mime_str("audio/mpeg")
            .unwrap();
        form = form.part("audio", part);
    }
    let r = c
        .post(format!("{}/api/v1/clips", srv.base))
        .bearer_auth(token)
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "upload should succeed");
    let body: serde_json::Value = r.json().await.unwrap();
    body["clips"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_i64().unwrap())
        .collect()
}

#[tokio::test]
async fn a_token_authenticates_without_any_cookie() {
    let srv = start().await;
    let (c, token) = signed_in(&srv).await;

    let (status, me) = get(&srv, &c, &token, "/api/v1/me").await;
    assert_eq!(status, 200);
    assert_eq!(me["username"], "alice");
    assert_eq!(me["isAdmin"], false);
}

#[tokio::test]
async fn requests_without_a_token_are_rejected() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "passw0rd!").await;
    let c = client();

    let r = c
        .get(format!("{}/api/v1/clips", srv.base))
        .send()
        .await
        .unwrap();
    assert_eq!(
        r.status(),
        401,
        "no credential at all is a 401, not a redirect"
    );

    let r = c
        .get(format!("{}/api/v1/clips", srv.base))
        .bearer_auth("not-a-real-token")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401, "a bogus token is a 401");
}

#[tokio::test]
async fn wrong_password_issues_no_token() {
    let srv = start().await;
    create_user(&srv.pool, "alice", "passw0rd!").await;
    let r = client()
        .post(format!("{}/api/v1/tokens", srv.base))
        .json(&serde_json::json!({
            "username": "alice",
            "password": "wrong",
            "deviceName": "Test phone",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
}

#[tokio::test]
async fn clips_carry_raw_timestamps_and_their_own_urls() {
    let srv = start().await;
    let (c, token) = signed_in(&srv).await;
    let ids = upload(&srv, &c, &token, &["riff.mp3"]).await;
    let id = ids[0];

    let (status, clips) = get(&srv, &c, &token, "/api/v1/clips").await;
    assert_eq!(status, 200);
    let clip = &clips.as_array().unwrap()[0];

    assert_eq!(clip["name"], "riff");
    assert_eq!(clip["uploader"], "alice");
    assert_eq!(clip["audioUrl"], format!("/clips/{id}/audio"));
    assert_eq!(clip["downloadUrl"], format!("/clips/{id}/audio?download=1"));
    // The uploader may delete their own clip, and the list says so
    // rather than making the client re-derive the rule.
    assert_eq!(clip["canDelete"], true);
    // The timestamp is the stored instant, not a formatted date: it
    // carries a time and a zone for the device to render.
    let uploaded_at = clip["uploadedAt"].as_str().unwrap();
    assert!(
        uploaded_at.contains('T') && uploaded_at.ends_with('Z'),
        "expected an RFC 3339 instant, got {uploaded_at}"
    );
}

#[tokio::test]
async fn another_users_clip_is_not_deletable() {
    let srv = start().await;
    let (alice, alice_token) = signed_in(&srv).await;
    let ids = upload(&srv, &alice, &alice_token, &["riff.mp3"]).await;

    create_user(&srv.pool, "bob", "passw0rd!").await;
    let bob = client();
    let bob_token = token_for(&srv, &bob, "bob", "passw0rd!").await;

    let (_, clip) = get(&srv, &bob, &bob_token, &format!("/api/v1/clips/{}", ids[0])).await;
    assert_eq!(clip["canDelete"], false);

    let r = bob
        .delete(format!("{}/api/v1/clips/{}", srv.base, ids[0]))
        .bearer_auth(&bob_token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403, "only the uploader may delete");
}

#[tokio::test]
async fn labels_filter_the_list_with_and_semantics() {
    let srv = start().await;
    let (c, token) = signed_in(&srv).await;
    let ids = upload(&srv, &c, &token, &["one.mp3", "two.mp3"]).await;

    for (id, names) in [(ids[0], vec!["verse", "live"]), (ids[1], vec!["verse"])] {
        for name in names {
            let r = c
                .post(format!("{}/api/v1/clips/{id}/labels", srv.base))
                .bearer_auth(&token)
                .json(&serde_json::json!({ "name": name }))
                .send()
                .await
                .unwrap();
            assert_eq!(r.status(), 200);
        }
    }

    let (_, both) = get(&srv, &c, &token, "/api/v1/clips?label=verse").await;
    assert_eq!(both.as_array().unwrap().len(), 2);

    let (_, narrowed) = get(&srv, &c, &token, "/api/v1/clips?label=verse&label=live").await;
    let narrowed = narrowed.as_array().unwrap();
    assert_eq!(narrowed.len(), 1, "both labels must match, not either");
    assert_eq!(narrowed[0]["id"], ids[0]);
}

#[tokio::test]
async fn a_label_must_be_lower_kebab_case() {
    let srv = start().await;
    let (c, token) = signed_in(&srv).await;
    let ids = upload(&srv, &c, &token, &["one.mp3"]).await;

    // Mixed case is normalised rather than rejected.
    let r = c
        .post(format!("{}/api/v1/clips/{}/labels", srv.base, ids[0]))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "name": "Verse-1" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let labels: serde_json::Value = r.json().await.unwrap();
    assert_eq!(labels.as_array().unwrap()[0]["name"], "verse-1");

    // A space is not.
    let r = c
        .post(format!("{}/api/v1/clips/{}/labels", srv.base, ids[0]))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "name": "verse 1" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
}

#[tokio::test]
async fn wiki_pages_travel_as_markdown_not_html() {
    let srv = start().await;
    let (c, token) = signed_in(&srv).await;
    let ids = upload(&srv, &c, &token, &["one.mp3"]).await;
    let r = c
        .post(format!("{}/api/v1/clips/{}/labels", srv.base, ids[0]))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "name": "verse" }))
        .send()
        .await
        .unwrap();
    let labels: serde_json::Value = r.json().await.unwrap();
    let label_id = labels.as_array().unwrap()[0]["id"].as_i64().unwrap();

    // An unwritten page is an empty page, not a 404.
    let (status, page) = get(&srv, &c, &token, &format!("/api/v1/labels/{label_id}/wiki")).await;
    assert_eq!(status, 200);
    assert_eq!(page["hasContent"], false);
    assert_eq!(page["content"], "");

    let r = c
        .post(format!("{}/api/v1/labels/{label_id}/wiki", srv.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "content": "# Verse\n\nTwo bars of *nothing*." }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let page: serde_json::Value = r.json().await.unwrap();
    assert_eq!(page["hasContent"], true);
    // The source survives intact — no rendering happened on the way.
    assert_eq!(page["content"], "# Verse\n\nTwo bars of *nothing*.");
    assert_eq!(page["lastEditedBy"], "alice");

    let (_, history) = get(
        &srv,
        &c,
        &token,
        &format!("/api/v1/labels/{label_id}/wiki/history"),
    )
    .await;
    let history = history.as_array().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0]["isCurrent"], true);
    assert_eq!(history[0]["content"], "# Verse\n\nTwo bars of *nothing*.");
}

#[tokio::test]
async fn restoring_a_revision_appends_rather_than_rewinds() {
    let srv = start().await;
    let (c, token) = signed_in(&srv).await;
    let ids = upload(&srv, &c, &token, &["one.mp3"]).await;
    let r = c
        .post(format!("{}/api/v1/clips/{}/labels", srv.base, ids[0]))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "name": "verse" }))
        .send()
        .await
        .unwrap();
    let labels: serde_json::Value = r.json().await.unwrap();
    let label_id = labels.as_array().unwrap()[0]["id"].as_i64().unwrap();

    for content in ["first", "second"] {
        let r = c
            .post(format!("{}/api/v1/labels/{label_id}/wiki", srv.base))
            .bearer_auth(&token)
            .json(&serde_json::json!({ "content": content }))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
    }

    let (_, history) = get(
        &srv,
        &c,
        &token,
        &format!("/api/v1/labels/{label_id}/wiki/history"),
    )
    .await;
    let oldest = history.as_array().unwrap().last().unwrap()["id"]
        .as_i64()
        .unwrap();

    let r = c
        .post(format!(
            "{}/api/v1/labels/{label_id}/wiki/restore/{oldest}",
            srv.base
        ))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);

    let (_, page) = get(&srv, &c, &token, &format!("/api/v1/labels/{label_id}/wiki")).await;
    assert_eq!(page["content"], "first");

    let (_, history) = get(
        &srv,
        &c,
        &token,
        &format!("/api/v1/labels/{label_id}/wiki/history"),
    )
    .await;
    assert_eq!(
        history.as_array().unwrap().len(),
        3,
        "restoring adds a revision; it doesn't delete the ones after it"
    );
}

#[tokio::test]
async fn audio_is_served_to_a_bearer_token() {
    let srv = start().await;
    let (c, token) = signed_in(&srv).await;
    let ids = upload(&srv, &c, &token, &["riff.mp3"]).await;

    let r = c
        .get(format!("{}/clips/{}/audio", srv.base, ids[0]))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "audio/mpeg");
    assert_eq!(r.bytes().await.unwrap().as_ref(), b"fake-audio-bytes");

    // And refused without one, rather than redirected: a player that
    // followed a redirect would try to decode the login page.
    let r = client()
        .get(format!("{}/clips/{}/audio", srv.base, ids[0]))
        .bearer_auth("not-a-real-token")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
}

#[tokio::test]
async fn audio_answers_range_requests() {
    // AVPlayer on iOS will not play a remote asset from a server that
    // ignores Range — it fails with "(-11850) Operation Stopped" before
    // playback ever starts. A browser's <audio> tolerates a plain 200, so
    // this breaks the app and nothing else, which is why it is asserted
    // here rather than left to be noticed on a device.
    let srv = start().await;
    let (c, token) = signed_in(&srv).await;
    let ids = upload(&srv, &c, &token, &["riff.mp3"]).await;
    let url = format!("{}/clips/{}/audio", srv.base, ids[0]);

    let full = c.get(&url).bearer_auth(&token).send().await.unwrap();
    assert_eq!(full.status(), 200);
    assert_eq!(
        full.headers()["accept-ranges"],
        "bytes",
        "the route must advertise that it takes ranges"
    );

    let part = c
        .get(&url)
        .bearer_auth(&token)
        .header("range", "bytes=0-3")
        .send()
        .await
        .unwrap();
    assert_eq!(part.status(), 206, "a Range request gets partial content");
    assert_eq!(part.headers()["content-range"], "bytes 0-3/16");
    assert_eq!(part.bytes().await.unwrap().as_ref(), b"fake");

    // A range past the end is refused, not silently clamped.
    let past = c
        .get(&url)
        .bearer_auth(&token)
        .header("range", "bytes=900-999")
        .send()
        .await
        .unwrap();
    assert_eq!(past.status(), 416);
}

#[tokio::test]
async fn devices_can_be_listed_and_revoked_one_at_a_time() {
    let srv = start().await;
    let (phone, phone_token) = signed_in(&srv).await;
    let tablet = client();
    let tablet_token = token_for(&srv, &tablet, "alice", "passw0rd!").await;

    let (_, devices) = get(&srv, &phone, &phone_token, "/api/v1/tokens").await;
    let devices = devices.as_array().unwrap();
    assert_eq!(devices.len(), 2);
    assert_eq!(
        devices.iter().filter(|d| d["isCurrent"] == true).count(),
        1,
        "exactly one device is the caller's own"
    );

    let tablet_id = devices.iter().find(|d| d["isCurrent"] == false).unwrap()["id"]
        .as_i64()
        .unwrap();

    let r = phone
        .delete(format!("{}/api/v1/tokens/{tablet_id}", srv.base))
        .bearer_auth(&phone_token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);

    // The revoked device is out, the one that did the revoking is not.
    let (status, _) = get(&srv, &tablet, &tablet_token, "/api/v1/me").await;
    assert_eq!(status, 401);
    let (status, _) = get(&srv, &phone, &phone_token, "/api/v1/me").await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn one_user_cannot_revoke_anothers_device() {
    let srv = start().await;
    let (alice, alice_token) = signed_in(&srv).await;
    create_user(&srv.pool, "bob", "passw0rd!").await;
    let bob = client();
    let bob_token = token_for(&srv, &bob, "bob", "passw0rd!").await;

    let (_, devices) = get(&srv, &alice, &alice_token, "/api/v1/tokens").await;
    let alice_device = devices.as_array().unwrap()[0]["id"].as_i64().unwrap();

    let r = bob
        .delete(format!("{}/api/v1/tokens/{alice_device}", srv.base))
        .bearer_auth(&bob_token)
        .send()
        .await
        .unwrap();
    assert_eq!(
        r.status(),
        404,
        "someone else's token id is simply not found"
    );

    let (status, _) = get(&srv, &alice, &alice_token, "/api/v1/me").await;
    assert_eq!(status, 200, "alice is still signed in");
}

#[tokio::test]
async fn changing_the_password_cuts_off_every_other_device() {
    let srv = start().await;
    let (phone, phone_token) = signed_in(&srv).await;
    let lost = client();
    let lost_token = token_for(&srv, &lost, "alice", "passw0rd!").await;

    let r = phone
        .post(format!("{}/api/v1/me/password", srv.base))
        .bearer_auth(&phone_token)
        .json(&serde_json::json!({
            "currentPassword": "passw0rd!",
            "newPassword": "a-much-longer-one",
            "deviceName": "Test phone",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let body: serde_json::Value = r.json().await.unwrap();
    let replacement = body["token"].as_str().unwrap();

    // The device that changed the password is handed a new token rather
    // than being signed out by its own request.
    let (status, _) = get(&srv, &phone, replacement, "/api/v1/me").await;
    assert_eq!(status, 200);

    // Everything else is out, including the token that made the call.
    let (status, _) = get(&srv, &lost, &lost_token, "/api/v1/me").await;
    assert_eq!(status, 401);
    let (status, _) = get(&srv, &phone, &phone_token, "/api/v1/me").await;
    assert_eq!(status, 401);
}

#[tokio::test]
async fn signing_out_revokes_only_this_device() {
    let srv = start().await;
    let (phone, phone_token) = signed_in(&srv).await;
    let tablet = client();
    let tablet_token = token_for(&srv, &tablet, "alice", "passw0rd!").await;

    let r = phone
        .delete(format!("{}/api/v1/tokens/current", srv.base))
        .bearer_auth(&phone_token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);

    let (status, _) = get(&srv, &phone, &phone_token, "/api/v1/me").await;
    assert_eq!(status, 401);
    let (status, _) = get(&srv, &tablet, &tablet_token, "/api/v1/me").await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn renaming_reports_a_conflict_rather_than_overwriting() {
    let srv = start().await;
    let (c, token) = signed_in(&srv).await;
    let ids = upload(&srv, &c, &token, &["one.mp3", "two.mp3"]).await;

    let r = c
        .post(format!("{}/api/v1/clips/{}/name", srv.base, ids[1]))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "name": "one" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);

    let r = c
        .post(format!("{}/api/v1/clips/{}/name", srv.base, ids[1]))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "name": "  the other one  " }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["name"], "the other one", "the name is trimmed");
}

#[tokio::test]
async fn label_search_offers_creation_only_for_a_valid_new_name() {
    let srv = start().await;
    let (c, token) = signed_in(&srv).await;
    let ids = upload(&srv, &c, &token, &["one.mp3"]).await;
    let r = c
        .post(format!("{}/api/v1/clips/{}/labels", srv.base, ids[0]))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "name": "verse" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    let (_, s) = get(&srv, &c, &token, "/api/v1/labels/search?q=vers").await;
    assert_eq!(s["matches"].as_array().unwrap().len(), 1);
    assert_eq!(s["canCreate"], true, "'vers' is valid and not yet taken");

    let (_, s) = get(&srv, &c, &token, "/api/v1/labels/search?q=verse").await;
    assert_eq!(s["canCreate"], false, "'verse' already exists");

    let (_, s) = get(&srv, &c, &token, "/api/v1/labels/search?q=not%20valid").await;
    assert_eq!(s["canCreate"], false, "a name with a space is not offered");
}
