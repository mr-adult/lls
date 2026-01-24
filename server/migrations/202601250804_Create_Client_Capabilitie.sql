CREATE TABLE IF NOT EXISTS sessions (
    id BIGSERIAL PRIMARY KEY
);

CREATE TABLE IF NOT EXISTS log_level (
    id INTEGER PRIMARY KEY
    , name TEXT UNIQUE NOT NULL
);

INSERT INTO log_level (id, name)
SELECT new_values.new_id AS id
    , new_values.new_value AS name
FROM (
    (
        SELECT 0 AS new_id, 'Trace' AS new_value
        UNION ALL
        SELECT 1 AS new_id, 'Debug' AS new_value
        UNION ALL
        SELECT 2 AS new_id, 'Info' AS new_value
        UNION ALL
        SELECT 3 AS new_id, 'Warn' AS new_value
        UNION ALL
        SELECT 4 AS new_id, 'Error' AS new_value
    ) new_values
    LEFT OUTER JOIN log_level ON log_level.id = new_values.new_id
)
WHERE id IS NULL;

CREATE TABLE IF NOT EXISTS logs (
    id BIGSERIAL PRIMARY KEY
    , time_stamp TIMESTAMPTZ NOT NULL
    , session_id BIGINT REFERENCES sessions(id)
    , message TEXT NOT NULL
    , fields JSON
);

CREATE TABLE IF NOT EXISTS log_spans (
    id BIGSERIAL PRIMARY KEY
    , index INTEGER NOT NULL
    , name TEXT NOT NULL
    , level INTEGER NOT NULL REFERENCES log_level (id)
    , fields JSON
);

CREATE TABLE IF NOT EXISTS sources (
    id INTEGER PRIMARY KEY
    , value TEXT UNIQUE NOT NULL
);

INSERT INTO sources (id, value)
SELECT new_values.new_id AS id
    , new_values.new_value AS value
FROM (
    (
        SELECT 0 AS new_id, 'client' AS new_value
        UNION ALL
        SELECT 1 AS new_id, 'server' AS new_value
    ) new_values
    LEFT OUTER JOIN sources ON sources.id = new_values.new_id
)
WHERE id IS NULL;

/* logging for the JSON RPC 2.0 protocol layer */
CREATE TABLE IF NOT EXISTS requests (
    id BIGSERIAL PRIMARY KEY
    , request_id TEXT NOT NULL
    , session_id BIGINT NOT NULL REFERENCES sessions(id)
    , method TEXT NOT NULL
    , params JSON NOT NULL
    , time_stamp TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS responses (
    id BIGSERIAL PRIMARY KEY REFERENCES requests(id)
    , session_id BIGINT NOT NULL REFERENCES sessions(id)
    , is_error BOOLEAN NOT NULL
    , result JSON CHECK (CASE WHEN is_error = FALSE THEN result IS NOT NULL ELSE 1=1 END)
    , error_code INTEGER CHECK (CASE WHEN is_error = TRUE THEN error_code IS NOT NULL ELSE 1=1 END)
    , error_message TEXT CHECK (CASE WHEN is_error = TRUE THEN error_message IS NOT NULL ELSE 1=1 END)
    , error_data JSON CHECK (CASE WHEN is_error = TRUE THEN 1=1 ELSE error_data IS NULL END)
    , time_stamp TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS notifications (
    id BIGSERIAL PRIMARY KEY
    , session_id BIGINT NOT NULL REFERENCES sessions(id)
    , method TEXT NOT NULL
    , params JSON CHECK (params IS NOT NULL AND (params IS JSON ARRAY OR params IS JSON object))
    , time_stamp TIMESTAMPTZ NOT NULL
    , source INTEGER NOT NULL REFERENCES sources(id)
);
