#![feature(str_from_utf16_endian)]

use anyhow::{anyhow, Context, Result};
use askama_axum::Template;
use async_watcher::{notify::RecursiveMode, AsyncDebouncer};
use axum::{debug_handler, extract, response::IntoResponse, routing::get, Router};
use chrono::{DateTime, Local, NaiveDateTime, Utc};
use itertools::Itertools;
use log::{debug, info, warn};
use phf::{phf_map, Map};
use serde::de::DeserializeOwned;
use sqlx::{
    Connection, Sqlite, SqliteConnection, SqliteExecutor, SqlitePool, Transaction,
};
use std::{
    collections::{HashMap, HashSet},
    env,
    fs::{self, read_dir, DirEntry},
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::net::TcpListener;
use tower_http::services::ServeDir;

mod json;
use json::*;

struct StateInner {
    pool: SqlitePool,
}

#[derive(Clone)]
struct State(Arc<StateInner>);

static NATIONALITY_TO_COUNTRY: Map<i64, &'static str> = phf_map! {
    0_i64 => "Other",
    1_i64 => "Italy",
    2_i64 => "Germany",
    3_i64 => "France",
    4_i64 => "Spain",
    5_i64 => "Great Britain",
    6_i64 => "Hungary",
    7_i64 => "Belgium",
    8_i64 => "Switzerland",
    9_i64 => "Austria",
    10_i64 => "Russia",
    11_i64 => "Thailand",
    12_i64 => "Netherlands",
    13_i64 => "Poland",
    14_i64 => "Argentina",
    15_i64 => "Monaco",
    16_i64 => "Ireland",
    17_i64 => "Brazil",
    18_i64 => "South Africa",
    19_i64 => "Puerto Rico",
    20_i64 => "Slovakia",
    21_i64 => "Oman",
    22_i64 => "Greece",
    23_i64 => "Saudi Arabia",
    24_i64 => "Norway",
    25_i64 => "Turkey",
    26_i64 => "South Korea",
    27_i64 => "Lebanon",
    28_i64 => "Armenia",
    29_i64 => "Mexico",
    30_i64 => "Sweden",
    31_i64 => "Finland",
    32_i64 => "Denmark",
    33_i64 => "Croatia",
    34_i64 => "Canada",
    35_i64 => "China",
    36_i64 => "Portugal",
    37_i64 => "Singapore",
    38_i64 => "Indonesia",
    39_i64 => "USA",
    40_i64 => "New Zealand",
    41_i64 => "Australia",
    42_i64 => "San Marino",
    43_i64 => "United Arab Emirates",
    44_i64 => "Luxembourg",
    45_i64 => "Kuwait",
    46_i64 => "Hong Kong",
    47_i64 => "Colombia",
    48_i64 => "Japan",
    49_i64 => "Andorra",
    50_i64 => "Azerbaijan",
    51_i64 => "Bulgaria",
    52_i64 => "Cuba",
    53_i64 => "Czech Republic",
    54_i64 => "Estonia",
    55_i64 => "Georgia",
    56_i64 => "India",
    57_i64 => "Israel",
    58_i64 => "Jamaica",
    59_i64 => "Latvia",
    60_i64 => "Lithuania",
    61_i64 => "Macao",
    62_i64 => "Malaysia",
    63_i64 => "Nepal",
    64_i64 => "New Caledonia",
    65_i64 => "Nigeria",
    66_i64 => "Northern Ireland",
    67_i64 => "Papua New Guinea",
    68_i64 => "Philippines",
    69_i64 => "Qatar",
    70_i64 => "Romania",
    71_i64 => "Scotland",
    72_i64 => "Serbia",
    73_i64 => "Slovenia",
    74_i64 => "Taiwan",
    75_i64 => "Ukraine",
    76_i64 => "Venezuela",
    77_i64 => "Wales",
    78_i64 => "Iran",
    79_i64 => "Bahrain",
    80_i64 => "Zimbabwe",
    81_i64 => "Chinese Taipei",
    82_i64 => "Chile",
    83_i64 => "Uruguay",
    84_i64 => "Madagascar",
    85_i64 => "Malta",
    86_i64 => "England",
};

static NATIONALITY_TO_ISO: Map<i64, &'static str> = phf_map! {
    0_i64 => "xx", // Other
    1_i64 => "it", // Italy
    2_i64 => "de", // Germany
    3_i64 => "fr", // France
    4_i64 => "es", // Spain
    5_i64 => "gb", // Great Britain
    6_i64 => "hu", // Hungary
    7_i64 => "be", // Belgium
    8_i64 => "ch", // Switzerland
    9_i64 => "at", // Austria
    10_i64 => "ru", // Russia
    11_i64 => "th", // Thailand
    12_i64 => "nl", // Netherlands
    13_i64 => "pl", // Poland
    14_i64 => "ar", // Argentina
    15_i64 => "mc", // Monaco
    16_i64 => "ie", // Ireland
    17_i64 => "br", // Brazil
    18_i64 => "za", // South Africa
    19_i64 => "pr", // Puerto Rico
    20_i64 => "sk", // Slovakia
    21_i64 => "om", // Oman
    22_i64 => "gr", // Greece
    23_i64 => "sa", // Saudi Arabia
    24_i64 => "no", // Norway
    25_i64 => "tr", // Turkey
    26_i64 => "kr", // South Korea
    27_i64 => "lb", // Lebanon
    28_i64 => "am", // Armenia
    29_i64 => "mx", // Mexico
    30_i64 => "se", // Sweden
    31_i64 => "fi", // Finland
    32_i64 => "dk", // Denmark
    33_i64 => "hr", // Croatia
    34_i64 => "ca", // Canada
    35_i64 => "cn", // China
    36_i64 => "pt", // Portugal
    37_i64 => "sg", // Singapore
    38_i64 => "id", // Indonesia
    39_i64 => "us", // USA
    40_i64 => "nz", // New Zealand
    41_i64 => "au", // Australia
    42_i64 => "sm", // San Marino
    43_i64 => "ae", // United Arab Emirates
    44_i64 => "lu", // Luxembourg
    45_i64 => "kw", // Kuwait
    46_i64 => "hk", // Hong Kong
    47_i64 => "co", // Colombia
    48_i64 => "jp", // Japan
    49_i64 => "ad", // Andorra
    50_i64 => "az", // Azerbaijan
    51_i64 => "bg", // Bulgaria
    52_i64 => "cu", // Cuba
    53_i64 => "cz", // Czech Republic
    54_i64 => "ee", // Estonia
    55_i64 => "ge", // Georgia
    56_i64 => "in", // India
    57_i64 => "il", // Israel
    58_i64 => "jm", // Jamaica
    59_i64 => "lv", // Latvia
    60_i64 => "lt", // Lithuania
    61_i64 => "mo", // Macao
    62_i64 => "my", // Malaysia
    63_i64 => "np", // Nepal
    64_i64 => "nc", // New Caledonia
    65_i64 => "ng", // Nigeria
    66_i64 => "gb-nir", // Northern Ireland
    67_i64 => "pg", // Papua New Guinea
    68_i64 => "ph", // Philippines
    69_i64 => "qa", // Qatar
    70_i64 => "ro", // Romania
    71_i64 => "gb-sct", // Scotland
    72_i64 => "rs", // Serbia
    73_i64 => "si", // Slovenia
    74_i64 => "tw", // Taiwan
    75_i64 => "ua", // Ukraine
    76_i64 => "ve", // Venezuela
    77_i64 => "gb-wls", // Wales
    78_i64 => "ir", // Iran
    79_i64 => "bh", // Bahrain
    80_i64 => "zw", // Zimbabwe
    81_i64 => "tw", // Chinese Taipei
    82_i64 => "cl", // Chile
    83_i64 => "uy", // Uruguay
    84_i64 => "mg", // Madagascar
    85_i64 => "mt", // Malta
    86_i64 => "gb-eng", // England

};

static CAR_MODEL_ID_TO_NAME: Map<u64, &'static str> = phf_map! {
    0_u64 => "Porsche 991 GT3 R",
    1_u64 => "Mercedes-AMG GT3",
    2_u64 => "Ferrari 488 GT3",
    3_u64 => "Audi R8 LMS",
    4_u64 => "Lamborghini Huracán GT3",
    5_u64 => "McLaren 650S GT3",
    6_u64 => "Nissan GT-R Nismo GT3",
    7_u64 => "BMW M6 GT3",
    8_u64 => "Bentley Continental GT3",
    9_u64 => "Porsche 991 II GT3 Cup",
    10_u64 => "Nissan GT-R Nismo GT3",
    11_u64 => "Bentley Continental GT3",
    12_u64 => "AMR V12 Vantage GT3",
    13_u64 => "Reiter Engineering R-EX GT3",
    14_u64 => "Emil Frey Jaguar G3",
    15_u64 => "Lexus RC F GT3",
    16_u64 => "Lamborghini Huracan GT3 Evo",
    17_u64 => "Honda NSX GT3",
    18_u64 => "Lamborghini Huracan SuperTrofeo",
    19_u64 => "Audi R8 LMS Evo",
    20_u64 => "AMR V8 Vantage",
    21_u64 => "Honda NSX GT3 Evo",
    22_u64 => "McLaren 720S GT3",
    23_u64 => "Porsche 991 II GT3 R",
    24_u64 => "Ferrari 488 GT3 Evo",
    25_u64 => "Mercedes-AMG GT3",
    26_u64 => "Ferrari 488 Challenge Evo",
    27_u64 => "BMW M2 Club Sport Racing",
    28_u64 => "Porsche 992 GT3 Cup",
    29_u64 => "Lamborghini Huracán SuperTrofeo EVO2",
    30_u64 => "BMW M4 GT3",
    31_u64 => "Audi R8 LMS GT3 Evo 2",
    32_u64 => "Ferrari 296 GT3",
    33_u64 => "Lamborghini Huracan GT3 Evo 2",
    34_u64 => "Porsche 992 GT3 R",
    35_u64 => "McLaren 720S GT3 Evo",
    50_u64 => "Alpine A110 GT4",
    51_u64 => "Aston Martin Vantage GT4",
    52_u64 => "Audi R8 LMS GT4",
    53_u64 => "BMW M4 GT4",
    55_u64 => "Chevrolet Camaro GT4",
    56_u64 => "Ginetta G55 GT4",
    57_u64 => "KTM X-Bow GT4",
    58_u64 => "Maserati MC GT4",
    59_u64 => "McLaren 570S GT4",
    60_u64 => "Mercedes AMG GT4",
    61_u64 => "Porsche 718 Cayman GT4 Clubsport",
    80_u64 => "Audi R8 LMS GT2",
    82_u64 => "KTM XBOW GT2",
    83_u64 => "Maserati MC20 GT2",
    84_u64 => "Mercedes AMG GT2",
    85_u64 => "Porsche 911 GT2 RS CS Evo",
    86_u64 => "Porsche 935",
};

struct DisplayLine {
    player_id: String,
    name: String,
    flag_code: &'static str,
    flag_name: &'static str,
    laptime: String,
    gap: String,
    interval: String,
    splits: String,
    car: String,
    date: i64,
}

type DisplayData = Vec<(String, Vec<DisplayLine>)>;

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
    session_results: &JsonSessionResults,
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
        "SELECT lap_id, time_ms
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
        rows.map(|row| Duration::from_millis(row.time_ms as u64))
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

fn player_id_to_steam_id(player_id: &str) -> Result<i64> {
    let mut chars = player_id.chars();
    if chars.next() != Some('S') {
        return Err(anyhow!("Invalid player ID: {}", player_id));
    }
    let player_id = chars.collect::<String>().parse::<i64>()?;
    Ok(player_id)
}

async fn add_session_results(
    conn: &mut SqliteConnection,
    session_results: JsonSessionResults,
    filename: &str,
) -> Result<()> {
    let mut tx = sqlx::Connection::begin(&mut *conn).await?;
    let timestamp = filename_to_timestamp(filename)?;
    while check_previous_session_overlap(&mut tx, &session_results, timestamp).await? {}
    let timestamp = timestamp.timestamp();
    let session_id = sqlx::query!(
        "INSERT INTO sessions (track, type, timestamp, server_name, wet) VALUES (?, ?, ?, ?, ?) RETURNING id;",
        session_results.track_name,
        session_results.session_type,
        timestamp,
        session_results.server_name,
        session_results.session_result.is_wet_session
    )
    .fetch_one(&mut *tx)
    .await?.id;

    // Snag all of the driver info from the session results
    let mut steam_id_to_player_names = HashMap::new();
    let mut car_driver_to_player = HashMap::new();
    let mut car_id_to_db_id = HashMap::new();
    for line in session_results.session_result.leader_board_lines {
        // enumerate here because there's a driver index in the lap data
        for (index, driver) in line.car.drivers.iter().enumerate() {
            let steam_id = player_id_to_steam_id(&driver.player_id)?;
            steam_id_to_player_names.insert(steam_id, driver.clone());
            car_driver_to_player.insert((line.car.car_id, index as i64), steam_id);
        }
        let db_car_id = sqlx::query!(
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
        .fetch_one(&mut *tx)
        .await?
        .id;
        car_id_to_db_id.insert(line.car.car_id, db_car_id);
    }

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
        .execute(&mut *tx)
        .await?;
    }

    for lap in session_results.laps {
        let player_id = car_driver_to_player
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
        let time_ms = lap.laptime.as_millis() as i64;
        let lap_id = sqlx::query!(
            "INSERT INTO laps (player_id, session_id, car_id, time_ms, valid) VALUES (?, ?, ?, ?, ?) RETURNING id;",
            player_id,
            session_id,
            car_id,
            time_ms,
            lap.is_valid_for_best
        )
        .fetch_one(&mut *tx)
        .await?.id;
        for (index, split_time) in lap.splits.iter().enumerate() {
            let time_ms = split_time.as_millis() as i64;
            let sector = index as i64 + 1;
            sqlx::query!(
                "INSERT INTO splits (lap_id, sector, time_ms) VALUES (?, ?, ?);",
                lap_id,
                sector,
                time_ms
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

async fn add_entrylist(
    conn: &mut SqliteConnection,
    entrylist: JsonEntryList,
    filename: &str,
) -> Result<()> {
    let mut tx = sqlx::Connection::begin(&mut *conn).await?;
    for entry in entrylist.entries {
        for driver in entry.drivers {
            let steam_id = player_id_to_steam_id(&driver.player_id)?;
            sqlx::query!(
                "INSERT INTO players 
                (steam_id, first_name, last_name, short_name)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(steam_id) DO UPDATE SET
                first_name = excluded.first_name,
                last_name = excluded.last_name,
                short_name = excluded.short_name;",
                steam_id,
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
                    steam_id
                )
                .execute(&mut *tx)
                .await?;
            }
            if let Some(nationality) = driver.nationality {
                sqlx::query!(
                    "UPDATE players SET nationality = ? WHERE steam_id = ?;",
                    nationality,
                    steam_id
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
        Some('P') | Some('Q') | Some('R') => {
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
            let entrylist: JsonEntryList = match read_file(&path) {
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

async fn check_directory(results_dir: impl AsRef<Path>, state: &State) -> Result<()> {
    let mut conn = state.0.pool.acquire().await?;
    // Iterate over all `*[PQR].json` files in the results directory
    let mut files = read_dir(&results_dir)?.collect::<std::io::Result<Vec<DirEntry>>>()?;
    // Sort so that they are processed in order, otherwise the superset of previous file detection won't work.
    files.sort_unstable_by_key(|entry| entry.path());
    for entry in files {
        check_file(entry.path(), &mut conn).await?;
    }
    info!("All files in {} processed", results_dir.as_ref().display());
    Ok(())
}

#[derive(Template)]
#[template(path = "root.html")]
struct RootTemplate {
    display_data: DisplayData,
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let minutes = seconds / 60;
    let seconds = seconds % 60;
    let milliseconds = duration.subsec_millis();
    if minutes == 0 {
        format!("{}.{:03}", seconds, milliseconds)
    } else {
        format!("{}:{:02}.{:03}", minutes, seconds, milliseconds)
    }
}

#[debug_handler]
async fn root(extract::State(state): extract::State<State>) -> impl IntoResponse {
    let mut conn = state.0.pool.acquire().await.unwrap();
    let data = sqlx::query!(
        r#"
        SELECT s.track,
            p.steam_id AS player_id,
            p.first_name,
            p.last_name, 
            p.short_name,
            p.nationality,
            l.id AS lap_id,
            l.time_ms,
            c.model,
            s.timestamp
        FROM sessions s
        INNER JOIN laps l ON s.id = l.session_id
        INNER JOIN cars c ON l.car_id = c.id
        INNER JOIN players p ON l.player_id = p.steam_id
        WHERE l.id = (SELECT sl.id
                      FROM laps sl
                      INNER JOIN sessions ss ON sl.session_id = ss.id
                      WHERE ss.track = s.track AND sl.player_id = l.player_id AND sl.valid = 1
                      ORDER BY sl.time_ms, ss.timestamp
                      LIMIT 1)
        AND l.valid = 1
        ORDER BY s.track, l.time_ms;
    "#
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();

    let mut lap_to_splits = HashMap::new();
    for lap_id in data.iter().map(|row| row.lap_id) {
        let splits = sqlx::query!(
            r#"
            SELECT time_ms
            FROM splits
            WHERE lap_id = ?
            ORDER BY sector;
        "#,
            lap_id
        )
        .map(|row| format_duration(Duration::from_millis(row.time_ms as u64)))
        .fetch_all(&mut *conn)
        .await
        .unwrap()
        .join(" | ");
        lap_to_splits.insert(lap_id, splits);
    }
    let display_data = data
        .into_iter()
        .group_by(|row| row.track.clone())
        .into_iter()
        .map(|(track, rows)| {
            // Replace underscore with space and capitalize every first letter of each word in the trackname
            let track = track.replace('_', " ");
            let track = track
                .split_whitespace()
                .map(|word| {
                    let mut chars = word.chars();
                    match chars.next() {
                        Some(c) => c.to_uppercase().chain(chars).collect::<String>(),
                        None => "".to_string(),
                    }
                })
                .join(" ");
            let mut fastest_time = None;
            let mut previous_time = None;
            let display_lines = rows
                .into_iter()
                .map(|row| {
                    let player_id = format!("S{}", row.player_id);
                    let name = format!("{} {} ({})", row.first_name, row.last_name, row.short_name);
                    let laptime = format_duration(Duration::from_millis(row.time_ms as u64));
                    let gap = match fastest_time {
                        Some(fastest_time) => {
                            let diff = row.time_ms - fastest_time;
                            format_duration(Duration::from_millis(diff as u64))
                        }
                        None => {
                            fastest_time = Some(row.time_ms);
                            "-".to_string()
                        }
                    };
                    let interval = match previous_time {
                        Some(previous_time) => {
                            let diff = row.time_ms - previous_time;
                            format_duration(Duration::from_millis(diff as u64))
                        }
                        None => "-".to_string(),
                    };
                    previous_time = Some(row.time_ms);

                    let splits = lap_to_splits.get(&row.lap_id).unwrap().to_string();
                    let car = CAR_MODEL_ID_TO_NAME
                        .get(&(row.model as u64))
                        .unwrap_or(&"Unknown");
                    let date = row.timestamp;
                    let natl = row.nationality;
                    let flag_code = natl
                        .and_then(|n| NATIONALITY_TO_ISO.get(&n))
                        .unwrap_or(&"xx");
                    let flag_name = natl
                        .and_then(|n| NATIONALITY_TO_COUNTRY.get(&n))
                        .unwrap_or(&"Unknown");
                    DisplayLine {
                        player_id,
                        name,
                        flag_code,
                        flag_name,
                        laptime,
                        gap,
                        interval,
                        splits,
                        car: car.to_string(),
                        date,
                    }
                })
                .collect();
            (track, display_lines)
        })
        .collect::<DisplayData>();
    RootTemplate { display_data }
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

    let state = State(Arc::new(StateInner { pool: pool }));

    // Check for new files
    check_directory(&results_path, &state).await?;

    // Start watcher task
    tokio::spawn(async move {
        watcher_task().await.unwrap();
    });

    let app = Router::new()
        .route("/", get(root))
        .with_state(state.clone())
        .nest_service("/static", ServeDir::new("static"));
    let listener = TcpListener::bind("127.0.0.1:3000")
        .await
        .context(anyhow!("Failed to bind to port 3000"))?;
    axum::serve(listener, app)
        .await
        .context(anyhow!("Failed to start server"))?;
    Ok(())
}
