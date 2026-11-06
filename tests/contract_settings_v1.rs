use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/settings-v1")
        .join(name);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

fn assert_keys(value: &Value, expected: &[&str]) {
    let object = value.as_object().expect("fixture must be an object");
    for key in expected {
        assert!(object.contains_key(*key), "missing key {key}");
    }
}

#[test]
fn aa_default_fixture_pins_settings_v1_defaults() {
    let value = fixture("aa-default.json");
    assert_keys(
        &value,
        &[
            "enabled",
            "antialias",
            "weight_mode",
            "center",
            "center_offset",
            "radius",
            "feather",
            "strength",
            "preserve_alpha",
            "fxaa_quality",
            "smaa_threshold",
            "compare",
            "compare_interval_ms",
            "show_region",
            "auto_scale",
            "min_scale",
            "max_scale",
        ],
    );
    assert_eq!(value["enabled"], serde_json::json!(false));
    assert_eq!(value["antialias"], serde_json::json!("cmaa2"));
    assert_eq!(value["weight_mode"], serde_json::json!("inverted"));
    assert_eq!(value["center"], serde_json::json!([0.5, 0.5]));
    assert_eq!(value["center_offset"], serde_json::json!(0.0));
    assert_eq!(value["radius"], serde_json::json!(0.35));
    assert_eq!(value["feather"], serde_json::json!(0.10));
    assert_eq!(value["strength"], serde_json::json!(1.0));
    assert_eq!(value["preserve_alpha"], serde_json::json!(true));
    assert_eq!(value["fxaa_quality"], serde_json::json!("quality"));
    assert_eq!(value["smaa_threshold"], serde_json::json!(0.1));
    assert_eq!(value["compare"], serde_json::json!(false));
    assert_eq!(value["compare_interval_ms"], serde_json::json!(2000));
    assert_eq!(value["show_region"], serde_json::json!(false));
    assert_eq!(value["auto_scale"], serde_json::json!(false));
    assert_eq!(value["min_scale"], serde_json::json!(0.5));
    assert_eq!(value["max_scale"], serde_json::json!(1.5));
}

#[test]
fn aa_weight_mode_fixtures_cover_all_modes() {
    for (name, mode) in [
        ("aa-center.json", "center"),
        ("aa-inverted.json", "inverted"),
        ("aa-whole.json", "whole"),
    ] {
        let value = fixture(name);
        assert_eq!(value["weight_mode"], serde_json::json!(mode));
    }
}

#[test]
fn vrs_default_fixture_pins_settings_v1_defaults() {
    let value = fixture("vrs-default.json");
    assert_keys(
        &value,
        &[
            "enabled",
            "center_left",
            "center_right",
            "center_offset",
            "inner_radius",
            "outer_radius",
            "feather",
            "rate_inner",
            "rate_mid",
            "rate_outer",
            "stereo_only",
            "scene_format",
            "flip_y",
            "compare",
            "compare_interval_ms",
            "debug_show_map",
        ],
    );
    assert_eq!(value["enabled"], serde_json::json!(false));
    assert_eq!(value["center_left"], serde_json::json!([0.5, 0.5]));
    assert_eq!(value["center_right"], serde_json::json!([0.5, 0.5]));
    assert_eq!(value["center_offset"], serde_json::json!(0.0));
    assert_eq!(value["inner_radius"], serde_json::json!(0.35));
    assert_eq!(value["outer_radius"], serde_json::json!(0.70));
    assert_eq!(value["feather"], serde_json::json!(0.10));
    assert_eq!(value["rate_inner"], serde_json::json!("1x1"));
    assert_eq!(value["rate_mid"], serde_json::json!("2x2"));
    assert_eq!(value["rate_outer"], serde_json::json!("4x4"));
    assert_eq!(value["stereo_only"], serde_json::json!(true));
    assert_eq!(value["scene_format"], serde_json::json!("rgba16f"));
    assert_eq!(value["flip_y"], serde_json::json!(true));
    assert_eq!(value["compare"], serde_json::json!(false));
    assert_eq!(value["compare_interval_ms"], serde_json::json!(2000));
    assert_eq!(value["debug_show_map"], serde_json::json!(false));
}

#[test]
fn vrs_rates_fixture_lists_the_seven_supported_rates() {
    let value = fixture("vrs-rates.json");
    let rates = value["allowed_rates"]
        .as_array()
        .expect("allowed_rates array");
    let labels: Vec<&str> = rates.iter().map(|rate| rate.as_str().unwrap()).collect();
    assert_eq!(
        labels,
        vec!["1x1", "1x2", "2x1", "2x2", "2x4", "4x2", "4x4"]
    );

    let radv = value["radv_gfx11_supported"]
        .as_array()
        .expect("radv_gfx11_supported array");
    let radv: Vec<&str> = radv.iter().map(|rate| rate.as_str().unwrap()).collect();
    assert_eq!(radv, vec!["1x1", "1x2", "2x1", "2x2"]);

    let msaa = value["msaa_coarse_sample_counts"]
        .as_array()
        .expect("msaa_coarse_sample_counts array");
    let msaa: Vec<u64> = msaa.iter().map(|count| count.as_u64().unwrap()).collect();
    assert_eq!(msaa, vec![1, 2, 4]);
}
