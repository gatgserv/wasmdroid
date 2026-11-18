//! Read-only actual application lineage. Labels describe presentation only;
//! durable native identities and current membership select every action scope.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationTargetKind {
    Application,
    Group,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationTarget {
    pub kind: ApplicationTargetKind,
    pub id: String,
    pub identity_token: String,
}
#[derive(Serialize)]
pub struct ApplicationMember {
    pub id: String,
    pub version: u64,
    pub state: crate::api::InstanceState,
    pub name: String,
    pub service: Option<String>,
    pub primary: bool,
}
#[derive(Serialize)]
pub struct ApplicationRecord {
    pub target: ApplicationTarget,
    pub name: String,
    pub version: u64,
    pub members: Vec<ApplicationMember>,
    pub primary_id: Option<String>,
}
#[derive(Serialize)]
pub struct ApplicationPage {
    pub targets: Vec<ApplicationRecord>,
    pub next: Option<String>,
}

fn title(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_control())
        .take(80)
        .collect()
}
fn member(record: &InstanceRecord, service: Option<String>, primary: bool) -> ApplicationMember {
    ApplicationMember {
        id: record.id.clone(),
        version: record.version,
        state: record.state,
        name: title(
            record
                .spec
                .labels
                .get("dev.wasmdroid.ui.name")
                .unwrap_or(&record.spec.command),
        ),
        service,
        primary,
    }
}

impl StateDb {
    pub fn application_control_epoch(&self) -> Result<u64> {
        let sequence:i64=self.connection.query_row("SELECT COALESCE(MAX(sequence),0) FROM events WHERE kind='application' AND json_extract(payload_json,'$.type')='application_stop_all'",[],|row|row.get(0))?;
        u64::try_from(sequence)
            .map_err(|_| EngineError::Conflict("invalid application control epoch".into()))
    }
    pub(crate) fn stack_was_published(&self, id: &str) -> Result<bool> {
        Ok(self.connection.query_row("SELECT EXISTS(SELECT 1 FROM events WHERE kind='stack' AND json_extract(payload_json,'$.type')='stack_created' AND json_extract(payload_json,'$.stack_id')=?1)",[id],|row|row.get(0))?)
    }
    pub fn application_target(
        &self,
        kind: ApplicationTargetKind,
        id: &str,
    ) -> Result<ApplicationRecord> {
        validate_name(id)?;
        let token = match kind {
            ApplicationTargetKind::Application => self.browser_identity(id)?,
            ApplicationTargetKind::Group => format!("stack:{}", self.stack_birth(id)?),
        };
        self.resolve_application_target(&ApplicationTarget {
            kind,
            id: id.into(),
            identity_token: token,
        })
    }
    fn stack_birth(&self, id: &str) -> Result<i64> {
        self.stack_summary(id)?;
        self.connection.query_row("SELECT MAX(sequence) FROM events WHERE kind='stack' AND json_extract(payload_json,'$.type')='stack_created' AND json_extract(payload_json,'$.stack_id')=?1",[id],|row|row.get::<_,Option<i64>>(0))?
            .filter(|sequence|*sequence>0).ok_or_else(||EngineError::Conflict("application group identity is missing".into()))
    }
    pub fn resolve_application_target(
        &self,
        target: &ApplicationTarget,
    ) -> Result<ApplicationRecord> {
        validate_name(&target.id)?;
        if target.identity_token.is_empty() || target.identity_token.len() > 256 {
            return Err(EngineError::Invalid("invalid application identity".into()));
        }
        match target.kind {
            ApplicationTargetKind::Application => {
                let id: String = self
                    .connection
                    .query_row(
                        "SELECT instance_id FROM instance_browser_identity WHERE profile_id=?1",
                        [&target.identity_token],
                        |row| row.get(0),
                    )
                    .optional()?
                    .ok_or_else(|| EngineError::NotFound("application identity".into()))?;
                if self.stack_for_instance(&id)?.is_some() {
                    return Err(EngineError::Conflict(
                        "choose the actual application group".into(),
                    ));
                }
                let record = self.instance(&id)?;
                if record.spec.endpoint_id != "local" {
                    return Err(EngineError::Unsupported("nonlocal application".into()));
                }
                let display = member(&record, None, true);
                Ok(ApplicationRecord {
                    target: ApplicationTarget {
                        kind: ApplicationTargetKind::Application,
                        id: id.clone(),
                        identity_token: target.identity_token.clone(),
                    },
                    name: display.name.clone(),
                    version: record.version,
                    members: vec![display],
                    primary_id: Some(id),
                })
            }
            ApplicationTargetKind::Group => {
                let birth = self.stack_birth(&target.id)?;
                if target.identity_token != format!("stack:{birth}") {
                    return Err(EngineError::Conflict(
                        "application group identity changed".into(),
                    ));
                }
                let stack = self.stack(&target.id)?;
                let primaries: Vec<_> = stack
                    .instances
                    .iter()
                    .filter(|instance| {
                        instance
                            .spec
                            .labels
                            .get("dev.wasmdroid.catalog.primary")
                            .is_some_and(|value| value == "true")
                    })
                    .collect();
                let primary = if primaries.len() == 1 {
                    Some(primaries[0])
                } else {
                    None
                };
                let name = title(
                    primary
                        .and_then(|instance| instance.spec.labels.get("dev.wasmdroid.ui.name"))
                        .map(String::as_str)
                        .unwrap_or(&target.id),
                );
                let primary_id = primary.map(|instance| instance.id.clone());
                let members = stack
                    .plan
                    .startup_order
                    .iter()
                    .zip(&stack.instances)
                    .map(|(service, instance)| {
                        member(
                            instance,
                            Some(service.clone()),
                            primary_id.as_ref() == Some(&instance.id),
                        )
                    })
                    .collect();
                Ok(ApplicationRecord {
                    target: target.clone(),
                    name,
                    version: stack.version,
                    members,
                    primary_id,
                })
            }
        }
    }
    pub fn application_page(&self, after: Option<&str>, limit: u32) -> Result<ApplicationPage> {
        if !(1..=50).contains(&limit) || after.is_some_and(|value| value.len() > 320) {
            return Err(EngineError::Invalid(
                "application page exceeds budget".into(),
            ));
        }
        let mut query=self.connection.prepare("SELECT cursor,kind,id,token FROM (
            SELECT 'application:'||b.profile_id AS cursor,'application' AS kind,b.instance_id AS id,b.profile_id AS token FROM instance_browser_identity b JOIN instances i ON i.id=b.instance_id WHERE i.endpoint_id='local' AND NOT EXISTS(SELECT 1 FROM stack_services m WHERE m.instance_id=b.instance_id)
            UNION ALL
            SELECT 'group:'||s.id AS cursor,'group' AS kind,s.id AS id,'stack:'||(SELECT MAX(e.sequence) FROM events e WHERE e.kind='stack' AND json_extract(e.payload_json,'$.type')='stack_created' AND json_extract(e.payload_json,'$.stack_id')=s.id) AS token FROM stacks s WHERE s.endpoint_id='local'
        ) WHERE cursor>?1 ORDER BY cursor LIMIT ?2")?;
        let rows = query
            .query_map(params![after.unwrap_or(""), i64::from(limit) + 1], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let more = rows.len() > limit as usize;
        let selected: Vec<_> = rows.into_iter().take(limit as usize).collect();
        let next = if more {
            selected.last().map(|row| row.0.clone())
        } else {
            None
        };
        let mut targets = Vec::new();
        for (_, kind, id, identity_token) in selected {
            targets.push(self.resolve_application_target(&ApplicationTarget {
                kind: if kind == "group" {
                    ApplicationTargetKind::Group
                } else {
                    ApplicationTargetKind::Application
                },
                id,
                identity_token,
            })?);
        }
        Ok(ApplicationPage { targets, next })
    }
}
