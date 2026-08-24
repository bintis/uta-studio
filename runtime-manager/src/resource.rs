use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{RuntimeManagerError, RuntimeManagerResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Model,
    Runtime,
    Tool,
    Bundle,
}

impl ResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Runtime => "runtime",
            Self::Tool => "tool",
            Self::Bundle => "bundle",
        }
    }
}

impl fmt::Display for ResourceKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ResourceKind {
    type Err = RuntimeManagerError;

    fn from_str(value: &str) -> RuntimeManagerResult<Self> {
        match value {
            "model" => Ok(Self::Model),
            "runtime" => Ok(Self::Runtime),
            "tool" => Ok(Self::Tool),
            "bundle" => Ok(Self::Bundle),
            other => Err(RuntimeManagerError::invalid_resource_ref(format!(
                "unknown resource kind: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ModelId(String);

impl ModelId {
    pub fn new(value: impl Into<String>) -> RuntimeManagerResult<Self> {
        let value = value.into();
        validate_id(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for ModelId {
    type Err = RuntimeManagerError;

    fn from_str(value: &str) -> RuntimeManagerResult<Self> {
        Self::new(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceRef {
    pub kind: ResourceKind,
    pub id: String,
}

impl ResourceRef {
    pub fn new(kind: ResourceKind, id: impl Into<String>) -> RuntimeManagerResult<Self> {
        let id = id.into();
        validate_id(&id)?;
        Ok(Self { kind, id })
    }

    pub fn model(id: impl Into<String>) -> RuntimeManagerResult<Self> {
        Self::new(ResourceKind::Model, id)
    }

    pub fn runtime(id: impl Into<String>) -> RuntimeManagerResult<Self> {
        Self::new(ResourceKind::Runtime, id)
    }

    pub fn tool(id: impl Into<String>) -> RuntimeManagerResult<Self> {
        Self::new(ResourceKind::Tool, id)
    }

    pub fn bundle(id: impl Into<String>) -> RuntimeManagerResult<Self> {
        Self::new(ResourceKind::Bundle, id)
    }
}

impl fmt::Display for ResourceRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.kind, self.id)
    }
}

impl Serialize for ResourceRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ResourceRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Canonical(String),
            Legacy { kind: ResourceKind, id: String },
        }
        match Wire::deserialize(deserializer)? {
            Wire::Canonical(value) => value.parse().map_err(serde::de::Error::custom),
            Wire::Legacy { kind, id } => Self::new(kind, id).map_err(serde::de::Error::custom),
        }
    }
}

impl FromStr for ResourceRef {
    type Err = RuntimeManagerError;

    fn from_str(value: &str) -> RuntimeManagerResult<Self> {
        let (kind, id) = value.split_once(':').ok_or_else(|| {
            RuntimeManagerError::invalid_resource_ref(format!(
                "resource must use <kind>:<id>, got {value:?}"
            ))
        })?;
        Self::new(ResourceKind::from_str(kind)?, id)
    }
}

fn validate_id(id: &str) -> RuntimeManagerResult<()> {
    if id.is_empty() {
        return Err(RuntimeManagerError::invalid_resource_ref(
            "resource id must not be empty",
        ));
    }
    if id == "." || id == ".." || id.contains("..") {
        return Err(RuntimeManagerError::invalid_resource_ref(
            "resource id must not contain path traversal",
        ));
    }
    if id.contains('/') || id.contains('\\') || id.contains(':') {
        return Err(RuntimeManagerError::invalid_resource_ref(
            "resource id must not contain path separators",
        ));
    }
    if !id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(RuntimeManagerError::invalid_resource_ref(format!(
            "resource id contains unsupported characters: {id:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_resource_refs() {
        assert_eq!(
            "model:rmvpe".parse::<ResourceRef>().unwrap(),
            ResourceRef::model("rmvpe").unwrap()
        );
        assert_eq!(
            "runtime:openvino_2026_3".parse::<ResourceRef>().unwrap(),
            ResourceRef::runtime("openvino_2026_3").unwrap()
        );
    }

    #[test]
    fn resource_refs_use_canonical_strings_and_read_legacy_objects() {
        let resource = ResourceRef::model("rmvpe").unwrap();
        assert_eq!(
            serde_json::to_string(&resource).unwrap(),
            r#""model:rmvpe""#
        );
        assert_eq!(
            serde_json::from_str::<ResourceRef>(r#"{"kind":"model","id":"rmvpe"}"#).unwrap(),
            resource
        );
    }

    #[test]
    fn rejects_invalid_resource_refs() {
        for value in [
            "expert:rmvpe",
            "model:",
            "model:../rmvpe",
            "model:/tmp/rmvpe",
            "model:rmvpe.onnx:extra",
            "model:rmvpe onnx",
        ] {
            assert!(value.parse::<ResourceRef>().is_err(), "{value}");
        }
    }
}
