ALTER TABLE instances ADD COLUMN version INTEGER NOT NULL DEFAULT 0 CHECK(version >= 0);
ALTER TABLE instances ADD COLUMN last_error TEXT;
CREATE TABLE instance_volumes (
    instance_id TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    volume_name TEXT NOT NULL REFERENCES volumes(name) ON DELETE RESTRICT,
    target TEXT NOT NULL,
    read_only INTEGER NOT NULL CHECK(read_only IN (0,1)),
    PRIMARY KEY(instance_id,target)
);
CREATE INDEX instance_volume_references ON instance_volumes(volume_name);
CREATE TABLE volume_deletions (
    name TEXT PRIMARY KEY,
    requested_at_ms INTEGER NOT NULL
);
CREATE INDEX instance_events ON events(instance_id,sequence);
PRAGMA user_version = 2;
