BEGIN IMMEDIATE;
ALTER TABLE stacks ADD COLUMN initial_plan_json TEXT;
CREATE TABLE stack_retained_volumes (
    stack_id TEXT NOT NULL REFERENCES stacks(id) ON DELETE CASCADE,
    volume_name TEXT NOT NULL REFERENCES volumes(name) ON DELETE RESTRICT,
    PRIMARY KEY(stack_id,volume_name)
);
CREATE INDEX retained_stack_volume_references ON stack_retained_volumes(volume_name);
PRAGMA user_version = 4;
COMMIT;
