use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;
use serde_with::{serde_as, DurationMilliSeconds};
use std::time::Duration;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Driver {
    pub first_name: String,
    pub last_name: String,
    pub short_name: String,
    #[serde(deserialize_with = "player_id_to_steam_id")]
    pub steam_id: i64,
}

#[allow(clippy::struct_field_names)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Car {
    pub car_id: i64,
    pub race_number: i64,
    pub car_model: i64,
    pub cup_category: i64,
    pub car_group: String,
    pub team_name: String,
    //pub nationality: i64,
    //car_guid: i64,
    //team_guid: i64,
    pub drivers: Vec<Driver>,
    pub ballast_kg: Option<i64>,
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timing {
    //#[serde_as(as = "DurationMilliSeconds<u64>")]
    //last_lap: Duration,
    //#[serde_as(as = "Vec<DurationMilliSeconds<f64>>")]
    //last_splits: Vec<Duration>,
    //#[serde_as(as = "DurationMilliSeconds<u64>")]
    //best_lap: Duration,
    //#[serde_as(as = "Vec<DurationMilliSeconds<f64>>")]
    //best_splits: Vec<Duration>,
    //#[serde_as(as = "DurationMilliSeconds<u64>")]
    //total_time: Duration,
    //lap_count: u64,
    //last_split_id: u64,
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaderBoardLine {
    pub car: Car,
    //current_driver: Driver,
    //current_driver_index: u64,
    //timing: Timing,
    //missing_mandatory_pitstop: i64,
    //#[serde_as(as = "Vec<DurationMilliSeconds<f64>>")]
    //driver_total_times: Vec<Duration>,
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResult {
    //#[serde_as(as = "DurationMilliSeconds<u64>")]
    //bestlap: Duration,
    //#[serde_as(as = "Vec<DurationMilliSeconds<u64>>")]
    //best_splits: Vec<Duration>,
    pub is_wet_session: i64,
    //#[serde(rename = "type")]
    //session_type: u64,
    pub leader_board_lines: Vec<LeaderBoardLine>,
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lap {
    pub car_id: i64,
    pub driver_index: i64,
    #[serde_as(as = "DurationMilliSeconds<u64>")]
    pub laptime: Duration,
    pub is_valid_for_best: bool,
    #[serde_as(as = "Vec<DurationMilliSeconds<u64>>")]
    pub splits: Vec<Duration>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Penalty {
    //car_id: u64,
    //driver_index: u64,
    //reason: String,
    //penalty: String,
    //penalty_value: u64,
    //violation_in_lap: i64,
    //cleared_in_lap: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResults {
    pub session_type: String,
    pub track_name: String,
    //session_index: u64,
    //race_weekend_index: i64,
    pub server_name: String,
    pub session_result: SessionResult,
    pub laps: Vec<Lap>,
    //penalties: Vec<Penalty>,
    //post_race_penalties: Option<Vec<Penalty>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryListDriver {
    pub first_name: String,
    pub last_name: String,
    pub short_name: String,
    pub nick_name: Option<String>,
    #[serde(rename = "playerID", deserialize_with = "player_id_to_steam_id")]
    pub steam_id: i64,
    pub nationality: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub drivers: Vec<EntryListDriver>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryList {
    pub entries: Vec<Entry>,
}

#[derive(Clone, Copy)]
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
        Encoding::Utf8 => dedup_json(std::str::from_utf8(bytes)?)?,
        Encoding::Utf16Le => dedup_json(&String::from_utf16le(bytes)?)?,
        Encoding::Utf16Be => dedup_json(&String::from_utf16be(bytes)?)?,
    })
}

#[allow(clippy::cast_precision_loss)]
pub fn bytes_to_json_string(bytes: &[u8]) -> Result<String> {
    // Check for BOM
    if bytes[0..3] == [0xEF, 0xBB, 0xBF] {
        // UTF-8 BOM
        return dedup_json(std::str::from_utf8(&bytes[3..])?);
    } else if bytes[0..2] == [0xFF, 0xFE] {
        return dedup_json(&String::from_utf16le(&bytes[2..])?);
    } else if bytes[0..2] == [0xFE, 0xFF] {
        return dedup_json(&String::from_utf16be(&bytes[2..])?);
    }

    // No BOM
    // Check for lots of BE NUL bytes
    let (be_nuls, le_nuls) =
        bytes
            .chunks(2)
            .fold((0_usize, 0_usize), |(be_nuls, le_nuls), chunk| match chunk {
                [0, 0] => (be_nuls + 1, le_nuls + 1),
                [0, _] => (be_nuls + 1, le_nuls),
                [_, 0] => (be_nuls, le_nuls + 1),
                _ => panic!("chunk size not 2?!"),
            });
    // Let's say if 45+% of the bytes are BE NULs, then it's probably UTF-16BE
    if be_nuls as f64 >= (0.45 * (bytes.len() as f64)) {
        if let Ok(text) = try_conversion(bytes, Encoding::Utf16Be) {
            return Ok(text);
        }
    }

    // Same for LE NULs
    if le_nuls as f64 >= (0.45 * (bytes.len() as f64)) {
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

// Player ID is basically SteamID with an 'S' prefix. Throughout this code,
// we'll refer to SteamID as `steam_id`, and we constrain the existence of
// Player ID to just this function.
pub fn player_id_to_steam_id<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let player_id = String::deserialize(deserializer)?;
    let mut chars = player_id.chars();
    if chars.next() != Some('S') {
        return Err(serde::de::Error::custom(format!("Invalid player ID: {player_id}")));
    }
    let steam_id = chars.collect::<String>().parse::<i64>().map_err(serde::de::Error::custom)?;
    Ok(steam_id)
}