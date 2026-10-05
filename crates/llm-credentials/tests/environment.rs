//! A reference bound to a caller-named environment variable (Harness `NamedSource::Environment`).
//!
//! Nothing here changes this process's environment: `unsafe_code` is forbidden, and
//! `std::env::set_var` is unsafe. A case that needs a variable set re-runs itself as a child test
//! process whose environment the parent chooses, and requires that exactly that case ran there.
#![cfg(all(feature = "environment", unix))]
use llm_credentials::{SecretError, SecretRef, SecretResolver, environment::EnvironmentResolver};
use std::{collections::BTreeMap, ffi::OsStr, os::unix::ffi::OsStrExt, process::Command};

const CHILD: &str = "LLM_CREDENTIALS_ENVIRONMENT_CHILD";
const VARIABLE: &str = "LLM_CREDENTIALS_FIXTURE_TOKEN";
const CANARY: &str = "llm-fixture-private-marker";

fn reference() -> SecretRef {
    SecretRef::new("explicit").unwrap()
}

fn resolver() -> EnvironmentResolver {
    EnvironmentResolver::new(BTreeMap::from([(reference(), VARIABLE.to_owned())])).unwrap()
}

/// In the parent: runs `name` again in a child whose environment holds `value` under
/// [`VARIABLE`] (or lacks it), requires that one case ran and passed there, and returns false.
/// In the child: returns true, and the caller runs the case.
fn in_child(name: &str, value: Option<&[u8]>) -> bool {
    if std::env::var_os(CHILD).is_some() {
        return true;
    }
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([name, "--exact", "--test-threads=1"])
        .env(CHILD, "1")
        .env_remove(VARIABLE);
    if let Some(value) = value {
        command.env(VARIABLE, OsStr::from_bytes(value));
    }
    let output = command.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The runner's own summary line, the last one: a filter that selected nothing exits 0 too.
    let summary = stdout
        .lines()
        .rfind(|line| line.starts_with("test result:"))
        .unwrap();
    assert!(summary.contains(" 1 passed;"), "{summary}");
    false
}

#[tokio::test]
async fn a_caller_named_variable_resolves_its_exact_bytes_and_is_read_only() {
    // A trailing newline, inner whitespace and a byte that is not UTF-8 all survive: the
    // consumer decides what can be presented, as for a file.
    let value = b"llm-fixture-private-marker\n raw \xff\n";
    if !in_child(
        "a_caller_named_variable_resolves_its_exact_bytes_and_is_read_only",
        Some(value),
    ) {
        return;
    }
    let resolver = resolver();
    let first = resolver.resolve(&reference()).await.unwrap();
    assert_eq!(first.secret.expose(), value);
    let second = resolver.resolve(&reference()).await.unwrap();
    assert_eq!(first.version, second.version);
    assert_eq!(
        resolver.refresh(&reference(), &first.version).await,
        Err(SecretError::RefreshUnsupported)
    );
    let read = resolver.read(&reference()).await.unwrap();
    assert_eq!(read.secret.expose(), value);
    let rendered = format!("{first:?} {resolver:?}");
    assert!(!rendered.contains(CANARY), "{rendered}");
    assert!(!rendered.contains(VARIABLE), "{rendered}");
}

#[tokio::test]
async fn an_unset_variable_and_an_unbound_reference_are_missing_and_name_only_the_reference() {
    if !in_child(
        "an_unset_variable_and_an_unbound_reference_are_missing_and_name_only_the_reference",
        None,
    ) {
        return;
    }
    let resolver = resolver();
    assert_eq!(
        resolver.resolve(&reference()).await.unwrap_err(),
        SecretError::Missing
    );
    let error = resolver.read(&reference()).await.unwrap_err();
    assert_eq!(error.kind(), SecretError::Missing);
    assert_eq!(error.reference(), &reference());
    let rendered = format!("{error} {error:?}");
    assert!(rendered.contains("explicit"), "{rendered}");
    assert!(!rendered.contains(VARIABLE), "{rendered}");

    let other = SecretRef::new("unbound").unwrap();
    assert_eq!(
        resolver.resolve(&other).await.unwrap_err(),
        SecretError::Missing
    );
    let error = resolver.read(&other).await.unwrap_err();
    assert_eq!(error.reference(), &other);
    assert!(error.to_string().contains("unbound"), "{error}");
}

#[test]
fn variable_names_are_validated_at_construction_and_never_shown() {
    for name in ["", "NAME=VALUE", "NAME\0VALUE"] {
        assert_eq!(
            EnvironmentResolver::new(BTreeMap::from([(reference(), name.to_owned())])).unwrap_err(),
            SecretError::InvalidReference,
            "{name:?}"
        );
    }
    let too_many = (0..=4096)
        .map(|index| {
            (
                SecretRef::new(format!("reference-{index}")).unwrap(),
                format!("VARIABLE_{index}"),
            )
        })
        .collect();
    assert_eq!(
        EnvironmentResolver::new(too_many).unwrap_err(),
        SecretError::TooManyReferences
    );
    assert!(!format!("{:?}", resolver()).contains(VARIABLE));
}
