use std::collections::BTreeSet;

use serde_json::Value;

fn oracle() -> Value {
    serde_json::from_str(include_str!("../../../port/hunk/oracles/ui-lib-test.json"))
        .expect("frozen ui-lib oracle must be valid JSON")
}

#[test]
fn frozen_ui_lib_oracle_maps_both_pins_and_every_source_test() {
    let oracle = oracle();
    assert_eq!(
        oracle["source"]["baseline"]["commit"],
        "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
    );
    assert_eq!(
        oracle["source"]["baseline"]["blob"],
        "1292f255de36843329783d6d7143dd25c512d500"
    );
    assert_eq!(oracle["source"]["baseline"]["bytes"], 23_737);
    assert_eq!(oracle["source"]["baseline"]["lines"], 609);
    assert_eq!(
        oracle["source"]["stable"]["commit"],
        "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
    );
    assert_eq!(
        oracle["source"]["stable"]["blob"],
        oracle["source"]["baseline"]["blob"]
    );
    assert_eq!(oracle["oracleRuns"]["baseline"]["passed"], 23);
    assert_eq!(oracle["oracleRuns"]["baseline"]["expectCalls"], 460);
    assert_eq!(oracle["oracleRuns"]["stable"]["passed"], 23);
    assert_eq!(oracle["oracleRuns"]["stable"]["expectCalls"], 460);

    let mappings = oracle["testMappings"].as_array().unwrap();
    assert_eq!(mappings.len(), 23);
    let upstream_names = mappings
        .iter()
        .map(|mapping| mapping["upstream"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(upstream_names.len(), 23);
    assert!(mappings.iter().all(|mapping| {
        !mapping["rust"].as_array().unwrap().is_empty()
            && !mapping["assertion"].as_str().unwrap().is_empty()
    }));

    let mut next_line = 1;
    for interval in oracle["sourceCoverage"].as_array().unwrap() {
        let lines = interval["lines"].as_array().unwrap();
        assert_eq!(lines[0].as_u64().unwrap(), next_line);
        assert!(!interval["rust"].as_array().unwrap().is_empty());
        next_line = lines[1].as_u64().unwrap() + 1;
    }
    assert_eq!(next_line, 610);
}
