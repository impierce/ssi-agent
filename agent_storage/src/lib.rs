use serde::{Deserialize, Serialize};
use surrealdb::engine::local::{Db, Mem};
use surrealdb::sql::Thing;
use surrealdb::Surreal;
use tokio::sync::OnceCell;

// #[derive(Debug, Deserialize, Serialize)]
// struct Event<'a> {
//     data: &'a str,
// }

/// Write model
#[derive(Debug, Serialize, Deserialize)]
pub struct EventWriteModel {
    // sequence_number: u64,
    stream_id: u16, // max 65_535
    version: u8,    // max 255
    data: serde_json::Value,
}

/// Read model
#[derive(Debug, Serialize, Deserialize)]
pub struct EventReadModel {
    #[serde(serialize_with = "serialize_id")]
    pub id: Thing,
    pub stream_id: u16,
    pub version: u8,
    pub data: serde_json::Value,
}

fn serialize_id<S>(id: &Thing, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&id.id.to_string())
}

#[derive(Debug, Deserialize)]
struct Record {
    #[allow(dead_code)]
    id: Thing,
}

type Id = String;

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

    // db.query("DEFINE TABLE event SCHEMAFULL").await?;
    // db.query("DEFINE FIELD id_ ON TABLE event TYPE number").await?;
    // db.query("DEFINE FIELD stream_id ON TABLE event TYPE number").await?;
    // db.query("DEFINE FIELD version ON TABLE event TYPE number").await?;
    // db.query("DEFINE FIELD data ON TABLE event TYPE object").await?;

    // // TODO: make id_ serial (auto increment)
    // // db.query("DEFINE INDEX id_ ON TABLE event COLUMNS id_ UNIQUE").await?;
    // // combination of stream_id and version must be unique
    // db.query("DEFINE INDEX stream_id ON TABLE event COLUMNS stream_id, version UNIQUE")
    //     .await?;

    dbg!("event store initialized");
    Ok(db)
}

// TODO: add aggregate_id to fn signature
pub async fn append_event(event: serde_json::Value) -> surrealdb::Result<Id> {
    // Find the latest version in an (aggregate) stream
    let mut response = db()
        .await
        .query("RETURN math::max(SELECT VALUE version FROM event);")
        .await?;
    let max_version: Option<u32> = response.take(0)?;
    dbg!(max_version);

    let created: Vec<Record> = db()
        .await
        .create("event") // rename to "credential_event"?
        .content(EventWriteModel {
            stream_id: 0, // TODO: pass in from outside? == aggregate id?
            version: (max_version.unwrap_or_default() + 1) as u8,
            data: event,
        })
        .await?;
    Ok(created.first().unwrap().id.id.to_string()) // id of type "Thing": {tb: "event", id: 123}
}

pub async fn get_all() -> surrealdb::Result<Vec<EventReadModel>> {
    let events: Vec<EventReadModel> = db().await.select("event").await?;
    dbg!(events.len());
    Ok(events)
}
