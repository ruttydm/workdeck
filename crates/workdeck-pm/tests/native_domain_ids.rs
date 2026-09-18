use workdeck_pm::{EvidenceId, FeatureId, GateId};
#[test]
fn native_domain_ids_reject_cross_kind_and_noncanonical_paths() {
    let feature = FeatureId::new();
    let gate = GateId::new();
    let evidence = EvidenceId::new();
    assert_eq!(feature.as_str().parse::<FeatureId>().unwrap(), feature);
    assert_eq!(gate.as_str().parse::<GateId>().unwrap(), gate);
    assert_eq!(evidence.as_str().parse::<EvidenceId>().unwrap(), evidence);
    for value in [
        gate.as_str(),
        evidence.as_str(),
        "FEAT-1",
        "features/FEAT-00000000000000000000000000",
        "FEAT-80000000000000000000000000",
    ] {
        assert!(value.parse::<FeatureId>().is_err(), "accepted {value}");
        assert!(serde_json::from_value::<FeatureId>(serde_json::json!(value)).is_err());
    }
    assert!(
        feature
            .to_string()
            .to_lowercase()
            .parse::<FeatureId>()
            .is_err()
    );
    assert!(feature.as_str().parse::<GateId>().is_err());
    assert!(gate.as_str().parse::<EvidenceId>().is_err());
    assert_eq!(
        serde_json::from_value::<EvidenceId>(serde_json::to_value(&evidence).unwrap()).unwrap(),
        evidence
    );
}
