use super::State;
use askama_axum::Template;
use axum::{extract, response::IntoResponse};
use itertools::Itertools;
use sqlx::SqliteConnection;
use std::collections::HashMap;
use std::time::Duration;

use super::format_duration;
use super::CAR_MODEL_ID_TO_NAME;
use super::NATIONALITY_TO_COUNTRY;
use super::NATIONALITY_TO_ISO;

struct DisplayLine {
    steam_id: i64,
    name: String,
    flag_code: &'static str,
    flag_name: &'static str,
    laptime: String,
    optimal_laptime: String,
    gap: String,
    interval: String,
    splits: Vec<String>,
    best_splits: Vec<String>,
    car: String,
    timestamp: i64,
    laps_valid: i32,
    laps_total: i32,
}

impl DisplayLine {
    fn new(
        steam_id: i64,
        first_name: &str,
        last_name: &str,
        short_name: &str,
        nationality: Option<i64>,
        time_ms: i64,
        model: i64,
        timestamp: i64,
        splits: &Vec<Duration>,
        best_splits: &Vec<Duration>,
        fastest_time: &mut Option<Duration>,
        previous_time: &mut Option<Duration>,
        laps_valid: i32,
        laps_total: i32,
    ) -> Self {
        // Name
        let name = format!("{first_name} {last_name} ({short_name})");

        // (Optimal) laptime
        let laptime = format_duration(Duration::from_millis(time_ms.try_into().unwrap()));
        // For a single lap, the splits added up can deviate by
        // a 1ms from the time for the full lap. This is due to
        // rounding errors. We'll just use the laptime as the
        // optimal laptime if the splits are all from the
        // current lap, to avoid the weird off by 1 value.
        let optimal_laptime = if splits == best_splits {
            laptime.clone()
        } else {
            let optimal_laptime = best_splits.iter().sum();
            format_duration(optimal_laptime)
        };

        // Gap and interval
        let lap_duration = Duration::from_millis(time_ms.try_into().unwrap());
        let gap = if let Some(fastest_time) = fastest_time {
            let diff = lap_duration - *fastest_time;
            format_duration(diff)
        } else {
            *fastest_time = Some(lap_duration);
            String::new()
        };
        let interval = previous_time.map_or_else(String::new, |previous_time| {
            let diff = lap_duration - previous_time;
            format_duration(diff)
        });
        *previous_time = Some(lap_duration);

        // (Optimal) splits
        let splits = splits.iter().copied().map(format_duration).collect();
        let best_splits = best_splits.iter().copied().map(format_duration).collect();

        // Car
        let car = (*CAR_MODEL_ID_TO_NAME
            .get(&(model.try_into().unwrap()))
            .unwrap_or(&"Unknown"))
        .to_string();

        // Flag & country name
        let natl = nationality;
        let flag_code = natl
            .and_then(|n| NATIONALITY_TO_ISO.get(&n))
            .unwrap_or(&"xx");
        let flag_name = natl
            .and_then(|n| NATIONALITY_TO_COUNTRY.get(&n))
            .unwrap_or(&"Unknown");

        // Combine it all together
        Self {
            steam_id,
            name,
            flag_code,
            flag_name,
            laptime,
            optimal_laptime,
            gap,
            interval,
            splits,
            best_splits,
            car,
            timestamp,
            laps_valid,
            laps_total,
        }
    }
}

type DisplayData = Vec<(String, Vec<DisplayLine>)>;

#[derive(Template)]
#[template(path = "root.html")]
struct RootTemplate {
    display_data: DisplayData,
}

struct FastestLapQueryRow {
    track: String,
    steam_id: i64,
    first_name: String,
    last_name: String,
    short_name: String,
    nationality: Option<i64>,
    time_ms: i64,
    model: i64,
    timestamp: i64,
    sector_time_ms: i64,
}

pub(crate) async fn handler(extract::State(state): extract::State<State>) -> impl IntoResponse {
    let mut conn = state.0.pool.acquire().await.unwrap();

    let fastest_laps_data = get_fastest_laps_data(&mut conn).await;

    let best_splits_data = get_best_splits(&mut conn).await;

    let laps_data = get_lap_counts(&mut conn).await;

    let display_data = fastest_laps_data
        .into_iter()
        .group_by(|row| row.track.clone())
        .into_iter()
        .map(|(track, rows)| {
            // Replace underscore with space and capitalize every first letter of each word in the trackname
            let display_track = track_id_to_display_name(&track);
            let mut fastest_time = None;
            let mut previous_time = None;
            let display_lines = rows
                .into_iter()
                .group_by(|row| {
                    (
                        row.steam_id,
                        row.first_name.clone(),
                        row.last_name.clone(),
                        row.short_name.clone(),
                        row.nationality,
                        row.time_ms,
                        row.model,
                        row.timestamp,
                    )
                })
                .into_iter()
                .map(
                    |(
                        (
                            steam_id,
                            first_name,
                            last_name,
                            short_name,
                            nationality,
                            time_ms,
                            model,
                            timestamp,
                        ),
                        rows,
                    )| {
                        // Prepare (optimal) sector times
                        let splits = rows
                            .into_iter()
                            .map(|row| {
                                Duration::from_millis(row.sector_time_ms.try_into().unwrap())
                            })
                            .collect::<Vec<_>>();
                        let best_splits = best_splits_data.get(&(track.clone(), steam_id)).unwrap();

                        let (laps_valid, laps_total) =
                            laps_data.get(&(track.clone(), steam_id)).unwrap();

                        DisplayLine::new(
                            steam_id,
                            &first_name,
                            &last_name,
                            &short_name,
                            nationality,
                            time_ms,
                            model,
                            timestamp,
                            &splits,
                            best_splits,
                            &mut fastest_time,
                            &mut previous_time,
                            *laps_valid,
                            *laps_total,
                        )
                    },
                )
                .collect();
            (display_track, display_lines)
        })
        .collect::<DisplayData>();
    RootTemplate { display_data }
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

async fn get_fastest_laps_data(conn: &mut SqliteConnection) -> Vec<FastestLapQueryRow> {
    // Fastest laps for all drivers on all tracks
    sqlx::query_as!(
        FastestLapQueryRow,
        r#"
        SELECT s.track,
            p.steam_id,
            p.first_name,
            p.last_name, 
            p.short_name,
            p.nationality,
            l.time_ms,
            c.model,
            s.timestamp,
            sp.time_ms AS sector_time_ms 
        FROM sessions s
        INNER JOIN laps l ON s.id = l.session_id
        INNER JOIN splits sp ON l.id = sp.lap_id
        INNER JOIN cars c ON l.car_id = c.id
        INNER JOIN players p ON l.steam_id = p.steam_id
        -- Subquery needed to find the fastest valid lap for each player on each track.
        -- We need to use LIMIT, so subquery it is.
        WHERE l.id = (SELECT sl.id
                      FROM laps sl
                      INNER JOIN sessions ss ON sl.session_id = ss.id
                      WHERE ss.track = s.track AND sl.steam_id = l.steam_id AND sl.valid = 1
                      ORDER BY sl.time_ms, ss.timestamp
                      LIMIT 1)
        -- Valid lap is superflous here, but it's a good habit to include it
        AND l.valid = 1
        ORDER BY s.track, l.time_ms;
    "#
    )
    .fetch_all(conn)
    .await
    .unwrap()
}

async fn get_best_splits(conn: &mut SqliteConnection) -> HashMap<(String, i64), Vec<Duration>> {
    sqlx::query!(
        r#"
        SELECT s.track,
            l.steam_id,
            sp.sector,
            MIN(sp.time_ms) AS time_ms
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
            .map(|row| Duration::from_millis(row.time_ms.try_into().unwrap()))
            .collect::<Vec<_>>();
        // unwrap() because SQLx thinks it's possible to have a NULL track or
        // steam_id. This isn't the case, but if this changes, just remove
        // these unwraps.
        ((track.unwrap(), steam_id.unwrap()), best_sectors)
    })
    .collect::<HashMap<_, _>>()
}

async fn get_lap_counts(conn: &mut SqliteConnection) -> HashMap<(String, i64), (i32, i32)> {
    // Get valid and total laps for each player for each track
    sqlx::query!(
        r#"
        SELECT s.track,
            l.steam_id,
            COUNT(1) FILTER (WHERE l.valid = 1) AS valid_laps,
            COUNT(1) AS total_laps
        FROM sessions s
        INNER JOIN laps l ON s.id = l.session_id
        GROUP BY s.track, l.steam_id;
        "#
    )
    .fetch_all(conn)
    .await
    .unwrap()
    .into_iter()
    .map(|row| {
        let track = row.track;
        let steam_id = row.steam_id;
        let valid_laps = row.valid_laps;
        let total_laps = row.total_laps;
        ((track, steam_id), (valid_laps, total_laps))
    })
    .collect::<HashMap<_, _>>()
}
