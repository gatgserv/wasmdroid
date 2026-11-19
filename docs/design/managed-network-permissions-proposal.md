# Managed application network grants: read-only source contract

2026-11-17. Proposal, no production edits/builds/device actions. Scope choice is
explicit UI/product review; it is not inferred from a recipe or existing labels.

1. Recommend a private durable operator-grant table, separate from StackPlan and
   Compose, rather than making positive recipe/plan fields into operator consent.
2. Current source has **no `StackPlan.template_digest`**. Compute any private review
   fingerprint from actual current native definitions/specs/membership; do not
   claim an existing field or accept a UI-provided raw plan as grant authority.
3. Keep `StackPlan::validate` rejection of positive Internet/LAN fields intact;
   catalog/template creation stays off and untrusted imported plans cannot grant.
4. Read API `group_network_permission_source(ApplicationTarget)` resolves actual
   group birth, stack version, current member IDs/versions/private profile IDs and
   grant revision in one native snapshot. Return bounded names/count/mixed flags
   plus opaque in-memory review token, never args/env/commands/raw plan JSON.
5. Write API `set_group_network_permissions(target,review_token,internet,lan)`
   applies only the explicitly reviewed **all-current-members** set (1–8). If a
   per-member UI is chosen instead, add a typed reviewed selection, not caller-
   supplied arbitrary member IDs. The all-current scope is stated in the
   [next English screen scheme](managed-network-permission-screen.md).
6. Proposed rows key `(group_birth,service,private_profile_id)` with effective
   flags, monotonic grant epoch, active/revoked state and private semantic pin.
   Binding only group ID, labels or template hash is insufficient against ABA.
7. Single SQLite transaction re-resolves exact birth/stack/member revisions,
   requires every selected member stopped, checks existing authority and writes
   the complete grant set atomically. Advance group/control grant revision so
   old permission/edit/shortcut state becomes stale; do not rewrite dataset bytes,
   private browser identity, actual member ID or any unrelated specification field.
8. Common owner additionally fences group starts/edits, all relevant jobs/data
   owners and close lifetime throughout the retained actual IO future. Caller
   observer cancellation cannot release the reservation before the real ACK.
9. Grants authorize only the actual reviewed identities, not future services.
   Added/replaced/recreated members start off. Exact same-spec Dead retry may
   retain the row by stable private profile identity, while verifying membership.
10. Definition/spec/data-mount semantic changes need explicit invalidation rules.
    Recommend revoking affected grants monotonically and requiring new review;
    returning to old bytes/hash must not resurrect an old grant. A digest mismatch
    alone is not irreversible revocation. No-op definition edits need not revoke.
11. Keep actual managed InstanceSpec/template flags neutral; resolve effective
    permissions from native grant storage into a sealed startup authority. Do not
    mirror positive flags into plans and silently weaken export/diff checks.
12. Extend the private effective-permission lookup for worker launch and FFI policy
    construction; Android private request hints only request host capture, and the
    native factory re-reads actual grant/birth/profile/epoch/spec before admission.
13. Existing `EgressScope::capture` requires desired Starting run authority, but
    `begin_authorized_run` rejects managed members and `Supervisor.startStack`
    creates no such authority. Add an explicit native **manual managed run** batch
    tied to actual group/current members/generations; do not remove the guard or
    invent group automatic restart/boot authorization.
14. Recheck the sealed grant through preparation, before spawn and actual numeric
    socket/DNS admission; revoke on real Stop/grant/context loss, with existing
    retained provider/stream ownership. Old no-cap entry points must remain closed
    when effective managed grants are positive, even though template flags are off.
15. Current export re-parses strict Compose and compares actual full specs; preview
    update compares full specs too. Separate grants leave these lossless checks
    intact. Export/import must clearly say permissions stay on this phone and
    import/create off; grant metadata is not part of Compose/catalog/volume backup.
16. Candidate source files: new state/group_network_permissions.rs +migration007;
    state/stacks.rs, stack_updates.rs and any spec/mount-rebind write path for
    identity/semantic invalidation; state/automation.rs for scoped manual batch;
    private FFI effective-source/startup wrappers; driver EgressScope/Prepared
    handling; Supervisor group admission/lifetime; typed permission controller/UI.
17. UI proposal: “Network permissions for all current services”, list current
    service names/count, two independent permissions and next-run effect. Show
    All/None/Mixed truthfully after changes; never turn Mixed into an implicit
    grant. Save explicitly says it applies to these N current services, with a
    separate future-service/replacement-off explanation. Per-member scope is an
    alternative scheme, not a silently assumed product policy.
18. Native tests: atomic rollback on later member failure; stale group birth,
    stack/member/profile/grant revision and membership replacement; running/data
    reservation rejection; unrelated specs/volumes/history unchanged; no recipe
    inheritance; new/replaced members off; exact retry continuity; semantic change
    and return-to-old-bytes cannot revive grants; export/no-op/update round-trip.
19. Runtime/controller tests: actual manual managed authority required; missing/
    forged/stale receipt stays closed; no-cap old constructor closed; current
    flags independently applied; old dialog/source callbacks cannot save a new
    selection; mixed flags/all-current scope and IO observer cancellation truthful.
20. The next implementation follows the English screen scheme. Current managed
    denial stays until private grants and actual managed startup authority are
    implemented. Actual multi-service Android Internet/LAN/DNS, stopping one member,
    profile/data preservation and default/VPN transition acceptance remain later.

Evidence locations: engine/src/stacks/mod.rs (`StackPlan`, `validate`);
engine/src/stacks/update.rs (`changes`, `check_current`, `export_current`);
engine/src/state/stacks.rs (`stack`, `retry_stack_member`);
engine/src/state/stack_updates.rs (`publish_stack_update`, `initial_plan_json`);
engine/src/state/applications.rs (group birth/actual target);
engine/src/state/automation.rs (`begin_authorized_run` managed rejection);
engine/drivers/wasmer-local/src/service_egress.rs (native Starting authority);
android/.../engine/ServiceSupervisor.kt (`startStack`/actual reservations).
