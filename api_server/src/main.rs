use std::env;
use warp::Filter;
use sqlx::postgres::PgPool;

/// Provides a RESTful web server managing some Todos.
/// 
/// API will be:
///
/// - `GET /todos`: return a JSON list of Todos.
/// - `POST /todos`: create a new Todo.
/// - `PUT /todos/:id`: update a specific Todo.
/// - `DELETE /todos/:id`: delete a specific Todo.
#[tokio::main]
async fn main() {
    if env::var_os("RUST_LOG").is_none() {
        // Set `RUST_LOG=todos=debug` to see debug logs,
        // this only shows access logs.
        env::set_var("RUST_LOG", "INFO");
    }
    pretty_env_logger::init();
    let db_host = &env::var("SQL_SERVER").expect("error reading db env");
    let pool = PgPool::connect(&format!("postgres://postgres:example_password@{}/postgres", db_host)).await.expect("error connecting to postgres");

    let _db = models::blank_db();
    // can use a migration file for this
    // to avoid running this every time
    sqlx::query!(
        "
        CREATE TABLE IF NOT EXISTS todos
        (
            id          INTEGER PRIMARY KEY NOT NULL,
            text TEXT                NOT NULL,
            completed        BOOLEAN             NOT NULL DEFAULT FALSE
        );"
    ).execute(&pool).await.unwrap();
    let pgdb = models::postgres_db(pool);

    let api = filters::todos(pgdb, cache::Cache::redis_cache());

    // View access logs by setting `RUST_LOG=todos`.
    let routes = api.with(warp::log("todos"));
    // Start up the server...
    warp::serve(routes).run(([0, 0, 0, 0], 3030)).await;
}

mod filters {
    use super::handlers;
    use super::models::{Db, ListOptions, Todo};
    use super::cache::Cache;
    use warp::Filter;
    
    /// The 4 TODOs filters combined.
    pub fn todos(
        db: Db,
        cache:Cache,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        todos_list(db.clone())
            .or(todos_create(db.clone(), cache.clone()))
            .or(todos_get(db.clone(), cache.clone()))
            .or(todos_update(db, cache))
    }

    /// GET /todos?offset=3&limit=5
    pub fn todos_list(
        db: Db,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("todos")
            .and(warp::get())
            .and(warp::query::<ListOptions>())
            .and(with_db(db))
            .and_then(handlers::list_todos)
    }

    pub fn todos_get(
        db: Db,
        cache: Cache
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("todo"/ i32)
            .and(warp::get())
            .and(with_db(db))
            .and(with_cache(cache))
            .and_then(handlers::get_todo)
    }

    /// POST /todos with JSON body
    pub fn todos_create(
        db: Db,
        cache: Cache
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("todos")
            .and(warp::post())
            .and(json_body())
            .and(with_db(db))
            .and(with_cache(cache))
            .and_then(handlers::create_todo)
    }

    /// PUT /todos/:id with JSON body
    pub fn todos_update(
        db: Db,
        cache: Cache
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("complete" / i32)
            .and(warp::post())
            .and(with_db(db))
            .and(with_cache(cache))
            .and_then(handlers::update_todo)
    }

    fn with_db(db: Db) -> impl Filter<Extract = (Db, ), Error = std::convert::Infallible> + Clone {
        warp::any().map(move || db.clone())
    }

    fn with_cache(cache: Cache) -> impl Filter<Extract = (Cache, ), Error = std::convert::Infallible> + Clone {
        warp::any().map(move || cache.clone())
    }

    fn json_body() -> impl Filter<Extract = (Todo,), Error = warp::Rejection> + Clone {
        // When accepting a body, we want a JSON body
        // (and to reject huge payloads)...
        warp::body::content_length_limit(1024 * 16).and(warp::body::json())
    }
}

/// These are our API handlers, the ends of each filter chain.
/// Notice how thanks to using `Filter::and`, we can define a function
/// with the exact arguments we'd expect from each filter in the chain.
/// No tuples are needed, it's auto flattened for the functions.
mod handlers {
    use super::models::{Db, ListOptions, Todo};
    use super::cache::Cache;
    use std::convert::Infallible;
    use warp::http::StatusCode;
    use sqlx::{Pool, Postgres};
    
    async fn add_db_todo(pool: Pool<Postgres>, description: String, id: i32) -> Result<i32, sqlx::Error> {
    
        // Insert the task, then obtain the ID of this row
        let id = sqlx::query!(
            r#"
    INSERT INTO todos ( text, id )
    VALUES ( $1, $2 )
    RETURNING id
            "#,
            description,
            id
        ).fetch_one(&pool)
        .await?;
    
        Ok(id.id)
    }

    async fn list_db_todos(pool: Pool<Postgres>) -> Result<Vec<Todo>, sqlx::Error> {
        let recs = sqlx::query!(
            r#"
    SELECT id, text, completed
    FROM todos
    ORDER BY id
            "#
        )
        .fetch_all(&pool)
        .await?;
    
        let todos = recs.into_iter().map(|rec| Todo{
            id: rec.id,
            text: rec.text,
            completed: rec.completed
        }).collect();
    
        Ok(todos)
    }

    async fn get_db_todo(pool: Pool<Postgres>, id:i32) -> Result<Todo, sqlx::Error> {
        let rec = sqlx::query!(
            r#"
    SELECT id, text, completed
    FROM todos
    WHERE id = $1
            "#,
            id
        )
        .fetch_one(&pool)
        .await?;
    
        Ok(Todo {
            id: rec.id,
            text: rec.text,
            completed: rec.completed
        })
    }

    async fn complete_todo(pool: Pool<Postgres>, id: i32) -> Result<bool, sqlx::Error> {
        let rows_affected = sqlx::query!(
            r#"
    UPDATE todos
    SET completed = TRUE
    WHERE id = $1
            "#,
            id
        )
        .execute(&pool)
        .await?
        .rows_affected();
    
        Ok(rows_affected > 0)
    }

    pub async fn list_todos(opts: ListOptions, database: Db) -> Result<impl warp::Reply, Infallible> {
        // Just return a JSON array of todos, applying the limit and offset.
        match database {
            Db::InMemory(db) => {
                let todos = db.lock().await;
                let todos: Vec<Todo> = todos
                    .clone()
                    .into_iter()
                    .skip(opts.offset.unwrap_or(0))
                    .take(opts.limit.unwrap_or(std::usize::MAX))
                    .collect();
                Ok(warp::reply::json(&todos))
            },
            Db::Postgres(pool) => {
                match list_db_todos(pool).await {
                    Ok(todos) => Ok(warp::reply::json(&todos)),
                    _ => Ok(warp::reply::json(&String::from("Internal Server Error")))
                }
            }
        }
    }

    pub async fn create_todo(create: Todo, database: Db, cache: Cache) -> Result<impl warp::Reply, Infallible> {
        log::debug!("create_todo: {:?}", create);
        match database {
            Db::InMemory(db) => {

                let mut vec = db.lock().await;
        
                for todo in vec.iter() {
                    if todo.id == create.id {
                        log::debug!("    -> id already exists: {}", create.id);
                        // Todo with id already exists, return `400 BadRequest`.
                        return Ok(StatusCode::BAD_REQUEST);
                    }
                }
        
                // No existing Todo with id, so insert and return `201 Created`.
                cache.invalidate_todo(create.id).await;
                vec.push(create);
        
                Ok(StatusCode::CREATED)
            },
            Db::Postgres(pool) => {
                match add_db_todo(pool, create.text, create.id).await {
                    Ok(_) => {
                        Ok(StatusCode::CREATED)
                    }
                    _ => {
                        Ok(StatusCode::BAD_REQUEST)
                    }
                }
            }
        }
    }

    pub async fn update_todo(
        id: i32,
        database: Db,
        cache: Cache
    ) -> Result<impl warp::Reply, Infallible> {
        log::debug!("update_todo: id={}", id);
        match database {
            Db::InMemory(db) => {
                let mut vec = db.lock().await;
        
                // Look for the specified Todo...
                for todo in vec.iter_mut() {
                    if todo.id == id {
                        todo.completed = true;
                        cache.invalidate_todo(id).await;
                        return Ok(StatusCode::OK);
                    }
                }
        
                log::debug!("    -> todo id not found!");
        
                // If the for loop didn't return OK, then the ID doesn't exist...
                Ok(StatusCode::NOT_FOUND)
            },
            Db::Postgres(pool) => {
                match complete_todo(pool, id).await {
                    Ok(true) => {                    
                        cache.invalidate_todo(id).await;
                        Ok(StatusCode::OK)
                    },
                    Ok(false) => Ok(StatusCode::NOT_FOUND),
                    Err(_) => Ok(StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
        }
    }
    
    pub async fn get_todo(
        id: i32,
        database: Db,
        cache: Cache
    ) -> Result<impl warp::Reply, Infallible> {
        log::debug!("update_todo: id={}", id);
        if let Some(todo) = cache.get_todo(id).await {
            log::debug!("Got cached TODO");
            return Ok(warp::reply::json(&todo));
        }
        match database {
            Db::InMemory(db) => {
                let vec = db.lock().await;
        
                // Look for the specified Todo...
                for todo in vec.iter() {
                    if todo.id == id {
                        cache.add_todo(todo.clone()).await;
                        return Ok(warp::reply::json(todo));
                    }
                }
        
                log::debug!("-> todo id not found!");
        
                // If the for loop didn't return OK, then the ID doesn't exist...
                Ok(warp::reply::json(&"{}".to_owned()))
            },
            Db::Postgres(pool) => {
                match get_db_todo(pool, id).await {
                    Ok(todo) => {
                        cache.add_todo(todo.clone()).await;
                        Ok(warp::reply::json(&todo))
                    },
                    Err(sqlx::Error::RowNotFound) => Ok(warp::reply::json(&"NOT FOUND".to_owned())),
                    Err(_) => Ok(warp::reply::json(&"INTERNAL SERVER ERROR".to_owned()))
                }
            }
        }
    }

}


mod models {
    use serde_derive::{Deserialize, Serialize};
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use sqlx::{Pool, Postgres};
    use redis_derive::{FromRedisValue, ToRedisArgs};


    /// So we don't have to tackle how different database work, we'll just use
    /// a simple in-memory DB, a vector synchronized by a mutex.
    #[derive(Clone)]
    pub enum Db{
        InMemory(Arc<Mutex<Vec<Todo>>>),
        Postgres(Pool<Postgres>)
    }

    pub fn blank_db() -> Db {
        Db::InMemory(Arc::new(Mutex::new(Vec::new())))
    }

    pub fn postgres_db(pool: Pool<Postgres>) -> Db {
        Db::Postgres(pool)
    }

    #[derive(Debug, Deserialize, Serialize, Clone)]
    pub struct Todo {
        pub id: i32,
        pub text: String,
        pub completed: bool,
    }

    // The query parameters for list_todos.
    #[derive(Debug, Deserialize)]
    pub struct ListOptions {
        pub offset: Option<usize>,
        pub limit: Option<usize>,
    }
}

mod cache {
    use crate::models::Todo;
    use redis::{Client, AsyncCommands};
    use std::sync::Arc;
    use std::env;
    use serde_json;


    #[derive(Clone)]
    pub enum Cache {
        Redis(Arc<Client>),
        None
    }

    impl Cache {
        pub fn blank() -> Self {
            Cache::None
        }

        pub fn redis_cache() -> Self {
            let redis_host = &env::var("REDIS").expect("error reading redis env");
            let client = Client::open(format!("redis://{}/", redis_host)).unwrap();
            Cache::Redis(Arc::new(client))
        }

        pub async fn get_todo(&self, id: i32) -> Option<Todo> {
            match self {
                Cache::Redis(client) => {
                    if let Ok(mut conn) = client.get_tokio_connection().await {
                        let key = format!("todo_{}", id);
                        match conn.get::<String, String>(key).await {
                            Ok(val) => serde_json::from_str(&val).ok(),
                            Err(e) => {
                                log::debug!("redis::fetch_todo failed: {:?}", e);
                                None
                            },
                        }
                        
                    } else {
                        log::debug!("redis::couldn't make a connection:");
                        None
                    }
                },
                _ => None
            }
        }

        pub async fn add_todo(&self, todo: Todo) {
            match self {
                Cache::Redis(client) => {
                    if let Ok(mut conn) = client.get_tokio_connection().await {
                        let key = format!("todo_{}", todo.id);
                        if let Err(e) = conn.set::<String, String, bool>(key, serde_json::to_string(&todo).expect("Serialization failed")).await {
                            log::debug!("redis::create_todo failed: {:?}", e);
                        }
                    } else {
                        log::debug!("redis::couldn't make a connection:");
                    }
                },
                _ => {}
            }
        }

        pub async fn invalidate_todo(&self, id: i32) {
            match self {
                Cache::Redis(client) => {
                    if let Ok(mut conn) = client.get_tokio_connection().await {
                        let key = format!("todo_{}", id);
                        if let Err(e) = conn.del::<String, bool>(key).await {
                            log::debug!("redis::Invalidate_todo failed: {:?}", e);
                        }
                    } else {
                        log::debug!("redis::couldn't make a connection:");
                    }
                },
                _ => {}
            }
        }
    }
}