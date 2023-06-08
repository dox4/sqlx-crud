use anyhow::Ok;
use sqlx::{
    mysql::{MySqlConnectOptions, MySqlPoolOptions},
    types::chrono::{DateTime, Local},
    FromRow, MySql, Pool,
};
use sqlx_crud::{add_timed_fields, Crud, SqlxCrud};
use std::{env, str::FromStr, time::Duration};

#[derive(FromRow, SqlxCrud)]
struct Record {
    #[auto_increment]
    record_id: i64,
    str_field: String,
    #[ignore_when(insert)]
    updated_at: Option<DateTime<Local>>,
}
#[derive(Debug, FromRow, SqlxCrud, Default)]
#[allow(dead_code)]
struct MoreFields {
    more_field_id: i64,
    str_field: String,
    #[ignore_when(insert, update)]
    created_at: Option<DateTime<Local>>,
    #[ignore_when(insert, update)]
    updated_at: Option<DateTime<Local>>,
    #[ignore_when(insert, update)]
    deleted_at: Option<DateTime<Local>>,
}
use serde::Serialize;
#[add_timed_fields]
#[derive(Debug, Clone, FromRow, SqlxCrud, Serialize, Default)]
struct TimedField {
    timed_field_id: i64,
    str_field: String,
}

async fn db_conn() -> anyhow::Result<Pool<MySql>> {
    let db_host = env::var("MYSQL_DB_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let db_port = env::var("MYSQL_DB_PORT").unwrap_or_else(|_| "3306".to_string());
    let db_user = env::var("MYSQL_DB_USER").unwrap_or_else(|_| "root".to_string());
    let db_password = env::var("MYSQL_DB_PASSWORD").unwrap_or_else(|_| "password".to_string());
    let db_databse = env::var("MYSQL_DB_DATABASE").unwrap_or_else(|_| "crud".to_string());
    let db_url = &format!(
        "mysql://{}:{}@{}:{}/{}",
        db_user, db_password, db_host, db_port, db_databse
    );
    let conn_opts = MySqlConnectOptions::from_str(&db_url)?;
    let pool = MySqlPoolOptions::new()
        .min_connections(5)
        .connect_with(conn_opts)
        .await?;
    Ok(pool)
}

async fn test_record(pool: &Pool<MySql>) -> anyhow::Result<()> {
    let record = Record {
        record_id: 1,
        str_field: "hello".to_string(),
        updated_at: None,
    };
    let record_id = record.record_id;
    let r = record.create(&pool).await?;
    assert_eq!(1, r.rows_affected());

    let record = Record::by_id(&pool, record_id).await?;
    match record {
        Some(record) => match record.updated_at {
            Some(upd_at) => println!("{}", upd_at),
            None => panic!("unreachable"),
        },
        None => panic!("error"),
    }
    Ok(())
}

async fn test_more_fields(pool: &Pool<MySql>) -> anyhow::Result<()> {
    let frecord = MoreFields {
        more_field_id: 16,
        str_field: "hello".to_string(),
        ..Default::default()
    };
    let r = frecord.create(&pool).await?;
    assert_eq!(1, r.rows_affected());
    let mut frecord = MoreFields::by_id(&pool, 16).await?.unwrap();
    println!("{:?}", frecord);
    frecord.str_field = "world".to_string();
    std::thread::sleep(Duration::from_secs(2));
    let r = frecord.update(&pool).await?;
    assert_eq!(1, r.rows_affected());
    let frecord = MoreFields::by_id(&pool, 16).await?.unwrap();
    println!("{:?}", frecord);
    let r = frecord.delete(&pool).await?;
    assert_eq!(1, r.rows_affected());
    Ok(())
}

async fn test_timed_fields(pool: &Pool<MySql>) -> anyhow::Result<()> {
    let record = TimedField {
        timed_field_id: 21,
        str_field: "hello".to_string(),
        ..Default::default()
    };
    let r = record.clone().create(pool).await?;
    assert_eq!(1, r.rows_affected());
    println!("to json: {}", serde_json::to_string(&record).unwrap());
    let record = TimedField::by_id(pool, 21).await?.unwrap();
    println!("to json: {}", serde_json::to_string(&record).unwrap());
    let r = record.delete(&pool).await?;
    assert_eq!(1, r.rows_affected());
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();
    let pool = db_conn().await?;
    test_record(&pool).await?;
    test_more_fields(&pool).await?;
    test_timed_fields(&pool).await?;
    Ok(())
}
