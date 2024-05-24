#![feature(str_from_utf16_endian)]

use anyhow::{anyhow, Context, Result};
use async_watcher::{notify::RecursiveMode, AsyncDebouncer};
use chrono::{DateTime, Local, NaiveDateTime, Utc};
use itertools::Itertools;
use log::{debug, info, warn};
use serde::de::DeserializeOwned;
use sqlx::{Connection, Sqlite, SqliteConnection, SqliteExecutor, SqlitePool, Transaction};
use std::{
    collections::{HashMap, HashSet},
    env,
    fs::{self, read_dir, DirEntry},
    path::Path,
    time::Duration,
};

mod appserver;
mod json;
use json::bytes_to_json_string;

fn read_file<T>(path: impl AsRef<Path>) -> Result<T>
where
    T: DeserializeOwned,
{
    let path = path.as_ref();
    // Read file into Vec<u8>
    let bytes = fs::read(path)?;
    // Convert UTF-16 to UTF-8
    let json_text = bytes_to_json_string(&bytes).context("Invalid JSON structure")?;
    // Parse JSON
    let result: T = serde_json::from_str(&json_text).with_context(|| {
        format!(
            "Failed to parse JSON file {}\n{}",
            path.file_name().unwrap().to_string_lossy(),
            json_text
        )
    })?;
    Ok(result)
}

async fn delete_session(executor: impl SqliteExecutor<'_>, session_id: i64) -> Result<()> {
    sqlx::query!("DELETE FROM sessions WHERE id = ?;", session_id)
        .execute(executor)
        .await?;
    Ok(())
}

async fn check_previous_session_overlap(
    tx: &mut Transaction<'_, Sqlite>,
    session_results: &json::SessionResults,
    timestamp: DateTime<Utc>,
) -> Result<bool> {
    let timestamp = timestamp.timestamp();
    let previous_session_id = sqlx::query!(
        "SELECT id
        FROM sessions
        WHERE track = ?
        AND type = ?
        AND server_name = ?
        AND wet = ?
        AND timestamp < ?
        ORDER BY timestamp DESC
        LIMIT 1;",
        session_results.track_name,
        session_results.session_type,
        session_results.server_name,
        session_results.session_result.is_wet_session,
        timestamp
    )
    .fetch_optional(&mut **tx)
    .await?
    .map(|row| row.id);
    if previous_session_id.is_none() {
        return Ok(false);
    }
    let previous_session_id = previous_session_id.unwrap();
    let previous_laps = sqlx::query!(
        "SELECT lap_id, time_ms as sector_time_ms
        FROM splits
        WHERE lap_id IN (
            SELECT id
            FROM laps
            WHERE session_id = (
                SELECT id
                FROM sessions
                WHERE track = ?
                AND type = ?
                AND server_name = ?
                AND wet = ?
                AND timestamp < ?
                ORDER BY timestamp DESC
                LIMIT 1
            )
        )
        ORDER BY lap_id, sector;",
        session_results.track_name,
        session_results.session_type,
        session_results.server_name,
        session_results.session_result.is_wet_session,
        timestamp
    )
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .group_by(|row| row.lap_id)
    .into_iter()
    .map(|(_, rows)| {
        rows.map(|row| Duration::from_millis(row.sector_time_ms.try_into().unwrap()))
            .collect::<Vec<_>>()
    })
    .collect::<HashSet<_>>();
    let current_laps = session_results
        .laps
        .iter()
        .map(|lap| lap.splits.clone())
        .collect::<HashSet<_>>();
    if current_laps.is_superset(&previous_laps) {
        warn!("Session is superset of previous session, deleting previous");
        delete_session(&mut **tx, previous_session_id).await?;
        return Ok(true);
    }
    Ok(false)
}

async fn add_session_results(
    conn: &mut SqliteConnection,
    session_results: json::SessionResults,
    filename: &str,
) -> Result<()> {
    let mut tx = sqlx::Connection::begin(&mut *conn).await?;
    let timestamp = filename_to_timestamp(filename)?;

    // Check for and delete previous session if current one is a superset of them
    while check_previous_session_overlap(&mut tx, &session_results, timestamp).await? {}

    let session_id = insert_session_row(timestamp, &session_results, &mut tx).await?;

    // Snag all of the driver info from the session results
    let mut steam_id_to_player_names = HashMap::new();
    let mut car_driver_to_steam_id = HashMap::new();
    let mut car_id_to_db_id = HashMap::new();
    for line in session_results.session_result.leader_board_lines {
        // there's a driver index in the lap data that matches the index from
        // enumerate()
        for (index, driver) in line.car.drivers.iter().enumerate() {
            steam_id_to_player_names.insert(driver.steam_id, driver.clone());
            car_driver_to_steam_id.insert(
                (line.car.car_id, i64::try_from(index).unwrap()),
                driver.steam_id,
            );
        }
        let db_car_id = insert_car(session_id, &line, &mut tx).await?;
        car_id_to_db_id.insert(line.car.car_id, db_car_id);
    }

    upsert_driver_data(steam_id_to_player_names, &mut tx).await?;

    for lap in session_results.laps {
        let steam_id = car_driver_to_steam_id
            .get(&(lap.car_id, lap.driver_index))
            .ok_or_else(|| {
                anyhow!(
                    "No player ID found for car {} driver {}",
                    lap.car_id,
                    lap.driver_index
                )
            })?;
        let car_id = car_id_to_db_id
            .get(&lap.car_id)
            .ok_or_else(|| anyhow!("No car ID found for car {}", lap.car_id))?;
        let laptime_ms: i64 = lap.laptime.as_millis().try_into().unwrap();
        let lap_id = sqlx::query!(
            "INSERT INTO laps (steam_id, session_id, car_id, time_ms, valid) VALUES (?, ?, ?, ?, ?) RETURNING id;",
            steam_id,
            session_id,
            car_id,
            laptime_ms,
            lap.is_valid_for_best
        )
        .fetch_one(&mut *tx)
        .await?.id;
        for (index, split_time) in lap.splits.iter().enumerate() {
            let sector_time_ms: i64 = split_time.as_millis().try_into().unwrap();
            #[allow(clippy::cast_possible_wrap)]
            let sector = index as i64 + 1;
            sqlx::query!(
                "INSERT INTO splits (lap_id, sector, time_ms) VALUES (?, ?, ?);",
                lap_id,
                sector,
                sector_time_ms
            )
            .execute(&mut *tx)
            .await?;
        }
    }

    sqlx::query!("INSERT INTO known_files (path) VALUES (?);", filename)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn upsert_driver_data(
    steam_id_to_player_names: HashMap<i64, json::Driver>,
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<(), anyhow::Error> {
    for (id, player) in steam_id_to_player_names {
        sqlx::query!(
            "INSERT INTO players
            (steam_id, first_name, last_name, short_name)
            VALUES (?, ?, ?, ?)
            ON CONFLICT (steam_id) DO UPDATE SET
            first_name = excluded.first_name,
            last_name = excluded.last_name,
            short_name = excluded.short_name;",
            id,
            player.first_name,
            player.last_name,
            player.short_name
        )
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn insert_car(
    session_id: i64,
    line: &json::LeaderBoardLine,
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<i64, anyhow::Error> {
    Ok(sqlx::query!(
        "INSERT OR REPLACE INTO cars (
                session_id, race_number, model, cup_category, car_group, team_name, ballast_kg
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            RETURNING id;",
        session_id,
        line.car.race_number,
        line.car.car_model,
        line.car.cup_category,
        line.car.car_group,
        line.car.team_name,
        line.car.ballast_kg
    )
    .fetch_one(&mut **tx)
    .await?
    .id)
}

async fn insert_session_row(
    timestamp: DateTime<Utc>,
    session_results: &json::SessionResults,
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<i64, anyhow::Error> {
    let timestamp = timestamp.timestamp();
    Ok(sqlx::query!(
        "INSERT INTO sessions (track, type, timestamp, server_name, wet) VALUES (?, ?, ?, ?, ?) RETURNING id;",
        session_results.track_name,
        session_results.session_type,
        timestamp,
        session_results.server_name,
        session_results.session_result.is_wet_session
    )
    .fetch_one(&mut **tx)
    .await?.id)
}

async fn add_entrylist(
    conn: &mut SqliteConnection,
    entrylist: json::EntryList,
    filename: &str,
) -> Result<()> {
    let mut tx = sqlx::Connection::begin(&mut *conn).await?;
    for entry in entrylist.entries {
        for driver in entry.drivers {
            sqlx::query!(
                "INSERT INTO players 
                (steam_id, first_name, last_name, short_name)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(steam_id) DO UPDATE SET
                first_name = excluded.first_name,
                last_name = excluded.last_name,
                short_name = excluded.short_name;",
                driver.steam_id,
                driver.first_name,
                driver.last_name,
                driver.short_name,
            )
            .execute(&mut *tx)
            .await?;
            if let Some(nickname) = driver.nick_name {
                sqlx::query!(
                    "UPDATE players SET nickname = ? WHERE steam_id = ?;",
                    nickname,
                    driver.steam_id
                )
                .execute(&mut *tx)
                .await?;
            }
            if let Some(nationality) = driver.nationality {
                sqlx::query!(
                    "UPDATE players SET nationality = ? WHERE steam_id = ?;",
                    nationality,
                    driver.steam_id
                )
                .execute(&mut *tx)
                .await?;
            }
        }
    }
    sqlx::query!("INSERT INTO known_files (path) VALUES (?);", filename)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

fn filename_to_timestamp(filename: &str) -> Result<DateTime<Utc>> {
    Ok(
        NaiveDateTime::parse_from_str(&filename[0..13], "%y%m%d_%H%M%S")
            .context(anyhow!("Failed to parse datetime from filename"))?
            .and_local_timezone(Local)
            .earliest()
            .context(anyhow!("Failed to convert datetime to local timezone"))?
            .with_timezone(&Utc),
    )
}

async fn check_file(path: impl AsRef<Path>, conn: &mut SqliteConnection) -> Result<()> {
    let filename = path
        .as_ref()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    if !filename.ends_with("P.json")
        && !filename.ends_with("Q.json")
        && !filename.ends_with("R.json")
        && !filename.ends_with("entrylist.json")
    {
        debug!("Skipping file: {} (wrong filename format)", filename);
        return Ok(());
    };
    let known = sqlx::query!("SELECT path FROM known_files WHERE path = ?;", filename)
        .fetch_optional(&mut *conn)
        .await?;
    if known.is_some() {
        info!("Skipping file: {} (already in database)", filename);
        return Ok(());
    }
    let mut filename_chars = filename.chars();
    match filename_chars.nth(14) {
        Some('P' | 'Q' | 'R') => {
            info!("Processing results file: {}", filename);
            assert_eq!(filename_chars.collect::<String>(), ".json");
            let session_results = match read_file(&path) {
                Ok(session_results) => session_results,
                Err(e) => {
                    warn!("Failed to read results file: {}", e);
                    return Ok(());
                }
            };
            add_session_results(&mut *conn, session_results, &filename).await?;
        }
        Some('e') => {
            info!("Processing entrylist file: {}", filename);
            assert_eq!(filename_chars.collect::<String>(), "ntrylist.json");
            let entrylist: json::EntryList = match read_file(&path) {
                Ok(entrylist) => entrylist,
                Err(e) => {
                    warn!("Failed to read entrylist file: {}", e.root_cause());
                    return Ok(());
                }
            };
            add_entrylist(&mut *conn, entrylist, &filename).await?;
        }
        _ => unreachable!("Invalid filename format"),
    };
    Ok(())
}

async fn check_directory(results_dir: impl AsRef<Path>, pool: &SqlitePool) -> Result<()> {
    let mut conn = pool.acquire().await?;
    // Iterate over all `*[PQR].json` files in the results directory
    let mut files = read_dir(&results_dir)?.collect::<std::io::Result<Vec<DirEntry>>>()?;
    // Sort so that they are processed in order, otherwise the superset of previous file detection won't work.
    files.sort_unstable_by_key(DirEntry::path);
    for entry in files {
        check_file(entry.path(), &mut conn).await?;
    }
    info!("All files in {} processed", results_dir.as_ref().display());
    Ok(())
}

async fn watcher_task<'a>() -> Result<()> {
    let dburl = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let results_path = env::var("RESULTS_PATH").expect("RESULTS_PATH must be set");
    // Make single connection
    let mut conn = SqliteConnection::connect(&dburl).await?;
    let (mut debouncer, mut file_events) =
        AsyncDebouncer::new_with_channel(Duration::from_secs(1), Some(Duration::from_secs(1)))
            .await?;
    debouncer
        .watcher()
        .watch(results_path.as_ref(), RecursiveMode::Recursive)
        .unwrap();
    while let Some(result) = file_events.recv().await {
        if let Ok(events) = result {
            debug!("Received events: {:?}", events);
            for event in events {
                check_file(event.path, &mut conn).await?;
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Read .env file
    dotenvy::dotenv()?;

    let dburl = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let results_path = env::var("RESULTS_PATH").expect("RESULTS_PATH must be set");

    // Initialize logger
    env_logger::init();

    // Connect to the database and run migrations
    let pool = SqlitePool::connect(&dburl).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;

    // Check for new files
    check_directory(&results_path, &pool).await?;

    // Start watcher task
    tokio::spawn(async move {
        watcher_task().await.unwrap();
    });

    appserver::run(pool).await
}
