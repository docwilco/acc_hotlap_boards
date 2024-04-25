#![feature(str_from_utf16_endian)]

use anyhow::{anyhow, Context, Result};
use askama_axum::Template;
use axum::{response::IntoResponse, routing::get, Router};
use chrono::{Date, DateTime, Local, NaiveDate, NaiveDateTime, Utc};
use itertools::Itertools;
use phf::{phf_map, Map};
use serde::Deserialize;
use serde_json::Value;
use serde_with::{serde_as, DurationMilliSeconds};
use std::{
    collections::HashMap,
    fs::{self, read_dir},
    path::Path,
    time::Duration,
};
use tokio::net::TcpListener;
use tower_http::services::ServeDir;

struct Lap {
    laptime: Duration,
    car: u64,
    date: DateTime<Utc>,
    splits: Vec<Duration>,
}

type DriverLaps = HashMap<String, Lap>;
//type Data = HashMap<String, DriverLaps>;
struct Data {
    by_track: HashMap<String, DriverLaps>,
    player_id_to_name: HashMap<String, String>,
}

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
    position: usize,
    name: String,
    laptime: String,
    gap: String,
    interval: String,
    splits: String,
    car: String,
    date: i64,
}

type DisplayData = Vec<(String, Vec<DisplayLine>)>;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonDriver {
    first_name: String,
    last_name: String,
    short_name: String,
    player_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonCar {
    car_id: u64,
    race_number: u64,
    car_model: u64,
    cup_category: u64,
    car_group: String,
    team_name: String,
    nationality: u64,
    car_guid: i64,
    team_guid: i64,
    drivers: Vec<JsonDriver>,
    ballast_kg: Option<i64>,
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonTiming {
    #[serde_as(as = "DurationMilliSeconds<u64>")]
    last_lap: Duration,
    #[serde_as(as = "Vec<DurationMilliSeconds<f64>>")]
    last_splits: Vec<Duration>,
    #[serde_as(as = "DurationMilliSeconds<u64>")]
    best_lap: Duration,
    #[serde_as(as = "Vec<DurationMilliSeconds<f64>>")]
    best_splits: Vec<Duration>,
    #[serde_as(as = "DurationMilliSeconds<u64>")]
    total_time: Duration,
    lap_count: u64,
    last_split_id: u64,
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonLeaderBoardLine {
    car: JsonCar,
    current_driver: JsonDriver,
    current_driver_index: u64,
    timing: JsonTiming,
    missing_mandatory_pitstop: i64,
    #[serde_as(as = "Vec<DurationMilliSeconds<f64>>")]
    driver_total_times: Vec<Duration>,
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonSessionResult {
    #[serde_as(as = "DurationMilliSeconds<u64>")]
    bestlap: Duration,
    #[serde_as(as = "Vec<DurationMilliSeconds<u64>>")]
    best_splits: Vec<Duration>,
    is_wet_session: i64,
    #[serde(rename = "type")]
    session_type: u64,
    leader_board_lines: Vec<JsonLeaderBoardLine>,
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonLap {
    car_id: u64,
    driver_index: u64,
    #[serde_as(as = "DurationMilliSeconds<u64>")]
    laptime: Duration,
    is_valid_for_best: bool,
    #[serde_as(as = "Vec<DurationMilliSeconds<u64>>")]
    splits: Vec<Duration>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonPenalty {
    car_id: u64,
    driver_index: u64,
    reason: String,
    penalty: String,
    penalty_value: u64,
    violation_in_lap: i64,
    cleared_in_lap: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonSessionResults {
    session_type: String,
    track_name: String,
    session_index: u64,
    race_weekend_index: i64,
    server_name: String,
    session_result: JsonSessionResult,
    laps: Vec<JsonLap>,
    penalties: Vec<JsonPenalty>,
    post_race_penalties: Option<Vec<JsonPenalty>>,
}

enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
}

// Some of the JSON files have duplicate fields, this gets rid of them
fn dedup_json(json: &str) -> Result<String> {
    let parsed: Value = serde_json::from_str(json)?;
    Ok(parsed.to_string())
}

fn try_conversion(bytes: &[u8], encoding: Encoding) -> Result<String> {
    Ok(match encoding {
        Encoding::Utf8 => dedup_json(&std::str::from_utf8(&bytes)?)?,
        Encoding::Utf16Le => dedup_json(&String::from_utf16le(&bytes)?)?,
        Encoding::Utf16Be => dedup_json(&String::from_utf16be(&bytes)?)?,
    })
}

fn bytes_to_json_string(bytes: &[u8]) -> Result<String> {
    // Check for BOM
    if bytes[0..3] == [0xEF, 0xBB, 0xBF] {
        // UTF-8 BOM
        return Ok(dedup_json(&std::str::from_utf8(&bytes[3..])?)?);
    } else if bytes[0..2] == [0xFF, 0xFE] {
        return Ok(dedup_json(&String::from_utf16le(&bytes[2..])?)?);
    } else if bytes[0..2] == [0xFE, 0xFF] {
        return Ok(dedup_json(&String::from_utf16be(&bytes[2..])?)?);
    }

    // No BOM
    // Check for lots of BE NUL bytes
    let (be_nuls, le_nuls) =
        bytes
            .chunks(2)
            .fold((0, 0), |(be_nuls, le_nuls), chunk| match chunk {
                [0, 0] => (be_nuls + 1, le_nuls + 1),
                [0, _] => (be_nuls + 1, le_nuls),
                [_, 0] => (be_nuls, le_nuls + 1),
                _ => panic!("chunk size not 2?!"),
            });
    // Let's say if 45+% of the bytes are BE NULs, then it's probably UTF-16BE
    if be_nuls >= (0.45 * (bytes.len() as f64)) as usize {
        if let Ok(text) = try_conversion(bytes, Encoding::Utf16Be) {
            return Ok(text);
        }
    }

    // Same for LE NULs
    if le_nuls >= (0.45 * (bytes.len() as f64)) as usize {
        if let Ok(text) = try_conversion(bytes, Encoding::Utf16Be) {
            return Ok(text);
        }
    }

    // Otherwise, try various UTFs. 16LE is most likely
    if let Ok(text) = try_conversion(bytes, Encoding::Utf16Le) {
        return Ok(text);
    }
    if let Ok(text) = try_conversion(bytes, Encoding::Utf16Be) {
        return Ok(text);
    }
    if let Ok(text) = try_conversion(bytes, Encoding::Utf8) {
        return Ok(text);
    }
    Err(anyhow::anyhow!("Failed to figure out JSON file encoding"))
}

fn add_session_results(
    data: &mut Data,
    session_results: JsonSessionResults,
    session_datetime: DateTime<Utc>,
) -> Result<()> {
    let valid_laps = session_results
        .laps
        .iter()
        .filter(|lap| lap.is_valid_for_best)
        .collect::<Vec<_>>();
    if !valid_laps.is_empty() {
        let track_name = session_results.track_name;
        let driver_laps = data
            .by_track
            .entry(track_name)
            .or_insert(Default::default());
        for lap in valid_laps {
            let car = session_results
                .session_result
                .leader_board_lines
                .iter()
                .find_map(|line| {
                    if line.car.car_id == lap.car_id {
                        Some(&line.car)
                    } else {
                        None
                    }
                })
                .context(anyhow!("Failed to find car for lap"))?;
            let driver = car
                .drivers
                .get(lap.driver_index as usize)
                .context(anyhow!("Failed to find driver for lap"))?;
            if driver_laps.get(&driver.player_id).map_or(false, |fastest| lap.laptime > fastest.laptime)
            {
                continue;
            }
            let splits = lap.splits.clone();
            driver_laps.insert(driver.player_id.clone(), Lap {
                laptime: lap.laptime,
                car: car.car_model,
                date: session_datetime,
                splits,
            });
            data.player_id_to_name.insert(
                driver.player_id.clone(),
                format!(
                    "{} {} ({})",
                    driver.first_name, driver.last_name, driver.short_name
                ),
            );
        }
    }
    Ok(())
}

fn read_file(path: impl AsRef<Path>) -> Result<JsonSessionResults> {
    let path = path.as_ref();
    // Read file into Vec<u8>
    let bytes = fs::read(&path)?;
    // Convert UTF-16 to UTF-8
    let json_text = bytes_to_json_string(&bytes).context("Invalid JSON structure")?;
    // Parse JSON
    let session_results: JsonSessionResults =
        serde_json::from_str(&json_text).with_context(|| {
            format!(
                "Failed to parse JSON file {}\n{}",
                path.file_name().unwrap().to_string_lossy(),
                json_text
            )
        })?;
    Ok(session_results)
}

fn compute_data(results_dir: impl AsRef<Path>) -> Result<Data> {
    let mut data = Data {
        by_track: HashMap::new(),
        player_id_to_name: HashMap::new(),
    };

    // Iterate over all `*[PQR].json` files in the results directory
    for entry in read_dir(results_dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = path.file_name().unwrap().to_string_lossy();
        if !file_name.ends_with("P.json")
            && !file_name.ends_with("Q.json")
            && !file_name.ends_with("R.json")
        {
            continue;
        }
        // Parse filename to get DateTime
        let session_datetime = NaiveDateTime::parse_from_str(&file_name[0..13], "%y%m%d_%H%M%S")
            .context(anyhow!("Failed to parse datetime from filename"))?
            .and_local_timezone(Local)
            .earliest()
            .context(anyhow!("Failed to convert datetime to local timezone"))?
            .with_timezone(&Utc);
        let session_results = read_file(&path)?;
        add_session_results(&mut data, session_results, session_datetime)?;
    }

    Ok(data)
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

async fn root() -> impl IntoResponse {
    let data = compute_data("/home/ac/accsm/server/results").unwrap();
    let display_data = data
        .by_track
        .into_iter()
        .sorted_by_key(|(track, _)| track.to_string())
        .map(|(track, driver_laps)| {
            // Replace _ with space, and capitalize first letters
            let track = track
                .split('_')
                .map(|word| {
                    let mut chars = word.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            let mut fastest_time = None;
            let mut previous_time = None;
            let driver_laps = driver_laps
                .into_iter()
                .sorted_by_key(|(_, lap)| lap.laptime)
                .enumerate()
                .map(|(position, (driver, lap))| {
                    let name = data.player_id_to_name.get(&driver).unwrap().clone();
                    let gap = match fastest_time {
                        None => {
                            fastest_time = Some(lap.laptime);
                            "-".to_string()
                        }
                        Some(fastest) => format_duration(lap.laptime - fastest),
                    };
                    let interval = match previous_time {
                        None => "-".to_string(),
                        Some(previous) => format_duration(lap.laptime - previous),
                    };
                    previous_time = Some(lap.laptime);
                    let splits = lap
                        .splits
                        .iter()
                        .map(|split| format_duration(*split))
                        .collect::<Vec<_>>()
                        .join(" ");
                    DisplayLine {
                        player_id: driver,
                        position: position + 1,
                        name,
                        laptime: format_duration(lap.laptime),
                        gap,
                        interval,
                        splits,
                        car: CAR_MODEL_ID_TO_NAME.get(&lap.car).unwrap().to_string(),
                        date: lap.date.timestamp(),
                    }
                })
                .collect::<Vec<_>>();
            (track, driver_laps)
        })
        .collect::<Vec<_>>();
    RootTemplate { display_data }
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = Router::new()
        .route("/", get(root))
        .nest_service("/static", ServeDir::new("static"));
    let listener = TcpListener::bind("127.0.0.1:3000")
        .await
        .context(anyhow!("Failed to bind to port 3000"))?;
    axum::serve(listener, app)
        .await
        .context(anyhow!("Failed to start server"))?;
    Ok(())
}
