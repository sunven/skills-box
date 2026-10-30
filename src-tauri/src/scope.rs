//! Typed skill scope identity.
//!
//! Every read and mutation is explicit about its scope. The wire format is a
//! single string: `global` or `project:<uuid>`. Project ids are stable UUIDs
//! owned by the project registry; the scope type never invents an id.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ScopeId {
    Global,
    Project(Uuid),
}

impl ScopeId {
    pub fn global() -> Self {
        Self::Global
    }

    pub fn project(id: Uuid) -> Self {
        Self::Project(id)
    }

    pub fn is_global(&self) -> bool {
        matches!(self, Self::Global)
    }

    pub fn project_id(&self) -> Option<Uuid> {
        match self {
            Self::Global => None,
            Self::Project(id) => Some(*id),
        }
    }

    /// The persisted and IPC representation.
    pub fn as_string(&self) -> String {
        match self {
            Self::Global => "global".to_string(),
            Self::Project(id) => format!("project:{id}"),
        }
    }
}

impl Default for ScopeId {
    fn default() -> Self {
        Self::Global
    }
}

impl std::fmt::Display for ScopeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.as_string())
    }
}

impl std::str::FromStr for ScopeId {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "global" => Ok(Self::Global),
            _ => match raw.strip_prefix("project:") {
                Some(id) => Uuid::parse_str(id)
                    .ok()
                    .filter(|uuid| uuid.to_string() == id)
                    .map(Self::Project)
                    .ok_or_else(|| format!("无效的 project scope id：{id}")),
                None => Err(format!("未知的 scope：{raw}")),
            },
        }
    }
}

impl Serialize for ScopeId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.as_string())
    }
}

impl<'de> Deserialize<'de> for ScopeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT_ID: &str = "5a7b6c1d-2e3f-4a5b-8c9d-0e1f2a3b4c5d";

    fn project_id() -> Uuid {
        Uuid::parse_str(PROJECT_ID).unwrap()
    }

    #[test]
    fn global_and_project_scopes_use_the_documented_wire_format() {
        assert_eq!(ScopeId::global().as_string(), "global");
        assert_eq!(ScopeId::global().to_string(), "global");
        assert_eq!(
            ScopeId::project(project_id()).as_string(),
            format!("project:{PROJECT_ID}")
        );
        assert!(ScopeId::global().is_global());
        assert!(!ScopeId::project(project_id()).is_global());
        assert_eq!(
            ScopeId::project(project_id()).project_id(),
            Some(project_id())
        );
        assert_eq!(ScopeId::global().project_id(), None);
    }

    #[test]
    fn parses_only_global_or_project_uuid_strings() {
        assert_eq!("global".parse::<ScopeId>(), Ok(ScopeId::global()));
        assert_eq!(
            format!("project:{PROJECT_ID}").parse::<ScopeId>(),
            Ok(ScopeId::project(project_id()))
        );
        for raw in [
            "",
            "GLOBAL",
            "project:",
            "project:not-a-uuid",
            "project:5a7b6c1d-2e3f-4a5b-8c9d-0e1f2a3b4c5d-extra",
            "project:5a7b6c1d2e3f4a5b8c9d0e1f2a3b4c5d",
            "workspace:5a7b6c1d-2e3f-4a5b-8c9d-0e1f2a3b4c5d",
            "global:1",
        ] {
            assert!(
                raw.parse::<ScopeId>().is_err(),
                "{raw:?} should be rejected"
            );
        }
    }

    #[test]
    fn serializes_as_a_string_and_round_trips() {
        for scope in [ScopeId::global(), ScopeId::project(project_id())] {
            let json = serde_json::to_string(&scope).unwrap();
            assert_eq!(json, format!("\"{}\"", scope.as_string()));
            let parsed: ScopeId = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, scope);
        }
        assert!(serde_json::from_str::<ScopeId>("\"project:nope\"").is_err());
        assert!(serde_json::from_str::<ScopeId>("1").is_err());
    }

    #[test]
    fn default_scope_is_global() {
        assert_eq!(ScopeId::default(), ScopeId::global());
    }
}
