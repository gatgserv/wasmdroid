BEGIN IMMEDIATE;
CREATE TABLE group_network_revisions (
    group_id TEXT PRIMARY KEY REFERENCES stacks(id) ON DELETE CASCADE,
    birth TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(typeof(revision)='integer' AND revision>0)
);
CREATE TABLE group_network_grants (
    group_id TEXT NOT NULL REFERENCES group_network_revisions(group_id) ON DELETE CASCADE,
    service TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    internet INTEGER NOT NULL CHECK(internet IN (0,1)),
    lan INTEGER NOT NULL CHECK(lan IN (0,1)),
    active INTEGER NOT NULL CHECK(active IN (0,1)),
    epoch INTEGER NOT NULL CHECK(typeof(epoch)='integer' AND epoch>0),
    spec_digest TEXT NOT NULL,
    PRIMARY KEY(group_id,service,profile_id)
);
-- Covers every actual specification/mount rebind path, including direct data
-- restoration writes. Returning to old bytes must never resurrect consent.
-- Exact unchanged writes and same-spec retry (identity move, no spec UPDATE)
-- retain consent. No specification, identity or dataset is changed here.
CREATE TRIGGER revoke_group_network_on_spec_change
AFTER UPDATE OF spec_json ON instances WHEN OLD.spec_json IS NOT NEW.spec_json
BEGIN
    UPDATE group_network_revisions SET revision=revision+1
        WHERE group_id IN (SELECT group_id FROM group_network_grants
            WHERE active=1 AND profile_id=(SELECT profile_id FROM instance_browser_identity WHERE instance_id=NEW.id));
    UPDATE group_network_grants SET active=0,epoch=epoch+1
        WHERE active=1 AND profile_id=(SELECT profile_id FROM instance_browser_identity WHERE instance_id=NEW.id);
END;
PRAGMA user_version = 7;
COMMIT;
