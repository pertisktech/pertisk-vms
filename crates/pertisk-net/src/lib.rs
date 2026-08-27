//! Phase 3: Linux bridge, TAP, and guest networking.

use pertisk_types::NetSpec;

/// Planned host-side attachment for a guest NIC.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NetworkPlan {
    pub bridge: String,
    pub tap: Option<String>,
}

impl NetworkPlan {
    pub fn from_spec(bridge: impl Into<String>, spec: &NetSpec) -> Self {
        Self {
            bridge: bridge.into(),
            tap: spec.tap.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_tap_from_spec() {
        let spec = NetSpec {
            tap: Some("vmtap0".into()),
        };
        let plan = NetworkPlan::from_spec("vmbr0", &spec);
        assert_eq!(plan.bridge, "vmbr0");
        assert_eq!(plan.tap.as_deref(), Some("vmtap0"));
    }
}
