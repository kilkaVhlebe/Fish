//! The boot report.

use serde::{Deserialize, Serialize};

/// ABI version pair in a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbiReport {
    /// Major version.
    pub major: u16,
    /// Minor version.
    pub minor: u16,
}

/// One mod in a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModReport {
    /// Mod id.
    pub id: String,
    /// Mod version.
    pub version: String,
    /// `ok`, `failed` or `loading`.
    pub status: String,
    /// Why the mod failed, if it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// What the loader did at boot (and what has happened since).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootReport {
    /// Fish version.
    pub fish: String,
    /// ABI served.
    pub abi: AbiReport,
    /// `client` or `server`.
    pub side: String,
    /// Mods in load order, failed ones last.
    pub mods: Vec<ModReport>,
    /// Non-fatal findings.
    pub warnings: Vec<String>,
    /// Errors (discovery, resolution, load and runtime failures).
    pub errors: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_omits_empty_reason() {
        let r = BootReport {
            fish: "0.1.0".into(),
            abi: AbiReport { major: 0, minor: 1 },
            side: "client".into(),
            mods: vec![
                ModReport {
                    id: "a".into(),
                    version: "1.0.0".into(),
                    status: "ok".into(),
                    reason: None,
                },
                ModReport {
                    id: "b".into(),
                    version: "1.0.0".into(),
                    status: "failed".into(),
                    reason: Some("boom".into()),
                },
            ],
            warnings: vec![],
            errors: vec!["x".into()],
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json.matches("reason").count(), 1);
        assert_eq!(serde_json::from_str::<BootReport>(&json).unwrap(), r);
    }
}
