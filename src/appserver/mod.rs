use anyhow::{anyhow, Context};
use axum::{routing::get, Router};
use include_dir::{include_dir, Dir};
use phf::{phf_map, Map};
use sqlx::SqlitePool;
use std::{env, sync::Arc, time::Duration};
use tokio::net::TcpListener;
use tower_serve_static::ServeDir;

mod rootpage;

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

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let minutes = seconds / 60;
    let seconds = seconds % 60;
    let milliseconds = duration.subsec_millis();
    if minutes == 0 {
        format!("{seconds}.{milliseconds:03}")
    } else {
        format!("{minutes}:{seconds:02}.{milliseconds:03}")
    }
}

struct StateInner {
    pool: SqlitePool,
}

#[derive(Clone)]
struct State(Arc<StateInner>);

static STATIC_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/static");

pub async fn run(pool: SqlitePool) -> Result<(), anyhow::Error> {
    let state = State(Arc::new(StateInner { pool }));

    let bind_address = env::var("BIND_ADDRESS").unwrap_or_else(|_| "127.0.0.1:3000".to_string());

    let app = Router::new()
        .route("/", get(rootpage::handler))
        .with_state(state.clone())
        .nest_service("/static", ServeDir::new(&STATIC_DIR));
    let listener = TcpListener::bind(&bind_address)
        .await
        .context(anyhow!("Failed to bind to {bind_address}"))?;
    axum::serve(listener, app)
        .await
        .context(anyhow!("Failed to start server"))?;
    Ok(())
}
