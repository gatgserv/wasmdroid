BEGIN IMMEDIATE;
CREATE TABLE artifacts (
    reference TEXT PRIMARY KEY, digest TEXT NOT NULL,
    kind TEXT NOT NULL, driver TEXT NOT NULL, metadata_json TEXT NOT NULL
);
CREATE TABLE instances (
    id TEXT PRIMARY KEY, endpoint_id TEXT NOT NULL,
    artifact_ref TEXT NOT NULL REFERENCES artifacts(reference) ON DELETE RESTRICT,
    kind TEXT NOT NULL, driver TEXT NOT NULL, spec_json TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('created','running','paused','restarting','exited','dead')),
    exit_code INTEGER
);
CREATE TABLE volumes (
    name TEXT PRIMARY KEY, kind TEXT NOT NULL, driver TEXT NOT NULL,
    metadata_json TEXT NOT NULL
);
CREATE TABLE port_bindings (
    instance_id TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    endpoint_id TEXT NOT NULL, host_port INTEGER NOT NULL CHECK(host_port BETWEEN 1 AND 65535),
    instance_port INTEGER NOT NULL CHECK(instance_port BETWEEN 1 AND 65535),
    protocol TEXT NOT NULL CHECK(protocol IN ('tcp','udp')),
    kind TEXT NOT NULL, driver TEXT NOT NULL,
    PRIMARY KEY(endpoint_id, host_port, protocol)
);
CREATE TABLE events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT, instance_id TEXT,
    kind TEXT NOT NULL, driver TEXT NOT NULL, timestamp_ms INTEGER NOT NULL, payload_json TEXT NOT NULL
);
PRAGMA user_version = 1;
COMMIT;
