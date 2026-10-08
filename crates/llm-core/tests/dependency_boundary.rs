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
            if package["name"] == "b10x-llm-core" && dependency["kind"].is_null() {
                assert!(
                    !matches!(
                        name,
                        "reqwest" | "hyper" | "axum" | "llm-http" | "b10x-llm-http"
                    ),
                    "core imported transport: {name}"
                );
            }
            // Building a protocol client from a binding is `b10x-llm-models`' job; routing
            // stays protocol-neutral so ordered fallback never names a client.
            if package["name"] == "b10x-llm-routing" && dependency["kind"].is_null() {
                assert!(
                    !matches!(
                        name,
                        "b10x-llm-chat" | "b10x-llm-responses" | "b10x-llm-messages"
                    ),
                    "routing imported a protocol crate: {name}"
                );
            }
        }
    }
    assert!(
        packages
            .iter()
            .any(|package| package["name"] == "b10x-llm-models"),
        "the port-construction crate is a workspace member this boundary covers"
    );
}
