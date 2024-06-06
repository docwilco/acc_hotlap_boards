use anyhow::Result;
use askama_axum::Template;
use axum::{extract, extract::Path, response::IntoResponse};
use itertools::{izip, EitherOrBoth, Itertools};
use log::debug;
use sqlx::SqliteConnection;
use std::{collections::HashMap, time::Duration};

use super::{
    DurationWithClass, State, CAR_MODEL_ID_TO_NAME, NATIONALITY_TO_COUNTRY, NATIONALITY_TO_ISO,
};

#[derive(Clone)]
struct DisplayLine {
    laptime: DurationWithClass,
    splits: Vec<DurationWithClass>,
    car: String,
    ballast_kg: Option<i64>,
    session_type: String,
    timestamp: i64,
    valid: bool,
}

impl DisplayLine {
    // Can't really make this a smaller argument list, but there's only one
    // place this is called from, so it's fine.
    #[allow(clippy::too_many_arguments)]
    fn new(
        laptime: Duration,
        model: i64,
        ballast_kg: Option<i64>,
        mut session_type: String,
        timestamp: i64,
        splits: &[Duration],
        valid: i64,
    ) -> Self {
        // Laptime
        let laptime = DurationWithClass::new(laptime);

        // Splits
        let splits = splits.iter().copied().map(DurationWithClass::new).collect();

        // Car
        let car = (*CAR_MODEL_ID_TO_NAME
            .get(&(model.try_into().unwrap()))
            .unwrap_or(&"Unknown"))
        .to_string();

        // Valid
        let valid = match valid {
            1 => true,
            0 => false,
            _ => panic!("Invalid value for valid"),
        };

        // Session type
        match session_type.chars().next() {
            Some('P') => session_type.push_str("ractice"),
            Some('Q') => session_type.push_str("ualifying"),
            Some('R') => session_type.push_str("ace"),
            _ => {
                session_type.clear();
                session_type.push_str("Unknown");
            }
        }

        // Combine it all together
        Self {
            laptime,
            splits,
            car,
            ballast_kg,
            session_type,
            timestamp,
            valid,
        }
    }
}

#[derive(Clone)]
struct DisplayData {
    steam_id: i64,
    name: String,
    flag_code: &'static str,
    flag_name: &'static str,
    valid_laps: i64,
    total_laps: i64,
    lines_per_track: Vec<(String, Vec<DisplayLine>)>,
}

#[derive(Template)]
#[template(path = "driver.html")]
struct RootTemplate {
    display_data: DisplayData,
}

struct DriverLapsQueryRow {
    track: String,
    laptime_ms: i64,
    model: i64,
    ballast_kg: Option<i64>,
    session_type: String,
    timestamp: i64,
    sector_time_ms: i64,
    valid: i64,
}

struct DriverData {
    name: String,
    nationality: Option<i64>,
    valid_laps: i64,
    total_laps: i64,
}

pub(crate) async fn handler(
    extract::State(state): extract::State<State>,
    Path(steam_id): Path<i64>,
) -> impl IntoResponse {
    debug!("Driver page for steam_id {}", steam_id);
    let display_data = get_display_data(state, steam_id).await.unwrap();
    RootTemplate { display_data }
}

async fn get_display_data(state: State, steam_id: i64) -> Result<DisplayData> {
    let mut conn = state.0.pool.acquire().await?;

    let driver_laps_data = get_driver_laps_data(&mut conn, steam_id).await;

    let overall_fastest_laps = get_overall_fastest_laps(&mut conn).await;

    let best_splits_data = get_fastest_splits(&mut conn).await;

    let driver_data = get_driver_data(&mut conn, steam_id).await;

    let mut lines_per_track = driver_laps_data
        .into_iter()
        .group_by(|row| row.track.clone())
        .into_iter()
        .map(|(track, rows)| {
            // Replace underscore with space and capitalize every first letter of each word in the trackname
            let display_track = track_id_to_display_name(&track);
            let mut display_lines: Vec<DisplayLine> = rows
                .into_iter()
                .group_by(|row| {
                    (
                        row.laptime_ms,
                        row.model,
                        row.ballast_kg,
                        row.session_type.clone(),
                        row.timestamp,
                        row.valid,
                    )
                })
                .into_iter()
                .map(
                    |((laptime_ms, model, ballast_kg, session_type, timestamp, valid), rows)| {
                        // Prepare sector times
                        let splits = rows
                            .into_iter()
                            .map(|row| {
                                Duration::from_millis(row.sector_time_ms.try_into().unwrap())
                            })
                            .collect::<Vec<_>>();

                        let laptime = Duration::from_millis(laptime_ms.try_into().unwrap());

                        DisplayLine::new(
                            laptime,
                            model,
                            ballast_kg,
                            session_type,
                            timestamp,
                            &splits,
                            valid,
                        )
                    },
                )
                .collect();
            let driver_fastest_laptime = display_lines
                .iter()
                .filter_map(|line| {
                    if line.valid {
                        Some(line.laptime.duration)
                    } else {
                        None
                    }
                })
                .min();
            let overall_fastest_laptime = overall_fastest_laps.get(&track).copied();
            set_purple_and_green(
                &track,
                steam_id,
                &mut display_lines,
                driver_fastest_laptime,
                overall_fastest_laptime,
                &best_splits_data,
            );
            Ok((display_track, display_lines))
        })
        .collect::<Result<Vec<_>>>()?;
    // Sort by latest driven
    lines_per_track.sort_unstable_by_key(|(_, lines)| {
        -(lines.iter().map(|line| line.timestamp).max().unwrap_or(0))
    });
    let flag_code = driver_data
        .nationality
        .and_then(|n| NATIONALITY_TO_ISO.get(&n))
        .copied()
        .unwrap_or("xx");
    let flag_name = driver_data
        .nationality
        .and_then(|n| NATIONALITY_TO_COUNTRY.get(&n))
        .copied()
        .unwrap_or("Unknown");
    Ok(DisplayData {
        steam_id,
        name: driver_data.name,
        flag_code,
        flag_name,
        valid_laps: driver_data.valid_laps,
        total_laps: driver_data.total_laps,
        lines_per_track,
    })
}

fn set_purple_and_green(
    track: &str,
    steam_id: i64,
    display_lines: &mut Vec<DisplayLine>,
    driver_fastest_laptime: Option<Duration>,
    overall_fastest_laptime: Option<Duration>,
    best_splits_data: &HashMap<(String, i64), Vec<Duration>>,
) {
    let driver_fastest_splits = best_splits_data.get(&(track.to_string(), steam_id));
    let overall_fastest_splits = best_splits_data
        .iter()
        .filter_map(|(key, splits)| if key.0 == track { Some(splits) } else { None })
        .fold(Vec::new(), |acc: Vec<Duration>, splits| {
            acc.into_iter()
                .zip_longest(splits)
                .map(|eitherorboth| match eitherorboth {
                    EitherOrBoth::Both(a, b) => {
                        if a < *b {
                            a
                        } else {
                            *b
                        }
                    }
                    EitherOrBoth::Left(_) => {
                        unreachable!("Accumulator should never have more values")
                    }
                    EitherOrBoth::Right(b) => *b,
                })
                .collect()
        });

    // If the driver didn't set a valid lap, there's nothing to color green or purple anyway
    let Some(driver_fastest_splits) = driver_fastest_splits else {
        return;
    };

    for display_line in display_lines {
        if overall_fastest_laptime.is_some_and(|ofl| display_line.laptime.duration == ofl) {
            display_line.laptime.class = "purple";
        } else if driver_fastest_laptime.is_some_and(|dfl| display_line.laptime.duration == dfl) {
            display_line.laptime.class = "green";
        }
        for (split, personal_fastest_split, overall_fastest_split) in izip!(
            display_line.splits.iter_mut(),
            driver_fastest_splits.iter().copied(),
            overall_fastest_splits.iter().copied()
        ) {
            if split.duration == overall_fastest_split {
                split.class = "purple";
            } else if personal_fastest_split == split.duration {
                split.class = "green";
            }
        }
    }
}

fn track_id_to_display_name(track: &str) -> String {
    track
        .replace('_', " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .join(" ")
}

async fn get_driver_laps_data(
    conn: &mut SqliteConnection,
    steam_id: i64,
) -> Vec<DriverLapsQueryRow> {
    sqlx::query_as!(
        DriverLapsQueryRow,
        r#"
        SELECT s.track,
            l.time_ms as laptime_ms,
            c.model,
            c.ballast_kg,
            s.type as session_type,
            s.timestamp,
            sp.time_ms AS sector_time_ms,
            l.valid
        FROM sessions s
        INNER JOIN laps l ON s.id = l.session_id
        INNER JOIN splits sp ON l.id = sp.lap_id
        INNER JOIN cars c ON l.car_id = c.id
        WHERE l.steam_id = ? 
        ORDER BY s.track, s.timestamp, l.id, sp.sector;
    "#,
        steam_id
    )
    .fetch_all(conn)
    .await
    .unwrap()
}

async fn get_fastest_splits(conn: &mut SqliteConnection) -> HashMap<(String, i64), Vec<Duration>> {
    sqlx::query!(
        r#"
        SELECT s.track as "track!",
            l.steam_id as "steam_id!",
            sp.sector,
            MIN(sp.time_ms) AS "sector_time_ms: i64"
        FROM splits sp
        INNER JOIN laps l ON sp.lap_id = l.id
        INNER JOIN sessions s ON l.session_id = s.id
        WHERE l.valid = 1
        GROUP BY s.track, l.steam_id, sp.sector
        ORDER BY s.track, l.steam_id, sp.sector;
    "#
    )
    .fetch_all(conn)
    .await
    .unwrap()
    .into_iter()
    .group_by(|row| (row.track.clone(), row.steam_id))
    .into_iter()
    .map(|((track, steam_id), rows)| {
        let best_sectors = rows
            .into_iter()
            .map(|row| Duration::from_millis(row.sector_time_ms.try_into().unwrap()))
            .collect::<Vec<_>>();
        ((track, steam_id), best_sectors)
    })
    .collect::<HashMap<_, _>>()
}

async fn get_overall_fastest_laps(conn: &mut SqliteConnection) -> HashMap<String, Duration> {
    sqlx::query!(
        r#"
        SELECT s.track as "track!",
            MIN(l.time_ms) AS "laptime_ms: i64"
        FROM sessions s
        INNER JOIN laps l ON s.id = l.session_id
        WHERE l.valid = 1
        GROUP BY s.track;
        "#
    )
    .fetch_all(conn)
    .await
    .unwrap()
    .into_iter()
    .map(|row| {
        let track = row.track;
        let laptime = Duration::from_millis(row.laptime_ms.try_into().unwrap());
        (track, laptime)
    })
    .collect::<HashMap<_, _>>()
}

async fn get_driver_data(conn: &mut SqliteConnection, steam_id: i64) -> DriverData {
    let row = sqlx::query!(
        r#"
        SELECT d.first_name,
            d.last_name, 
            d.short_name,
            d.nationality,
            COUNT(1) FILTER (WHERE l.valid = 1) AS "valid_laps: i64",
            COUNT(1) AS "total_laps: i64"
        FROM drivers d
        INNER JOIN laps l ON d.steam_id = l.steam_id
        WHERE d.steam_id = ?
        GROUP BY d.first_name, d.last_name, d.short_name, d.nationality;
        "#,
        steam_id
    )
    .fetch_one(conn)
    .await
    .unwrap();
    let name = format!("{} {} ({})", row.first_name, row.last_name, row.short_name);
    DriverData {
        name,
        nationality: row.nationality,
        valid_laps: row.valid_laps,
        total_laps: row.total_laps,
    }
}
