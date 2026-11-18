//! What the migrations do to data that is already there.
//!
//! The ordinary tests start from an empty database, which is exactly the
//! case a migration cannot get wrong. This one seeds the schema as it
//! stood before a migration and asserts what survived it.

use sqlx::{sqlite::SqliteConnectOptions, Connection, SqliteConnection};

/// Apply the numbered migration files from `first` to `last` inclusive,
/// in order, each as its own script — the same order and the same
/// statements `sqlx::migrate!` would run, against a connection with
/// foreign keys on as `db::connect` sets them.
async fn apply_range(conn: &mut SqliteConnection, first: u32, last: u32) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("migrations directory")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "sql"))
        .collect();
    files.sort();

    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let number: u32 = name[..4].parse().expect("a numbered migration");
        if number < first {
            continue;
        }
        if number > last {
            break;
        }
        let sql = std::fs::read_to_string(&path).expect("readable migration");
        sqlx::raw_sql(&sql)
            .execute(&mut *conn)
            .await
            .unwrap_or_else(|e| panic!("applying {name}: {e}"));
    }
}

async fn connect() -> SqliteConnection {
    SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .in_memory(true)
            .foreign_keys(true),
    )
    .await
    .expect("in-memory sqlite")
}

async fn count(conn: &mut SqliteConnection, sql: &str) -> i64 {
    let row: (i64,) = sqlx::query_as(sql).fetch_one(&mut *conn).await.unwrap();
    row.0
}

/// 0008 rebuilds `labels` to make its name unique per band instead of
/// globally, and the two tables that reference a label have to be
/// rebuilt and dropped *before* it.
///
/// Otherwise `DROP TABLE labels` performs an implicit delete of every
/// row, which fires `ON DELETE CASCADE` and takes every label on every
/// clip and the whole wiki history with it. `PRAGMA defer_foreign_keys`
/// does not save you: it defers the check that a reference is satisfied,
/// not the cascade. The first version of this migration lost both, in
/// silence, and nothing in the schema afterwards looked wrong.
#[tokio::test]
async fn the_bands_migration_keeps_every_label_and_every_wiki_revision() {
    let mut conn = connect().await;
    apply_range(&mut conn, 1, 7).await;

    sqlx::raw_sql(
        "INSERT INTO users (username, password_hash) VALUES ('simon','x'),('jonas','y');
         INSERT INTO clips (name, storage_filename, original_filename, content_type,
                            size_bytes, uploaded_by)
         VALUES ('Take 3','a.mp3','Take 3.mp3','audio/mpeg',10,1),
                ('Riff','b.mp3','Riff.mp3','audio/mpeg',10,2);
         INSERT INTO labels (name) VALUES ('verse-1'),('blues');
         INSERT INTO clip_labels (clip_id, label_id, added_by, position)
         VALUES (1,1,1,0),(1,2,1,1024),(2,1,2,2048);
         INSERT INTO label_wiki_revisions (label_id, content, edited_by)
         VALUES (1,'# Verse one',1),(1,'# Verse one, again',2);",
    )
    .execute(&mut conn)
    .await
    .unwrap();

    apply_range(&mut conn, 8, 8).await;

    assert_eq!(
        count(&mut conn, "SELECT COUNT(*) FROM clip_labels").await,
        3,
        "every label on every clip survived the rebuild"
    );
    assert_eq!(
        count(&mut conn, "SELECT COUNT(*) FROM label_wiki_revisions").await,
        2,
        "and the whole wiki history"
    );
    assert_eq!(count(&mut conn, "SELECT COUNT(*) FROM labels").await, 2);

    // Ids are preserved, so every link ever sent still resolves and the
    // rows copied back into the children point at the label they were on.
    let row: (i64, String, i64) =
        sqlx::query_as("SELECT id, name, band_id FROM labels ORDER BY id LIMIT 1")
            .fetch_one(&mut conn)
            .await
            .unwrap();
    assert_eq!((row.0, row.1.as_str()), (1, "verse-1"));

    // The playlist order came across as it stood.
    let positions: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT clip_id, position FROM clip_labels WHERE label_id = 1 ORDER BY position",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    assert_eq!(positions, vec![(1, 0), (2, 2048)]);

    // Nothing dangles, and the rebuilt foreign keys still cascade.
    let dangling = count(
        &mut conn,
        "SELECT COUNT(*) FROM clip_labels WHERE label_id NOT IN (SELECT id FROM labels)",
    )
    .await;
    assert_eq!(dangling, 0);

    sqlx::raw_sql("DELETE FROM labels WHERE id = 1")
        .execute(&mut conn)
        .await
        .unwrap();
    assert_eq!(
        count(&mut conn, "SELECT COUNT(*) FROM label_wiki_revisions").await,
        0,
        "the rebuilt cascade still works"
    );
}

/// Everything that existed lands in `default`, so the day it ships
/// nothing moves.
#[tokio::test]
async fn the_bands_migration_puts_everything_that_existed_in_one_band() {
    let mut conn = connect().await;
    apply_range(&mut conn, 1, 7).await;
    sqlx::raw_sql(
        "INSERT INTO users (username, password_hash) VALUES ('simon','x'),('jonas','y');
         INSERT INTO clips (name, storage_filename, original_filename, content_type,
                            size_bytes, uploaded_by)
         VALUES ('Take 3','a.mp3','Take 3.mp3','audio/mpeg',10,1);
         INSERT INTO labels (name) VALUES ('verse-1');",
    )
    .execute(&mut conn)
    .await
    .unwrap();

    apply_range(&mut conn, 8, 8).await;

    let band: (i64, String) = sqlx::query_as("SELECT id, slug FROM bands")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(band.1, "default");

    assert_eq!(
        count(&mut conn, "SELECT COUNT(*) FROM band_members").await,
        2,
        "both users are members"
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) FROM users WHERE last_band_id IS NULL"
        )
        .await,
        0,
        "and both land there"
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) FROM clips WHERE band_id IS NULL"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &mut conn,
            "SELECT COUNT(*) FROM labels WHERE band_id IS NULL"
        )
        .await,
        0
    );

    // A second band may reuse both names; neither is unique any more
    // except within a band.
    sqlx::raw_sql(
        "INSERT INTO bands (slug) VALUES ('bonband');
         INSERT INTO labels (name, band_id) VALUES ('verse-1', 2);
         INSERT INTO clips (name, storage_filename, original_filename, content_type,
                            size_bytes, uploaded_by, band_id)
         VALUES ('Take 3','c.mp3','Take 3.mp3','audio/mpeg',10,1,2);",
    )
    .execute(&mut conn)
    .await
    .expect("a second band may reuse the same names");

    // And within one band they are still unique.
    for sql in [
        "INSERT INTO labels (name, band_id) VALUES ('VERSE-1', 1)",
        "INSERT INTO clips (name, storage_filename, original_filename, content_type,
                            size_bytes, uploaded_by, band_id)
         VALUES ('take 3','d.mp3','d.mp3','audio/mpeg',10,1,1)",
    ] {
        assert!(
            sqlx::raw_sql(sql).execute(&mut conn).await.is_err(),
            "a name is still unique within its band, case-insensitively"
        );
    }
}
