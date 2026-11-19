BEGIN IMMEDIATE;
-- Private profile history outlives actual instances and provider-data erasure.
-- No cascade/reclamation: an old origin must not become another app's origin.
CREATE TABLE browser_origin_profiles (
    profile_id TEXT PRIMARY KEY,
    tracked_from_birth INTEGER NOT NULL CHECK(tracked_from_birth IN (0,1))
);
INSERT INTO browser_origin_profiles(profile_id,tracked_from_birth)
    SELECT profile_id,0 FROM instance_browser_identity;
CREATE TABLE browser_origin_assignments (
    profile_id TEXT NOT NULL REFERENCES browser_origin_profiles(profile_id),
    guest_port INTEGER NOT NULL CHECK(guest_port BETWEEN 1 AND 65535),
    host_port INTEGER NOT NULL UNIQUE CHECK(host_port BETWEEN 1 AND 65535),
    PRIMARY KEY(profile_id,guest_port)
);
-- Only currently persisted local TCP mappings can be recovered. Past stopped/
-- removed mappings were not stored; never invent historical origin coverage.
INSERT INTO browser_origin_assignments(profile_id,guest_port,host_port)
    SELECT b.profile_id,p.instance_port,p.host_port
    FROM port_bindings p JOIN instance_browser_identity b ON b.instance_id=p.instance_id
    WHERE p.endpoint_id='local' AND p.protocol='tcp';
PRAGMA user_version = 6;
COMMIT;
