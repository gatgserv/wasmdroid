//! Private host IPC contracts. Keys/configuration must never be sent to a guest,
//! diagnostic event, public spec, backup or log. Debug always redacts keys.
use crate::{EngineError, Result, api::validate_name, state::StateDb};
use ring::{
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::Ipv4Addr,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct GrantKey([u8; 32]);
impl std::fmt::Debug for GrantKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<private grant>")
    }
}
impl TryFrom<String> for GrantKey {
    type Error = EngineError;
    fn try_from(text: String) -> Result<Self> {
        let mut bytes = [0; 32];
        hex::decode_to_slice(text, &mut bytes).map_err(|_| invalid())?;
        Ok(Self(bytes))
    }
}
impl From<GrantKey> for String {
    fn from(key: GrantKey) -> Self {
        hex::encode(key.0)
    }
}
impl GrantKey {
    fn new() -> Result<Self> {
        let mut bytes = [0; 32];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| invalid())?;
        Ok(Self(bytes))
    }
    pub fn tag(&self, header: &[u8]) -> [u8; 32] {
        hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, &self.0), header)
            .as_ref()
            .try_into()
            .expect("HMAC-SHA256 length")
    }
    pub fn verify(&self, header: &[u8], tag: &[u8]) -> bool {
        hmac::verify(&hmac::Key::new(hmac::HMAC_SHA256, &self.0), header, tag).is_ok()
    }
    pub fn same(&self, other: &Self) -> bool {
        self.verify(b"grant identity", &other.tag(b"grant identity"))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerGrant {
    pub instance_id: String,
    pub generation: String,
    pub alias: String,
    pub ip: Ipv4Addr,
    pub key: GrantKey,
}
impl PeerGrant {
    pub fn same(&self, other: &Self) -> bool {
        self.instance_id == other.instance_id
            && self.generation == other.generation
            && self.alias == other.alias
            && self.ip == other.ip
            && self.key.same(&other.key)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerNetwork {
    pub schema_version: u32,
    pub revision: u64,
    pub stack_id: String,
    pub instance_id: String,
    pub generation: String,
    pub ip: Ipv4Addr,
    pub alias: String,
    pub incoming: Vec<PeerGrant>,
    pub outgoing: Vec<PeerGrant>,
}
fn invalid() -> EngineError {
    EngineError::Invalid("invalid private network configuration".into())
}
pub fn endpoint_name(id: &str, generation: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(id.as_bytes());
    hash.update([0]);
    hash.update(generation.as_bytes());
    format!("b1-{}", &hex::encode(hash.finalize())[..32])
}
fn private_ip(ip: Ipv4Addr) -> bool {
    let bytes = ip.octets();
    bytes[..3] == [10, 203, 0] && (2..=9).contains(&bytes[3])
}
impl WorkerNetwork {
    /// Native inventory, not a list supplied by a guest or permission request.
    /// Reserve declared services even when stopped or never granted a route.
    pub fn reserved_aliases(&self, state: &StateDb) -> Result<BTreeSet<String>> {
        let stack = state.stack(&self.stack_id)?;
        if stack.plan.services.is_empty() || stack.plan.services.len() > 8 {
            return Err(invalid());
        }
        self.validate_membership(state)?;
        let names: BTreeSet<String> = stack
            .plan
            .services
            .keys()
            .map(|name| name.to_ascii_lowercase())
            .collect();
        if names.len() != stack.plan.services.len()
            || names.iter().any(|name| {
                name.is_empty()
                    || name.len() > 63
                    || name == "localhost"
                    || name.ends_with('.')
                    || name.parse::<std::net::IpAddr>().is_ok()
                    || !name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
            })
        {
            return Err(invalid());
        }
        // A changed plan cannot supply an old capture's DNS inventory.
        if state.stack(&self.stack_id)?.version != stack.version {
            return Err(invalid());
        }
        Ok(names)
    }

    pub fn validate_membership(&self, state: &StateDb) -> Result<()> {
        self.validate(&self.instance_id, &self.generation)?;
        if state.stack_for_instance(&self.instance_id)?.as_deref() != Some(&self.stack_id) {
            return Err(invalid());
        }
        let stack = state.stack(&self.stack_id)?;
        let members = state.stack_members(&self.stack_id)?;
        let known: BTreeMap<_, _> = stack
            .plan
            .services
            .keys()
            .enumerate()
            .map(|(index, name)| {
                let member = members
                    .iter()
                    .find(|member| member["service"].as_str() == Some(name))
                    .ok_or_else(invalid)?;
                Ok((
                    member["instance_id"]
                        .as_str()
                        .ok_or_else(invalid)?
                        .to_owned(),
                    (
                        name.to_ascii_lowercase(),
                        Ipv4Addr::new(10, 203, 0, index as u8 + 2),
                    ),
                ))
            })
            .collect::<Result<_>>()?;
        if known.get(&self.instance_id) != Some(&(self.alias.to_ascii_lowercase(), self.ip)) {
            return Err(invalid());
        }
        for peer in self.incoming.iter().chain(&self.outgoing) {
            if known.get(&peer.instance_id) != Some(&(peer.alias.to_ascii_lowercase(), peer.ip)) {
                return Err(invalid());
            }
        }
        Ok(())
    }
    pub fn validate(&self, id: &str, generation: &str) -> Result<()> {
        if self.schema_version != 1
            || self.revision == 0
            || self.instance_id != id
            || self.generation != generation
            || !private_ip(self.ip)
            || self.incoming.len() > 7
            || self.outgoing.len() > 7
        {
            return Err(invalid());
        }
        validate_name(id)?;
        validate_name(generation)?;
        validate_name(&self.stack_id)?;
        for peers in [&self.incoming, &self.outgoing] {
            let mut ids = BTreeSet::new();
            let mut ips = BTreeSet::new();
            let mut aliases = BTreeSet::new();
            for peer in peers {
                validate_name(&peer.instance_id)?;
                validate_name(&peer.generation)?;
                let alias = peer.alias.to_ascii_lowercase();
                if !private_ip(peer.ip)
                    || peer.ip == self.ip
                    || peer.instance_id == id
                    || !ids.insert(&peer.instance_id)
                    || !ips.insert(peer.ip)
                    || alias.is_empty()
                    || alias.len() > 63
                    || alias == "localhost"
                    || alias.ends_with('.')
                    || !alias
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
                    || !aliases.insert(alias)
                {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
}

/// Only a freshly acquired exclusive coordinator owner may reclaim these
/// generation-specific socket names. Unknown files and symlink directories stay.
#[cfg(unix)]
pub(crate) fn recover_endpoints(session: &crate::session::EngineSession) -> Result<()> {
    use std::os::unix::fs::FileTypeExt;
    let directory = session.root().join("network");
    let metadata = match std::fs::symlink_metadata(&directory) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid());
    }
    for entry in std::fs::read_dir(&directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.len() == 35
            && name.starts_with("b1-")
            && name[3..]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && entry.file_type()?.is_socket()
        {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// Called by the common host using only its actual reserved/running jobs. The
/// persisted membership supplies aliases/IPs; caller labels grant no authority.
/// Reuse unchanged directed keys so a third peer stopping does not reset a flow.
pub fn topology(
    state: &StateDb,
    stack_id: &str,
    workers: &BTreeMap<String, String>,
    previous: &BTreeMap<String, WorkerNetwork>,
    revision: u64,
) -> Result<BTreeMap<String, WorkerNetwork>> {
    let stack = state.stack(stack_id)?;
    if workers.is_empty() || workers.len() > 8 || revision == 0 {
        return Err(invalid());
    }
    let members = state.stack_members(stack_id)?;
    let mut known = BTreeMap::new();
    for (index, alias) in stack.plan.services.keys().enumerate() {
        let member = members
            .iter()
            .find(|member| member["service"].as_str() == Some(alias))
            .ok_or_else(invalid)?;
        known.insert(
            member["instance_id"]
                .as_str()
                .ok_or_else(invalid)?
                .to_owned(),
            (alias.clone(), Ipv4Addr::new(10, 203, 0, index as u8 + 2)),
        );
    }
    let mut result = BTreeMap::new();
    for (id, generation) in workers {
        validate_name(generation)?;
        let (_, ip) = known.get(id).ok_or_else(invalid)?;
        result.insert(
            id.clone(),
            WorkerNetwork {
                schema_version: 1,
                revision,
                stack_id: stack_id.into(),
                instance_id: id.clone(),
                generation: generation.clone(),
                ip: *ip,
                alias: known[id].0.to_ascii_lowercase(),
                incoming: Vec::new(),
                outgoing: Vec::new(),
            },
        );
    }
    for (source, source_generation) in workers {
        for (target, target_generation) in workers {
            if source == target {
                continue;
            }
            let (alias, ip) = &known[target];
            let old = previous.get(source).filter(|old| {
                old.stack_id == stack_id
                    && old.generation == *source_generation
                    && old.ip == known[source].1
            });
            let key = old
                .and_then(|old| {
                    old.outgoing.iter().find(|peer| {
                        peer.instance_id == *target
                            && peer.generation == *target_generation
                            && peer.alias == *alias
                            && peer.ip == *ip
                    })
                })
                .map(|peer| Ok(peer.key.clone()))
                .unwrap_or_else(GrantKey::new)?;
            result
                .get_mut(source)
                .ok_or_else(invalid)?
                .outgoing
                .push(PeerGrant {
                    instance_id: target.clone(),
                    generation: target_generation.clone(),
                    alias: alias.clone(),
                    ip: *ip,
                    key: key.clone(),
                });
            let (alias, ip) = &known[source];
            result
                .get_mut(target)
                .ok_or_else(invalid)?
                .incoming
                .push(PeerGrant {
                    instance_id: source.clone(),
                    generation: source_generation.clone(),
                    alias: alias.clone(),
                    ip: *ip,
                    key,
                });
        }
    }
    for config in result.values() {
        config.validate(&config.instance_id, &config.generation)?;
    }
    Ok(result)
}
