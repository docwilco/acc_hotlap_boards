use super::State;
use askama_axum::Template;
use axum::{extract, response::IntoResponse};
use itertools::Itertools;
use std::collections::HashMap;
use std::time::Duration;

use super::format_duration;
use super::CAR_MODEL_ID_TO_NAME;
use super::NATIONALITY_TO_COUNTRY;
use super::NATIONALITY_TO_ISO;

#[derive(Template)]
#[template(path = "root.html")]
struct RootTemplate {
    display_data: DisplayData,
}

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
    optimal_splits: Vec<String>,
    car: String,
    date: i64,
    laps_valid: i32,
    laps_total: i32,
}

type DisplayData = Vec<(String, Vec<DisplayLine>)>;

pub(crate) async fn handler(extract::State(state): extract::State<State>) -> impl IntoResponse {
    let mut conn = state.0.pool.acquire().await.unwrap();

    // Fastest laps for all drivers on all tracks
    let data = sqlx::query!(
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
            sp.sector,
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
    .fetch_all(&mut *conn)
    .await
    .unwrap();

    let optimal_sectors_data = sqlx::query!(
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
    .fetch_all(&mut *conn)
    .await
    .unwrap()
    .into_iter()
    .group_by(|row| (row.track.clone(), row.steam_id))
    .into_iter()
    .map(|((track, steam_id), rows)| {
        let optimal_sectors = rows
            .into_iter()
            .map(|row| Duration::from_millis(row.time_ms.try_into().unwrap()))
            .collect::<Vec<_>>();
        // unwrap() because SQLx thinks it's possible to have a NULL track or
        // steam_id. This isn't the case, but if this changes, just remove
        // these unwraps.
        ((track.unwrap(), steam_id.unwrap()), optimal_sectors)
    })
    .collect::<HashMap<_, _>>();

    // Get valid and total laps for each player for each track
    let laps_data = sqlx::query!(
        r#"
        SELECT s.track,
            l.steam_id,
            COUNT(1) AS total_laps,
            COUNT(1) FILTER (WHERE l.valid = 1) AS valid_laps
        FROM sessions s
        INNER JOIN laps l ON s.id = l.session_id
        GROUP BY s.track, l.steam_id;
        "#
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap()
    .into_iter()
    .map(|row| {
        let track = row.track;
        let steam_id = row.steam_id;
        let total_laps = row.total_laps;
        let valid_laps = row.valid_laps;
        ((track, steam_id), (total_laps, valid_laps))
    })
    .collect::<HashMap<_, _>>();

    let display_data = data
        .into_iter()
        .group_by(|row| row.track.clone())
        .into_iter()
        .map(|(track, rows)| {
            // Replace underscore with space and capitalize every first letter of each word in the trackname
            let display_track = track
                .replace('_', " ")
                .split_whitespace()
                .map(|word| {
                    let mut chars = word.chars();
                    match chars.next() {
                        Some(c) => c.to_uppercase().chain(chars).collect::<String>(),
                        None => String::new(),
                    }
                })
                .join(" ");
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
                        let splits_times_ms = rows
                            .into_iter()
                            .map(|row| {
                                Duration::from_millis(row.sector_time_ms.try_into().unwrap())
                            })
                            .collect::<Vec<_>>();
                        let optimal_splits_times_ms = optimal_sectors_data
                            .get(&(track.clone(), steam_id))
                            .unwrap();

                        // Name
                        let name = format!("{first_name} {last_name} ({short_name})");

                        // (Optimal) laptime
                        let laptime =
                            format_duration(Duration::from_millis(time_ms.try_into().unwrap()));
                        // For a single lap, the splits added up can deviate by
                        // a 1ms from the time for the full lap. This is due to
                        // rounding errors. We'll just use the laptime as the
                        // optimal laptime if the splits are all from the
                        // current lap, to avoid the weird off by 1 value.
                        let optimal_laptime = if &splits_times_ms == optimal_splits_times_ms {
                            laptime.clone()
                        } else {
                            let optimal_laptime = optimal_splits_times_ms.iter().sum::<Duration>();
                            format_duration(optimal_laptime)
                        };

                        // Gap and interval
                        let lap_duration = Duration::from_millis(time_ms.try_into().unwrap());
                        let gap = if let Some(fastest_time) = fastest_time {
                            let diff = lap_duration - fastest_time;
                            format_duration(diff)
                        } else {
                            fastest_time = Some(lap_duration);
                            String::new()
                        };
                        let interval = previous_time.map_or_else(String::new, |previous_time| {
                            let diff = lap_duration - previous_time;
                            format_duration(diff)
                        });
                        previous_time = Some(lap_duration);

                        // (Optimal) splits
                        let splits = splits_times_ms.into_iter().map(format_duration).collect();
                        let optimal_splits = optimal_splits_times_ms
                            .iter()
                            .copied()
                            .map(format_duration)
                            .collect();

                        // Car
                        let car = (*CAR_MODEL_ID_TO_NAME
                            .get(&(model.try_into().unwrap()))
                            .unwrap_or(&"Unknown"))
                        .to_string();

                        // Date/time
                        let datetime = timestamp;

                        // Flag & country name
                        let natl = nationality;
                        let flag_code = natl
                            .and_then(|n| NATIONALITY_TO_ISO.get(&n))
                            .unwrap_or(&"xx");
                        let flag_name = natl
                            .and_then(|n| NATIONALITY_TO_COUNTRY.get(&n))
                            .unwrap_or(&"Unknown");

                        // Laps numbers
                        let (laps_total, laps_valid) = laps_data
                            .get(&(track.clone(), steam_id))
                            .unwrap()
                            .to_owned();

                        // Combine it all together
                        DisplayLine {
                            steam_id,
                            name,
                            flag_code,
                            flag_name,
                            laptime,
                            optimal_laptime,
                            gap,
                            interval,
                            splits,
                            optimal_splits,
                            car,
                            date: datetime,
                            laps_valid,
                            laps_total,
                        }
                    },
                )
                .collect();
            (display_track, display_lines)
        })
        .collect::<DisplayData>();
    RootTemplate { display_data }
}
