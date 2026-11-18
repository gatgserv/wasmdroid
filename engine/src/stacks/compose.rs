use super::{
    MAX_SERVICES,
    yaml::{self, Node, invalid, unsupported},
};
use crate::{
    Result,
    api::{PortBinding, Protocol},
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Document {
    pub services: BTreeMap<String, Service>,
    pub volumes: BTreeMap<String, Option<String>>,
}
pub(super) struct Service {
    pub image: String,
    pub command: Option<Vec<String>>,
    pub environment: BTreeMap<String, String>,
    pub ports: Vec<PortBinding>,
    pub mounts: Vec<VolumeMount>,
    pub depends_on: Vec<String>,
    pub working_dir: Option<String>,
    pub tty: bool,
    pub stdin_open: bool,
}
pub(super) struct VolumeMount {
    pub source: String,
    pub target: String,
    pub read_only: bool,
}
fn fields(map: &BTreeMap<String, Node>, allowed: &[&str]) -> Result<()> {
    if map.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(unsupported("unknown Compose attribute"));
    }
    Ok(())
}
pub(super) fn logical_name(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 63
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err(invalid(
            "logical names require 1..63 ASCII letters/digits/._-, starting with a letter/digit",
        ));
    }
    Ok(())
}
pub(super) fn service_alias(value: &str) -> Result<String> {
    logical_name(value)?;
    let alias = value.to_ascii_lowercase();
    if alias.trim_end_matches('.') == "localhost"
        || value.parse::<std::net::IpAddr>().is_ok()
        || value.ends_with('.')
    {
        return Err(invalid("service name conflicts with local addressing"));
    }
    Ok(alias)
}
fn value(node: Node) -> Result<String> {
    literal(&node.string()?)
}
fn literal(input: &str) -> Result<String> {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '$' {
            if chars.peek() == Some(&'$') {
                chars.next();
                output.push('$');
                continue;
            }
            if chars
                .peek()
                .is_some_and(|ch| ch.is_ascii_alphabetic() || *ch == '_' || *ch == '{')
            {
                return Err(unsupported(
                    "host environment interpolation; use explicit values or $$ for literal dollars",
                ));
            }
        }
        output.push(ch);
    }
    Ok(output)
}
fn required(map: &mut BTreeMap<String, Node>, key: &str) -> Result<Node> {
    map.remove(key)
        .ok_or_else(|| invalid("missing required Compose attribute"))
}
fn boolean(map: &mut BTreeMap<String, Node>, key: &str) -> Result<bool> {
    map.remove(key)
        .map(Node::boolean)
        .transpose()
        .map(|value| value.unwrap_or(false))
}
pub(super) fn parse(document: &str) -> Result<Document> {
    let mut root = yaml::parse(document)?.mapping()?;
    fields(&root, &["name", "version", "services", "volumes"])?;
    if let Some(name) = root.remove("name") {
        logical_name(&value(name)?)?;
    }
    if let Some(version) = root.remove("version") {
        let _ = value(version)?;
    }
    let raw_services = required(&mut root, "services")?.mapping()?;
    if raw_services.is_empty() || raw_services.len() > MAX_SERVICES {
        return Err(invalid("stack requires 1..8 services"));
    }
    let mut names = BTreeSet::new();
    let mut services = BTreeMap::new();
    for (name, node) in raw_services {
        if !names.insert(service_alias(&name)?) {
            return Err(invalid("ambiguous case-insensitive service name"));
        }
        services.insert(name, service(node)?);
    }
    let mut volumes = BTreeMap::new();
    if let Some(node) = root.remove("volumes") {
        let raw = node.mapping()?;
        if raw.len() > 32 {
            return Err(invalid("too many named volumes"));
        }
        for (name, node) in raw {
            logical_name(&name)?;
            if node.empty() {
                volumes.insert(name, None);
                continue;
            }
            // Re-parse the definition without discarding any unknown option.
            let mut options = node.mapping()?;
            fields(&options, &["external", "name"])?;
            let external = boolean(&mut options, "external")?;
            let actual = options.remove("name").map(value).transpose()?;
            if !external && actual.is_some() {
                return Err(unsupported("unscoped non-external volume name"));
            }
            volumes.insert(name.clone(), external.then(|| actual.unwrap_or(name)));
        }
    }
    Ok(Document { services, volumes })
}
fn service(node: Node) -> Result<Service> {
    let mut map = node.mapping()?;
    fields(
        &map,
        &[
            "image",
            "command",
            "environment",
            "ports",
            "volumes",
            "depends_on",
            "working_dir",
            "tty",
            "stdin_open",
        ],
    )?;
    let image = value(required(&mut map, "image")?)?;
    let command = match map.remove("command") {
        None | Some(Node::Null) => None,
        Some(Node::Scalar(text, _)) => Some(
            shell_words::split(&literal(&text)?).map_err(|_| invalid("invalid command quoting"))?,
        ),
        Some(Node::Sequence(values)) => {
            Some(values.into_iter().map(value).collect::<Result<Vec<_>>>()?)
        }
        _ => return Err(invalid("command must be a string, argv sequence or null")),
    };
    let environment = environment(map.remove("environment"))?;
    let ports = match map.remove("ports") {
        None => Vec::new(),
        Some(node) => ports(node)?,
    };
    let mounts = match map.remove("volumes") {
        None => Vec::new(),
        Some(node) => node
            .sequence()?
            .into_iter()
            .map(mount)
            .collect::<Result<Vec<_>>>()?,
    };
    if mounts.len() > 32 {
        return Err(invalid("too many volume mounts"));
    }
    let depends_on = dependencies(map.remove("depends_on"))?;
    let working_dir = map.remove("working_dir").map(value).transpose()?;
    let tty = boolean(&mut map, "tty")?;
    let stdin_open = boolean(&mut map, "stdin_open")?;
    Ok(Service {
        image,
        command,
        environment,
        ports,
        mounts,
        depends_on,
        working_dir,
        tty,
        stdin_open,
    })
}
fn environment(node: Option<Node>) -> Result<BTreeMap<String, String>> {
    let pairs=match node {
        None=>return Ok(BTreeMap::new()),
        Some(Node::Mapping(values))=>values.into_iter().map(|(name,node)|{
            if matches!(&node,Node::Scalar(value,true) if value.eq_ignore_ascii_case("true")||value.eq_ignore_ascii_case("false")) {
                return Err(invalid("environment booleans must be quoted"));
            }
            Ok((name,literal(&node.scalar()?)?))
        }).collect::<Result<Vec<_>>>()?,
        Some(Node::Sequence(values))=>values.into_iter().map(|node|{
            let entry=value(node)?;
            let (name,value)=entry.split_once('=').ok_or_else(||unsupported("inherited host environment variable"))?;
            Ok((name.to_owned(),value.to_owned()))
        }).collect::<Result<Vec<_>>>()?,
        _=>return Err(invalid("environment must be a mapping or sequence")),
    };
    if pairs.len() > 128 {
        return Err(invalid("too many environment entries"));
    }
    let mut result = BTreeMap::new();
    for (name, value) in pairs {
        if name.is_empty()
            || name.len() > 128
            || name.contains(['=', '\0'])
            || value.contains('\0')
            || result.insert(name, value).is_some()
        {
            return Err(invalid("invalid or duplicate environment entry"));
        }
    }
    Ok(result)
}
fn dependencies(node: Option<Node>) -> Result<Vec<String>> {
    let values = match node {
        None => return Ok(Vec::new()),
        Some(Node::Sequence(values)) => {
            values.into_iter().map(value).collect::<Result<Vec<_>>>()?
        }
        Some(Node::Mapping(values)) => {
            let mut result = Vec::new();
            for (name, node) in values {
                let mut options = node.mapping()?;
                fields(&options, &["condition", "required", "restart"])?;
                if options
                    .remove("condition")
                    .map(value)
                    .transpose()?
                    .unwrap_or_else(|| "service_started".into())
                    != "service_started"
                {
                    return Err(unsupported("dependency health/completion conditions"));
                }
                if options.remove("required").map(Node::boolean).transpose()? == Some(false)
                    || boolean(&mut options, "restart")?
                {
                    return Err(unsupported("optional/restarting dependencies"));
                }
                result.push(name);
            }
            result
        }
        _ => return Err(invalid("depends_on must be a sequence or mapping")),
    };
    let mut seen = BTreeSet::new();
    for name in &values {
        logical_name(name)?;
        if !seen.insert(name) {
            return Err(invalid("duplicate dependency"));
        }
    }
    Ok(values)
}
fn port(value: &str, zero: bool) -> Result<u16> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid("invalid numeric TCP port"));
    }
    let port = value
        .parse::<u16>()
        .map_err(|_| invalid("port exceeds 65535"))?;
    if port == 0 && !zero {
        return Err(invalid("target port must be positive"));
    }
    Ok(port)
}
fn range(value: &str, zero: bool) -> Result<Vec<u16>> {
    let (start, end) = match value.split_once('-') {
        Some((a, b)) => (port(a, zero)?, port(b, zero)?),
        None => {
            let value = port(value, zero)?;
            (value, value)
        }
    };
    if end < start || u32::from(end) - u32::from(start) >= 16 {
        return Err(invalid("port range exceeds 16 or is reversed"));
    }
    Ok((start..=end).collect())
}
fn ports(node: Node) -> Result<Vec<PortBinding>> {
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for node in node.sequence()? {
        let (host, target) = match node {
            Node::Scalar(text, _) => {
                let text = literal(&text)?;
                let (text, protocol) = text
                    .split_once('/')
                    .map(|(text, proto)| (text, Some(proto)))
                    .unwrap_or((&text, None));
                if protocol.is_some_and(|protocol| protocol != "tcp") {
                    return Err(unsupported("non-TCP publication"));
                }
                let parts = text.split(':').collect::<Vec<_>>();
                let (host, target) = match parts.as_slice() {
                    [target] => ("0", *target),
                    [host, target] => (*host, *target),
                    ["127.0.0.1", host, target] => (*host, *target),
                    _ => return Err(unsupported("host publication address")),
                };
                let targets = range(target, false)?;
                let hosts = range(host, true)?;
                let hosts = if hosts == vec![0] {
                    vec![0; targets.len()]
                } else {
                    hosts
                };
                if hosts.len() != targets.len() {
                    return Err(invalid("published and target port ranges differ"));
                }
                (hosts, targets)
            }
            Node::Mapping(mut map) => {
                fields(&map, &["target", "published", "host_ip", "protocol"])?;
                if map
                    .remove("host_ip")
                    .map(value)
                    .transpose()?
                    .is_some_and(|ip| ip != "127.0.0.1")
                {
                    return Err(unsupported("host publication address"));
                }
                if map
                    .remove("protocol")
                    .map(value)
                    .transpose()?
                    .is_some_and(|protocol| protocol != "tcp")
                {
                    return Err(unsupported("non-TCP publication"));
                }
                let target = port(&required(&mut map, "target")?.scalar()?, false)?;
                let host = map
                    .remove("published")
                    .map(Node::scalar)
                    .transpose()?
                    .map(|text| port(&text, true))
                    .transpose()?
                    .unwrap_or(0);
                (vec![host], vec![target])
            }
            _ => return Err(invalid("invalid port mapping")),
        };
        for (host_port, instance_port) in host.into_iter().zip(target) {
            if result.len() >= 16 || !seen.insert(instance_port) {
                return Err(invalid("too many or duplicate guest port publications"));
            }
            result.push(PortBinding {
                host_port,
                instance_port,
                protocol: Protocol::Tcp,
            });
        }
    }
    Ok(result)
}
fn mount(node: Node) -> Result<VolumeMount> {
    let (source, target, read_only) = match node {
        Node::Scalar(text, _) => {
            let text = literal(&text)?;
            let parts = text.split(':').collect::<Vec<_>>();
            let (source, target, read_only) = match parts.as_slice() {
                [source, target] => (*source, *target, false),
                [source, target, "ro"] => (*source, *target, true),
                [source, target, "rw"] => (*source, *target, false),
                _ => return Err(unsupported("anonymous/host-path volume syntax")),
            };
            (source.to_owned(), target.to_owned(), read_only)
        }
        Node::Mapping(mut map) => {
            fields(&map, &["type", "source", "target", "read_only"])?;
            if value(required(&mut map, "type")?)? != "volume" {
                return Err(unsupported("bind/tmpfs volume mount"));
            }
            (
                value(required(&mut map, "source")?)?,
                value(required(&mut map, "target")?)?,
                boolean(&mut map, "read_only")?,
            )
        }
        _ => return Err(invalid("invalid named volume mount")),
    };
    logical_name(&source)?;
    // InstanceSpec performs the final absolute guest path/duplicate target checks.
    Ok(VolumeMount {
        source,
        target,
        read_only,
    })
}
