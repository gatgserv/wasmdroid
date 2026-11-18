//! Neutral boot/restart serialization regressions; no worker or platform launch.
use wasmdroid_engine::api::{InstanceSpec, RestartPolicy};

#[test]
fn old_specifications_never_implicitly_opt_into_boot() {
    let spec: InstanceSpec = serde_json::from_value(serde_json::json!({
        "artifact_ref":"wasmer.io/example/server@1.0.0", "command":"serve",
        "restart_policy":"always"
    }))
    .unwrap();
    assert_eq!(spec.restart_policy, RestartPolicy::Always);
    assert!(!spec.start_on_boot);
}

#[test]
fn explicit_boot_is_independent_of_restart_policy_and_round_trips() {
    for policy in ["no", "on_failure", "always"] {
        let source = serde_json::json!({
            "artifact_ref":"wasmer.io/example/server@1.0.0", "command":"serve",
            "restart_policy":policy, "start_on_boot":true
        });
        let spec: InstanceSpec = serde_json::from_value(source).unwrap();
        spec.validate().unwrap();
        assert!(spec.start_on_boot);
        let wire = serde_json::to_value(&spec).unwrap();
        assert_eq!(wire["start_on_boot"], true);
        assert_eq!(wire["restart_policy"], policy);
        assert!(
            serde_json::from_value::<InstanceSpec>(wire)
                .unwrap()
                .start_on_boot
        );
    }
    assert!(serde_json::from_value::<InstanceSpec>(serde_json::json!({
        "artifact_ref":"wasmer.io/example/server@1.0.0", "command":"serve", "start_on_boot":"true"
    })).is_err());
}
