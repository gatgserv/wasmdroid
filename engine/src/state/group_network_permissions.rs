//! Explicit all-listed operator grants, separate from recipes and raw specs.
//! Opaque reviews never retain runtime ownership or serialize private specs.
use super::lifecycle::event;
use super::*;
use crate::{
    api::{InstanceSpec, InstanceState, NetworkMode},
    store::Digest,
};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupPermissionChoice {
    Off,
    Allow,
    Mixed,
}
#[derive(Serialize)]
pub struct GroupNetworkMember {
    pub id: String,
    pub identity: String,
    pub service: String,
    pub name: String,
    pub version: u64,
    pub state: InstanceState,
    pub internet: bool,
    pub lan: bool,
}
#[derive(Serialize)]
pub struct GroupNetworkSource {
    pub target: ApplicationTarget,
    pub name: String,
    pub version: u64,
    pub grant_revision: u64,
    pub members: Vec<GroupNetworkMember>,
    pub internet: GroupPermissionChoice,
    pub lan: GroupPermissionChoice,
    pub can_edit: bool,
    pub network_enabled: bool,
}
impl std::fmt::Debug for GroupNetworkSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GroupNetworkSource(private)")
    }
}
#[derive(Clone, PartialEq, Eq)]
pub(super) struct GrantSnapshot {
    pub group: String,
    pub birth: String,
    pub service: String,
    pub profile: String,
    pub epoch: u64,
    pub active: bool,
    pub internet: bool,
    pub lan: bool,
    pub digest: String,
}
#[derive(PartialEq, Eq)]
struct ReviewMember {
    id: String,
    profile: String,
    service: String,
    version: u64,
    state: InstanceState,
    specification: String,
    authority: String,
    grant: GrantSnapshot,
}
pub struct GroupNetworkReview {
    database: PathBuf,
    target: ApplicationTarget,
    version: u64,
    grant_revision: u64,
    members: Vec<ReviewMember>,
    source: GroupNetworkSource,
}
impl GroupNetworkReview {
    pub fn source(&self) -> &GroupNetworkSource {
        &self.source
    }
    pub(super) fn validate(&self, state: &StateDb) -> Result<Self> {
        let current = state.read_group_network_review(&self.target)?;
        if self.database != current.database
            || self.version != current.version
            || self.grant_revision != current.grant_revision
            || self.members != current.members
        {
            return Err(EngineError::Conflict(
                "listed application services changed; review again".into(),
            ));
        }
        Ok(current)
    }
    pub(super) fn member_ids(&self) -> impl Iterator<Item = &str> {
        self.members.iter().map(|member| member.id.as_str())
    }
}
pub(super) fn sql_revision(value: u64) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| EngineError::Invalid("group permission revision exceeds storage range".into()))
}
pub(super) fn next_revision(value: u64) -> Result<u64> {
    value
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| EngineError::Invalid("group permission revision exhausted".into()))
}
fn stopped(state: InstanceState) -> bool {
    matches!(
        state,
        InstanceState::Created | InstanceState::Exited | InstanceState::Dead
    )
}
fn choice(values: impl Iterator<Item = bool>) -> GroupPermissionChoice {
    let values: Vec<_> = values.collect();
    if values.iter().all(|value| *value) {
        GroupPermissionChoice::Allow
    } else if values.iter().all(|value| !*value) {
        GroupPermissionChoice::Off
    } else {
        GroupPermissionChoice::Mixed
    }
}
impl StateDb {
    pub(super) fn permission_database(&self) -> Result<PathBuf> {
        Ok(std::fs::canonicalize(self.connection.path().ok_or_else(
            || EngineError::Conflict("permission database is unavailable".into()),
        )?)?)
    }
    pub(super) fn managed_grant(
        &self,
        id: &str,
        spec: &InstanceSpec,
    ) -> Result<Option<GrantSnapshot>> {
        let Some(group) = self.stack_for_instance(id)? else {
            return Ok(None);
        };
        if spec.internet_access || spec.lan_access {
            return Err(EngineError::Conflict(
                "managed template permissions are not operator grants".into(),
            ));
        }
        let actual = self.application_target(ApplicationTargetKind::Group, &group)?;
        let stack = self.stack(&group)?;
        let position = stack
            .instances
            .iter()
            .position(|member| member.id == id)
            .ok_or_else(|| EngineError::Conflict("actual managed service changed".into()))?;
        let service = stack.plan.startup_order[position].clone();
        let profile = self.browser_identity(id)?;
        let row=self.connection.query_row(
            "SELECT r.birth,g.epoch,g.active,g.internet,g.lan,g.spec_digest FROM group_network_grants g JOIN group_network_revisions r ON r.group_id=g.group_id WHERE g.group_id=?1 AND g.service=?2 AND g.profile_id=?3",
            params![group,service,profile],|row|Ok((row.get::<_,String>(0)?,row.get::<_,i64>(1)?,row.get::<_,bool>(2)?,row.get::<_,bool>(3)?,row.get::<_,bool>(4)?,row.get::<_,String>(5)?)),
        ).optional()?;
        let mut grant = GrantSnapshot {
            group,
            birth: actual.target.identity_token,
            service,
            profile,
            epoch: 0,
            active: false,
            internet: false,
            lan: false,
            digest: String::new(),
        };
        if let Some((birth, epoch, active, internet, lan, digest)) = row {
            if birth != grant.birth || epoch <= 0 {
                return Err(EngineError::Conflict(
                    "managed permission identity changed".into(),
                ));
            }
            grant.epoch = epoch as u64;
            grant.digest = digest;
            // A mismatched row is never usable. The SQL mutation trigger makes
            // revocation irreversible even if a later edit restores old bytes.
            grant.active = active
                && grant.digest == Digest::of(serde_json::to_string(spec)?.as_bytes()).as_str();
            grant.internet = grant.active && internet;
            grant.lan = grant.active && lan;
        }
        Ok(Some(grant))
    }
    fn read_group_network_review(&self, target: &ApplicationTarget) -> Result<GroupNetworkReview> {
        if target.kind != ApplicationTargetKind::Group {
            return Err(EngineError::Unsupported(
                "choose the actual application group".into(),
            ));
        }
        let actual = self.resolve_application_target(target)?;
        let stack = self.stack(&actual.target.id)?;
        if stack.instances.is_empty() || stack.instances.len() > 8 {
            return Err(EngineError::Conflict("invalid listed service count".into()));
        }
        let revision = self
            .connection
            .query_row(
                "SELECT birth,revision FROM group_network_revisions WHERE group_id=?1",
                [&stack.id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;
        let grant_revision = match revision {
            Some((birth, revision)) if birth == actual.target.identity_token && revision > 0 => {
                revision as u64
            }
            None => 0,
            _ => return Err(EngineError::Conflict("group grant identity changed".into())),
        };
        let mut members = Vec::new();
        let mut displays = Vec::new();
        let mut can_edit = true;
        let mut network_enabled = true;
        for ((service, instance), display) in stack
            .plan
            .startup_order
            .iter()
            .zip(&stack.instances)
            .zip(&actual.members)
        {
            let authority = self.run_authority(&instance.id)?;
            can_edit &= stopped(instance.state)
                && !matches!(
                    authority.outcome,
                    RunOutcome::Starting | RunOutcome::Running
                );
            network_enabled &= instance.spec.network == NetworkMode::Loopback;
            let grant = self
                .managed_grant(&instance.id, &instance.spec)?
                .ok_or_else(|| {
                    EngineError::Conflict("managed service membership changed".into())
                })?;
            displays.push(GroupNetworkMember {
                id: instance.id.clone(),
                identity: grant.profile.clone(),
                service: service.clone(),
                name: display.name.clone(),
                version: instance.version,
                state: instance.state,
                internet: grant.internet,
                lan: grant.lan,
            });
            members.push(ReviewMember {
                id: instance.id.clone(),
                profile: grant.profile.clone(),
                service: service.clone(),
                version: instance.version,
                state: instance.state,
                specification: serde_json::to_string(&instance.spec)?,
                authority: serde_json::to_string(&authority)?,
                grant,
            });
        }
        let source = GroupNetworkSource {
            target: actual.target.clone(),
            name: actual.name,
            version: stack.version,
            grant_revision,
            internet: choice(displays.iter().map(|member| member.internet)),
            lan: choice(displays.iter().map(|member| member.lan)),
            members: displays,
            can_edit,
            network_enabled,
        };
        Ok(GroupNetworkReview {
            database: self.permission_database()?,
            target: actual.target,
            version: stack.version,
            grant_revision,
            members,
            source,
        })
    }
    pub fn group_network_review(&self, target: &ApplicationTarget) -> Result<GroupNetworkReview> {
        let tx = self.connection.unchecked_transaction()?;
        let review = self.read_group_network_review(target)?;
        tx.commit()?;
        Ok(review)
    }
    pub fn set_group_network_permissions(
        &self,
        review: &GroupNetworkReview,
        internet: bool,
        lan: bool,
    ) -> Result<GroupNetworkReview> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.connection,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let current = review.validate(self)?;
        if !current.source.can_edit {
            return Err(EngineError::Conflict(
                "stop all listed services before changing permissions".into(),
            ));
        }
        if (internet || lan) && !current.source.network_enabled {
            return Err(EngineError::Unsupported(
                "listed application networking is disabled".into(),
            ));
        }
        let revision = next_revision(current.grant_revision)?;
        tx.execute("INSERT INTO group_network_revisions(group_id,birth,revision) VALUES (?1,?2,?3) ON CONFLICT(group_id) DO UPDATE SET revision=excluded.revision",
            params![current.target.id,current.target.identity_token,sql_revision(revision)?])?;
        for member in &current.members {
            self.clear_run_intent_for_edit(&tx, &member.id)?;
            tx.execute("INSERT INTO group_network_grants(group_id,service,profile_id,internet,lan,active,epoch,spec_digest) VALUES (?1,?2,?3,?4,?5,1,?6,?7) ON CONFLICT(group_id,service,profile_id) DO UPDATE SET internet=excluded.internet,lan=excluded.lan,active=1,epoch=excluded.epoch,spec_digest=excluded.spec_digest",
                params![current.target.id,member.service,member.profile,internet,lan,sql_revision(revision)?,Digest::of(member.specification.as_bytes()).as_str()])?;
        }
        event(
            &tx,
            None,
            "stack",
            "wasmer-local",
            serde_json::json!({"type":"group_network_permissions_changed","stack_id":current.target.id,"service_count":current.members.len(),"revision":revision,"internet":internet,"lan":lan}),
        )?;
        let version = self.connection.last_insert_rowid();
        if version <= 0 {
            return Err(EngineError::Conflict(
                "group permission event is missing".into(),
            ));
        }
        let changed = tx.execute(
            "UPDATE stacks SET version=?1 WHERE id=?2 AND version=?3",
            params![version, current.target.id, sql_revision(current.version)?],
        )?;
        if changed != 1 {
            return Err(EngineError::Conflict(
                "group changed during permission save".into(),
            ));
        }
        let answer = self.read_group_network_review(&current.target)?;
        tx.commit()?;
        Ok(answer)
    }
}
