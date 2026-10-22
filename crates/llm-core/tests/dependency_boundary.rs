use serde_json::Value;
use std::{path::Path, process::Command};

#[test]
fn inference_workspace_does_not_depend_on_its_consumers_and_core_has_no_transport() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--offline",
            "--locked",
            "--no-deps",
            "--format-version",
            "1",
        ])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success());
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    let packages = metadata["packages"].as_array().unwrap();
    for package in packages {
        for dependency in package["dependencies"].as_array().unwrap() {
            let name = dependency["name"].as_str().unwrap();
            assert!(
                !name.contains("harness")
                    && !name.contains("llmgw")
                    && !name.starts_with("inference-"),
                "LLM must own its port instead of depending on consumer implementation: {name}"
            );
            if package["name"] == "llm-core" && dependency["kind"].is_null() {
                assert!(
                    !matches!(name, "reqwest" | "hyper" | "axum" | "llm-http"),
                    "core imported transport: {name}"
                );
            }
            // Building a protocol client from a binding is `llm-models`' job; routing
            // stays protocol-neutral so ordered fallback never names a client.
            if package["name"] == "llm-routing" && dependency["kind"].is_null() {
                assert!(
                    !matches!(name, "llm-chat" | "llm-responses" | "llm-messages"),
                    "routing imported a protocol crate: {name}"
                );
            }
        }
    }
    assert!(
        packages
            .iter()
            .any(|package| package["name"] == "llm-models"),
        "the port-construction crate is a workspace member this boundary covers"
    );
}

// Bedrock signs on `ring`; an AWS SDK crate or `aws-lc` anywhere in the lockfile breaks that.
#[test]
fn no_package_in_the_lockfile_is_an_aws_sdk_crate_or_aws_lc() {
    let lock =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock"))
            .unwrap();
    let names: Vec<&str> = lock
        .lines()
        .filter_map(|line| line.strip_prefix("name = \""))
        .filter_map(|rest| rest.strip_suffix('"'))
        .collect();
    assert!(
        names.contains(&"ring"),
        "the lockfile is read: ring is there"
    );
    for name in names {
        assert!(
            !name.starts_with("aws-"),
            "an AWS crate entered the lockfile: {name}"
        );
    }
}
