//! Adversary cases for story:secrets-resolver. Every store is a fresh `keyring_core::mock::Store`
//! passed to the constructor; nothing sets the process-wide default store, opens a native store or
//! reads a real credential.
#![cfg(feature = "secrets")]

use keyring_core::{CredentialStore, mock};
use llm_credentials::{
    CoordinatedResolver, MAX_SECRET_BYTES, SecretError, SecretRef, SecretResolver, SecretVersion,
    secrets::{
        SecretsResolver,
        authorize::Authorized,
        keychain::KeychainBackend,
        storage::{
            Address, Capability, Revealed, Scope, ScopeName, SecretStorage, SecretValue,
            StorageError, Target, Version,
        },
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use toml::{Table, Value};

const CANARY: &str = "llm-fixture-private-marker";
const NAME: &str = "lab-llm-token";

fn fresh() -> Arc<mock::Store> {
    mock::Store::new().unwrap()
}

fn address(scope: &Scope, name: &str) -> Address {
    Address::parse(
        scope.tenant.as_str(),
        scope.namespace.as_str(),
        scope.user.as_str(),
        name,
    )
    .unwrap()
}

async fn put(store: &Arc<mock::Store>, scope: &Scope, name: &str, value: &[u8]) {
    KeychainBackend::new(store.clone() as Arc<CredentialStore>)
        .write(
            &Target::unbound(address(scope, name)),
            SecretValue::new(value.to_vec()).unwrap(),
        )
        .await
        .unwrap();
}

fn keychain(store: &Arc<mock::Store>, scope: Scope) -> SecretsResolver {
    SecretsResolver::keychain(store.clone() as Arc<CredentialStore>, scope)
}

fn reference(name: &str) -> SecretRef {
    SecretRef::new(name).unwrap()
}

/// Counts every read and answers each with a fixed value, whatever it claims to offer.
struct Answering {
    capabilities: &'static [Capability],
    reads: AtomicUsize,
}
#[async_trait::async_trait]
impl SecretStorage for Answering {
    fn capabilities(&self) -> &[Capability] {
        self.capabilities
    }
    async fn read(&self, _: &Target) -> Result<Revealed, StorageError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(Revealed {
            value: SecretValue::new(CANARY.as_bytes().to_vec())?,
            version: Some(Version::new("1")),
        })
    }
}

/// Scope confinement: a reference shaped like a path, a traversal, a separator other than `/`, an
/// edge `/` or a case variant is refused before any backend is asked.
#[tokio::test]
async fn path_shaped_references_are_refused_before_any_backend() {
    let backend = Arc::new(Answering {
        capabilities: &Capability::ALL,
        reads: AtomicUsize::new(0),
    });
    let resolver = SecretsResolver::new(backend.clone(), Scope::local());
    for name in [
        "..",
        ".",
        "../lab-llm-token",
        "team/../lab-llm-token",
        "team/./lab-llm-token",
        "team/.hidden",
        "/lab-llm-token",
        "lab-llm-token/",
        "team\\lab-llm-token",
        "LAB-LLM-TOKEN",
        "lab-llm-token%2f..",
        "_lab",
    ] {
        assert_eq!(
            resolver.resolve(&reference(name)).await.unwrap_err(),
            SecretError::InvalidReference,
            "{name}"
        );
    }
    // A non-ASCII lookalike and an empty reference never become a `SecretRef` at all.
    assert_eq!(
        SecretRef::new("l\u{0430}b-llm-token").unwrap_err(),
        SecretError::InvalidReference
    );
    assert_eq!(
        SecretRef::new("").unwrap_err(),
        SecretError::InvalidReference
    );
    assert_eq!(backend.reads.load(Ordering::SeqCst), 0);
}

/// Scope confinement: a nested name that spells another scope's keychain key is read in the
/// configured scope, never in the one it spells.
#[tokio::test]
async fn a_nested_name_never_reaches_another_namespace() {
    let store = fresh();
    let elsewhere = Scope {
        namespace: ScopeName::parse("elsewhere").unwrap(),
        ..Scope::local()
    };
    put(&store, &elsewhere, NAME, CANARY.as_bytes()).await;
    let resolver = keychain(&store, Scope::local());
    for name in [
        "elsewhere/default/lab-llm-token",
        "default/elsewhere/default/lab-llm-token",
        "default/default/default/default/elsewhere/default/lab-llm-token",
    ] {
        assert_eq!(
            resolver.resolve(&reference(name)).await.unwrap_err(),
            SecretError::Missing,
            "{name}"
        );
    }
}

/// Denied: the local authorizer refuses a user other than `default` as it refuses a tenant, and
/// no backend read happens.
#[tokio::test]
async fn a_denied_user_is_unsafe_source_and_reaches_no_backend() {
    let backend = Arc::new(Authorized::local(Answering {
        capabilities: &Capability::ALL,
        reads: AtomicUsize::new(0),
    }));
    let scope = Scope {
        user: ScopeName::parse("someone").unwrap(),
        ..Scope::local()
    };
    let resolver = SecretsResolver::new(backend.clone(), scope);
    assert_eq!(
        resolver.resolve(&reference(NAME)).await.unwrap_err(),
        SecretError::UnsafeSource
    );
    let rejected = SecretVersion::new("v".into()).unwrap();
    assert_eq!(
        resolver.refresh(&reference(NAME), &rejected).await,
        Err(SecretError::UnsafeSource)
    );
    assert_eq!(backend.inner().reads.load(Ordering::SeqCst), 0);
}

/// Unsupported, as `website/docs/concepts/credentials.md` states it: "a backend without read
/// `UnsupportedPlatform`". The library's own mount routing refuses a read on a backend whose
/// capabilities lack `Read` before calling it (`secrets-federation`, `require`); this resolver
/// never consults the capabilities.
#[tokio::test]
async fn a_backend_that_offers_no_read_is_unsupported_platform() {
    let backend = Arc::new(Answering {
        capabilities: &[Capability::Write],
        reads: AtomicUsize::new(0),
    });
    let resolver = SecretsResolver::new(backend.clone(), Scope::local());
    let answer = resolver.resolve(&reference(NAME)).await;
    assert_eq!(
        answer.as_ref().err(),
        Some(&SecretError::UnsupportedPlatform),
        "a backend without read answered {}",
        if answer.is_ok() {
            "a value"
        } else {
            "another refusal"
        }
    );
    assert_eq!(backend.reads.load(Ordering::SeqCst), 0);
}

/// Concurrency on the shared store: reads that race rotations never pair one version with two
/// different values, and every value read is one that was written.
#[tokio::test]
async fn concurrent_reads_during_rotation_never_pair_a_version_with_two_values() {
    let store = fresh();
    put(&store, &Scope::local(), NAME, b"value-0").await;
    let resolver = Arc::new(keychain(&store, Scope::local()));
    let writer = {
        let store = store.clone();
        tokio::spawn(async move {
            for round in 1..=40u32 {
                let value = format!("value-{}", round % 3);
                put(&store, &Scope::local(), NAME, value.as_bytes()).await;
            }
        })
    };
    let mut readers = Vec::new();
    for _ in 0..8 {
        let resolver = resolver.clone();
        readers.push(tokio::spawn(async move {
            let mut seen = Vec::new();
            for _ in 0..40 {
                let answer = resolver.resolve(&reference(NAME)).await.unwrap();
                seen.push((answer.version, answer.secret.expose().to_vec()));
            }
            seen
        }));
    }
    writer.await.unwrap();
    let mut seen: Vec<(SecretVersion, Vec<u8>)> = Vec::new();
    for reader in readers {
        seen.extend(reader.await.unwrap());
    }
    for (version, value) in &seen {
        assert!(
            [b"value-0".as_slice(), b"value-1", b"value-2"].contains(&value.as_slice()),
            "a value that was never written"
        );
        for (other_version, other_value) in &seen {
            if version == other_version {
                assert_eq!(value, other_value, "one version, two values");
            }
        }
    }
}

/// Refresh racing a rotation: through the coordinated resolver, a refresh that succeeds is
/// followed by a resolve whose version moved; one that does not is `RefreshRejected`.
#[tokio::test]
async fn refresh_racing_rotation_succeeds_only_once_the_version_moved() {
    let store = fresh();
    put(&store, &Scope::local(), NAME, b"first").await;
    let coordinated = Arc::new(CoordinatedResolver::new(
        Arc::new(keychain(&store, Scope::local())),
        4,
    ));
    let rejected = coordinated.resolve(&reference(NAME)).await.unwrap().version;
    let writer = {
        let store = store.clone();
        tokio::spawn(async move {
            put(&store, &Scope::local(), NAME, b"second").await;
        })
    };
    let mut refreshes = Vec::new();
    for _ in 0..8 {
        let coordinated = coordinated.clone();
        let rejected = rejected.clone();
        refreshes.push(tokio::spawn(async move {
            coordinated.refresh(&reference(NAME), &rejected).await
        }));
    }
    writer.await.unwrap();
    for refresh in refreshes {
        match refresh.await.unwrap() {
            Ok(()) | Err(SecretError::RefreshRejected) => {}
            other => panic!("refresh racing a rotation answered {other:?}"),
        }
    }
    assert_eq!(
        coordinated.refresh(&reference(NAME), &rejected).await,
        Ok(())
    );
    let current = coordinated.resolve(&reference(NAME)).await.unwrap();
    assert_ne!(current.version, rejected);
    assert_eq!(current.secret.expose(), b"second");
}

/// Boundaries: a value of exactly `MAX_SECRET_BYTES` and an empty value both resolve exactly.
#[tokio::test]
async fn a_value_at_the_size_bound_and_an_empty_value_resolve() {
    let store = fresh();
    let resolver = keychain(&store, Scope::local());
    let largest = vec![b'x'; MAX_SECRET_BYTES];
    put(&store, &Scope::local(), NAME, &largest).await;
    assert_eq!(
        resolver
            .resolve(&reference(NAME))
            .await
            .unwrap()
            .secret
            .expose(),
        largest.as_slice()
    );
    put(&store, &Scope::local(), "empty", b"").await;
    assert_eq!(
        resolver
            .resolve(&reference("empty"))
            .await
            .unwrap()
            .secret
            .expose(),
        b""
    );
}

/// A backend that gives no version: restoring the rejected bytes restores the rejected identity,
/// so `refresh` is `RefreshRejected` again, as the read-only local adapters behave.
#[tokio::test]
async fn restoring_rejected_bytes_on_an_unversioned_backend_is_rejected_again() {
    struct Unversioned(Mutex<Vec<u8>>);
    #[async_trait::async_trait]
    impl SecretStorage for Unversioned {
        fn capabilities(&self) -> &[Capability] {
            &[Capability::Read]
        }
        async fn read(&self, _: &Target) -> Result<Revealed, StorageError> {
            Ok(Revealed {
                value: SecretValue::new(self.0.lock().unwrap().clone())?,
                version: None,
            })
        }
    }
    let backend = Arc::new(Unversioned(Mutex::new(b"first".to_vec())));
    let resolver = SecretsResolver::new(backend.clone(), Scope::local());
    let rejected = resolver.resolve(&reference(NAME)).await.unwrap().version;
    *backend.0.lock().unwrap() = b"second".to_vec();
    assert_eq!(resolver.refresh(&reference(NAME), &rejected).await, Ok(()));
    *backend.0.lock().unwrap() = b"first".to_vec();
    assert_eq!(
        resolver.refresh(&reference(NAME), &rejected).await,
        Err(SecretError::RefreshRejected)
    );
}

// The feature boundary, stated over the whole library source rather than two crate names. The
// library repository at the pinned tag holds twelve crates (`secrets-app`, `secrets-client`,
// `secrets-remote`, ...); "core crates gain no dependency on `secrets`" covers every one of them,
// and a core crate's own `[features]` table that forwards `llm-credentials/secrets` is a
// dependency path as much as a dependency entry is.

const SOURCE: &str = "https://github.com/beyond10x/secrets";
const OWNER: &str = "crates/llm-credentials";
const CREDENTIALS: &str = "b10x-llm-credentials";
const FEATURE_USERS: [&str; 1] = ["checks/conformance"];
const TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

fn root() -> PathBuf {
    PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..")
}

fn read(path: &Path) -> Table {
    fs::read_to_string(path).unwrap().parse().unwrap()
}

fn tables(manifest: &Table) -> Vec<Table> {
    let mut found: Vec<Table> = TABLES
        .iter()
        .filter_map(|name| manifest.get(*name).and_then(Value::as_table).cloned())
        .collect();
    if let Some(targets) = manifest.get("target").and_then(Value::as_table) {
        for target in targets.values() {
            for name in TABLES {
                if let Some(table) = target.get(name).and_then(Value::as_table) {
                    found.push(table.clone());
                }
            }
        }
    }
    found
}

/// The entry a key resolves to, following `workspace = true` into the shared table.
fn resolved<'a>(entry: &'a Value, key: &str, shared: &'a Table) -> &'a Value {
    if entry.get("workspace").and_then(Value::as_bool) == Some(true) {
        shared.get(key).unwrap_or(entry)
    } else {
        entry
    }
}

fn boundary_breaches(root: &Path) -> Vec<String> {
    let workspace = read(&root.join("Cargo.toml"));
    let shared = workspace["workspace"]
        .get("dependencies")
        .and_then(Value::as_table)
        .cloned()
        .unwrap_or_default();
    let mut found = Vec::new();
    for member in workspace["workspace"]["members"].as_array().unwrap() {
        let member = member.as_str().unwrap();
        let manifest = read(&root.join(member).join("Cargo.toml"));
        let mut credential_keys = Vec::new();
        for table in tables(&manifest) {
            for (key, entry) in &table {
                let entry = resolved(entry, key, &shared);
                if member != OWNER && entry.get("git").and_then(Value::as_str) == Some(SOURCE) {
                    found.push(format!("{member} depends on {key} from the library source"));
                }
                let package = entry.get("package").and_then(Value::as_str).unwrap_or(key);
                if package == CREDENTIALS {
                    credential_keys.push(key.clone());
                }
            }
        }
        if member == OWNER || FEATURE_USERS.contains(&member) {
            continue;
        }
        let Some(features) = manifest.get("features").and_then(Value::as_table) else {
            continue;
        };
        for (feature, items) in features {
            for item in items
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if credential_keys.iter().any(|key| {
                    item == format!("{key}/secrets") || item == format!("{key}?/secrets")
                }) {
                    found.push(format!("{member} feature {feature} forwards {item}"));
                }
            }
        }
    }
    found
}

#[test]
fn no_core_crate_reaches_the_library_source_or_forwards_the_feature() {
    let found = boundary_breaches(&root());
    assert!(found.is_empty(), "{found:#?}");
}

/// A copy of the workspace manifests with one replacement applied.
fn mutant(name: &str, file: &str, from: &str, to: &str) -> PathBuf {
    let source = root();
    let copy = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("adversary-{name}"));
    let _ = fs::remove_dir_all(&copy);
    let workspace = read(&source.join("Cargo.toml"));
    let members = workspace["workspace"]["members"].as_array().unwrap();
    for path in std::iter::once(PathBuf::from("Cargo.toml")).chain(
        members
            .iter()
            .map(|member| Path::new(member.as_str().unwrap()).join("Cargo.toml")),
    ) {
        let to = copy.join(&path);
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::copy(source.join(&path), to).unwrap();
    }
    let path = copy.join(file);
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{file} no longer holds {from:?}");
    fs::write(&path, text.replacen(from, to, 1)).unwrap();
    copy
}

/// The rule above can fail: a core crate on another crate of the library source.
#[test]
fn a_core_crate_on_another_library_crate_is_a_breach() {
    let root = mutant(
        "other-crate",
        "crates/llm-providers/Cargo.toml",
        "[dependencies]\n",
        "[dependencies]\nsecrets-client = { git = \"https://github.com/beyond10x/secrets\", tag = \"v0.5.0\" }\n",
    );
    assert!(!boundary_breaches(&root).is_empty());
}

/// The rule above can fail: a core crate forwarding the feature from its own `[features]`.
#[test]
fn a_core_crate_forwarding_the_feature_is_a_breach() {
    let root = mutant(
        "forwarded",
        "crates/llm-providers/Cargo.toml",
        "[lints]",
        "[features]\nsecrets = [\"llm-credentials/secrets\"]\n\n[lints]",
    );
    assert!(!boundary_breaches(&root).is_empty());
}
