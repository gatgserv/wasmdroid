//! Bounded event-to-node conversion. No alias expansion, tag construction,
//! duplicate-key overwrite, implicit environment lookup or unsafe YAML loader.
use crate::{EngineError, Result};
use saphyr_parser::{Event, Parser, ScalarStyle, StrInput};
use std::collections::BTreeMap;

pub(super) enum Node {
    Null,
    Scalar(String, bool), // bool: plain YAML style (type-sensitive options)
    Sequence(Vec<Node>),
    Mapping(BTreeMap<String, Node>),
}
impl Node {
    pub fn mapping(self) -> Result<BTreeMap<String, Node>> {
        match self {
            Self::Mapping(map) => Ok(map),
            _ => Err(invalid("expected a mapping")),
        }
    }
    pub fn sequence(self) -> Result<Vec<Node>> {
        match self {
            Self::Sequence(values) => Ok(values),
            _ => Err(invalid("expected a sequence")),
        }
    }
    pub fn string(self) -> Result<String> {
        match self {
            Self::Scalar(value, plain) if !plain || !typed_scalar(&value) => Ok(value),
            _ => Err(invalid("expected a string; quote typed YAML scalars")),
        }
    }
    pub fn scalar(self) -> Result<String> {
        match self {
            Self::Scalar(value, _) => Ok(value),
            _ => Err(invalid("expected a scalar value")),
        }
    }
    pub fn boolean(self) -> Result<bool> {
        match self {
            Self::Scalar(value, true) if value.eq_ignore_ascii_case("true") => Ok(true),
            Self::Scalar(value, true) if value.eq_ignore_ascii_case("false") => Ok(false),
            _ => Err(invalid("expected a YAML boolean")),
        }
    }
    pub fn empty(&self) -> bool {
        matches!(self, Self::Null) || matches!(self,Self::Mapping(map) if map.is_empty())
    }
}
fn typed_scalar(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let unsigned = lower.trim_start_matches(['+', '-']);
    matches!(
        lower.as_str(),
        "true" | "false" | ".inf" | "+.inf" | "-.inf" | ".nan"
    ) || value.parse::<f64>().is_ok()
        || unsigned.starts_with("0x")
        || unsigned.starts_with("0o")
}
pub(super) fn invalid(reason: &str) -> EngineError {
    EngineError::Invalid(format!("stack: {reason}"))
}
pub(super) fn unsupported(reason: &str) -> EngineError {
    EngineError::Unsupported(format!("stack: {reason}"))
}

struct Reader<'a> {
    parser: Parser<'a, StrInput<'a>>,
    events: usize,
}
impl<'a> Reader<'a> {
    fn next(&mut self) -> Result<Event<'a>> {
        self.events += 1;
        if self.events > 8192 {
            return Err(invalid("YAML event budget exceeded"));
        }
        self.parser
            .next()
            .ok_or_else(|| invalid("unexpected YAML end"))?
            .map(|(event, _)| event)
            .map_err(|error| {
                // Scanner details can contain input values (including secrets).
                EngineError::Invalid(format!(
                    "stack YAML syntax at line {}, column {}",
                    error.marker().line(),
                    error.marker().col() + 1
                ))
            })
    }
    fn node(&mut self, event: Event<'a>, depth: usize) -> Result<Node> {
        if depth > 16 {
            return Err(invalid("YAML nesting exceeds 16 levels"));
        }
        match event {
            Event::Scalar(value, style, anchor, tag) => {
                if anchor != 0 || tag.is_some() {
                    return Err(unsupported("YAML anchors and tags"));
                }
                if value.len() > 64 * 1024 || value.contains('\0') {
                    return Err(invalid("scalar exceeds limit or contains NUL"));
                }
                let plain = style == ScalarStyle::Plain;
                if plain && (value.is_empty() || value == "~" || value.eq_ignore_ascii_case("null"))
                {
                    Ok(Node::Null)
                } else {
                    Ok(Node::Scalar(value.into_owned(), plain))
                }
            }
            Event::SequenceStart(anchor, tag) => {
                if anchor != 0 || tag.is_some() {
                    return Err(unsupported("YAML anchors and tags"));
                }
                let mut values = Vec::new();
                loop {
                    let event = self.next()?;
                    if matches!(event, Event::SequenceEnd) {
                        break;
                    }
                    values.push(self.node(event, depth + 1)?);
                }
                Ok(Node::Sequence(values))
            }
            Event::MappingStart(anchor, tag) => {
                if anchor != 0 || tag.is_some() {
                    return Err(unsupported("YAML anchors and tags"));
                }
                let mut map = BTreeMap::new();
                loop {
                    let event = self.next()?;
                    if matches!(event, Event::MappingEnd) {
                        break;
                    }
                    let key = self.node(event, depth + 1)?.string()?;
                    if key.is_empty() || key.len() > 256 || key == "<<" {
                        return Err(invalid("invalid mapping key or YAML merge"));
                    }
                    if map.contains_key(&key) {
                        return Err(invalid("duplicate YAML mapping key"));
                    }
                    let event = self.next()?;
                    map.insert(key, self.node(event, depth + 1)?);
                }
                Ok(Node::Mapping(map))
            }
            Event::Alias(_) => Err(unsupported("YAML aliases")),
            _ => Err(invalid("unexpected YAML structure")),
        }
    }
}
pub(super) fn parse(input: &str) -> Result<Node> {
    if input.is_empty() || input.len() > 128 * 1024 {
        return Err(invalid("document must contain 1..131072 UTF-8 bytes"));
    }
    let mut reader = Reader {
        parser: Parser::new_from_str(input),
        events: 0,
    };
    if !matches!(reader.next()?, Event::StreamStart)
        || !matches!(reader.next()?, Event::DocumentStart(_))
    {
        return Err(invalid("expected one YAML document"));
    }
    let first = reader.next()?;
    let root = reader.node(first, 0)?;
    if !matches!(reader.next()?, Event::DocumentEnd) || !matches!(reader.next()?, Event::StreamEnd)
    {
        return Err(unsupported("multiple YAML documents"));
    }
    Ok(root)
}
