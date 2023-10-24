use serde::{Deserialize, Serialize};
use surrealdb::engine::local::{Db, Mem};
use surrealdb::sql::Thing;
use surrealdb::Surreal;
use tokio::sync::OnceCell;

// #[derive(Debug, Deserialize, Serialize)]
// struct Event<'a> {
//     data: &'a str,
// }

#[derive(Debug, Deserialize, Serialize)]
pub struct Event {
    id_: u64,       // max 18_446_744_073_709_551_615
    stream_id: u16, // max 65_535
    version: u8,    // max 255
    data: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct Record {
    #[allow(dead_code)]
    id: Thing,
}

// TODO: use LazyLock, currently still "unstable"
// static DATABASE_CONNECTION: LazyLock<Mutex<Surreal<Db>>> = LazyLock::new(|| Mutex::new(initialize_in_mem_db()));
static DATABASE_CONNECTION: OnceCell<Surreal<Db>> = OnceCell::const_new();

async fn db() -> &'static Surreal<Db> {
    DATABASE_CONNECTION
        .get_or_init(|| async { initialize_in_mem_db().await.unwrap() })
        .await
}

async fn initialize_in_mem_db() -> surrealdb::Result<Surreal<Db>> {
    let db = Surreal::new::<Mem>(()).await?;
    db.use_ns("events").use_db("events").await?;
    dbg!("event store initialized");
    Ok(db)
}

pub async fn append_event(event: serde_json::Value) -> surrealdb::Result<()> {
    let created: Vec<Record> = db()
        .await
        .create("event")
        .content(Event {
            id_: 0,
            stream_id: 0,
            version: 0,
            data: event,
        })
        .await?;
    dbg!(created);
    Ok(())
}

pub async fn get_all() -> surrealdb::Result<Vec<Event>> {
    let events: Vec<Event> = db().await.select("event").await?;
    dbg!(events.len());
    Ok(events)
}
