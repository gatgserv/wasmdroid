BEGIN IMMEDIATE;
CREATE TABLE stacks (
    id TEXT PRIMARY KEY,
    endpoint_id TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 0 CHECK(version >= 0),
    plan_json TEXT NOT NULL
);
CREATE TABLE stack_services (
    stack_id TEXT NOT NULL REFERENCES stacks(id) ON DELETE CASCADE,
    service_name TEXT NOT NULL,
    instance_id TEXT NOT NULL UNIQUE REFERENCES instances(id) ON DELETE RESTRICT,
    position INTEGER NOT NULL CHECK(position >= 0),
    PRIMARY KEY(stack_id,service_name),
    UNIQUE(stack_id,position)
);
CREATE TABLE stack_volumes (
    stack_id TEXT NOT NULL REFERENCES stacks(id) ON DELETE CASCADE,
    logical_name TEXT NOT NULL,
    volume_name TEXT NOT NULL REFERENCES volumes(name) ON DELETE RESTRICT,
    external INTEGER NOT NULL CHECK(external IN (0,1)),
    PRIMARY KEY(stack_id,logical_name)
);
CREATE INDEX stack_volume_references ON stack_volumes(volume_name);
PRAGMA user_version = 3;
COMMIT;
