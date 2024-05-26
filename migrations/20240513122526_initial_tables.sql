CREATE TABLE known_files (
    path TEXT PRIMARY KEY NOT NULL
);

CREATE TABLE sessions (
    id INTEGER PRIMARY KEY NOT NULL,
    track TEXT NOT NULL,
    type TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    server_name TEXT NOT NULL,
    wet INTEGER NOT NULL,
    UNIQUE (timestamp, server_name)
);

CREATE TABLE drivers (
    steam_id INTEGER PRIMARY KEY NOT NULL,
    first_name TEXT NOT NULL,
    last_name TEXT NOT NULL,
    short_name TEXT NOT NULL,
    nickname TEXT,
    nationality INTEGER
);

CREATE TABLE cars (
    id INTEGER PRIMARY KEY NOT NULL,
    session_id INTEGER NOT NULL,
    race_number INTEGER NOT NULL,
    model INTEGER NOT NULL,
    cup_category INTEGER NOT NULL,
    car_group TEXT NOT NULL,
    team_name TEXT,
    ballast_kg INTEGER,
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX cars_session_id_idx ON cars(session_id);

CREATE TABLE laps (
    id INTEGER PRIMARY KEY NOT NULL,
    steam_id INTEGER NOT NULL,
    session_id INTEGER NOT NULL,
    car_id INTEGER NOT NULL,
    time_ms INTEGER NOT NULL,
    valid INTEGER NOT NULL,
    FOREIGN KEY (steam_id) REFERENCES drivers(steam_id),
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE,
    FOREIGN KEY (car_id) REFERENCES cars(id)
);

CREATE INDEX laps_session_id_idx ON laps(session_id);
CREATE INDEX laps_car_id_idx ON laps(car_id);
CREATE INDEX laps_steam_id_idx ON laps(steam_id);

CREATE TABLE splits (
    lap_id INTEGER NOT NULL,
    sector INTEGER NOT NULL,
    time_ms INTEGER NOT NULL,
    FOREIGN KEY (lap_id) REFERENCES laps(id) ON DELETE CASCADE,
    PRIMARY KEY (lap_id, sector)
);
