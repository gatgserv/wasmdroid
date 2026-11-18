BEGIN IMMEDIATE;
CREATE TABLE instance_automation (
    instance_id TEXT PRIMARY KEY REFERENCES instances(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK(revision > 0),
    desired_run INTEGER NOT NULL CHECK(desired_run IN (0,1)),
    generation TEXT,
    boot_epoch INTEGER CHECK(boot_epoch IS NULL OR boot_epoch >= 0),
    outcome TEXT NOT NULL CHECK(outcome IN ('idle','starting','running','succeeded','failed','interrupted','stopped'))
);
CREATE TABLE instance_browser_identity (
    instance_id TEXT PRIMARY KEY REFERENCES instances(id) ON DELETE CASCADE,
    profile_id TEXT NOT NULL UNIQUE
);
-- Existing provider profiles were hashed from the actual instance identity.
-- Preserve those exact bytes; future allocations use a disjoint namespace.
INSERT INTO instance_browser_identity(instance_id,profile_id) SELECT id,id FROM instances;
PRAGMA user_version = 5;
COMMIT;
