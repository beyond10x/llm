//! story:secrets-resolver. A route's `secret_reference_id` resolves through the `secrets` library
//! (feature `secrets`): the keychain backend over a fresh `keyring_core::mock::Store` per test,
//! passed to the constructor. No test sets the process-wide default store, opens a native
//! store or reads a real credential.
#![cfg(feature = "secrets")]

use keyring_core::{CredentialStore, api::CredentialStoreApi, mock};
use llm_credentials::{
    CoordinatedResolver, ReferenceError, SecretError, SecretRef, SecretResolver,
    secrets::{
        SecretsResolver,
        authorize::Authorized,
        keychain::{DEFAULT_SERVICE, KeychainBackend, entry_user},
        storage::{
            Address, Capability, Revealed, Scope, ScopeName, SecretStorage, SecretValue,
            StorageError, Target,
        },
    },
};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

/// Carried in every fixture value and backend error, so a diagnostic that quotes one is caught.
const CANARY: &str = "llm-fixture-private-marker";

/// The `secret_reference_id` the example route catalog gives the account `remote`.
fn route_reference() -> String {
    let path = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../examples/catalog.toml");
    let catalog: toml::Table = std::fs::read_to_string(path).unwrap().parse().unwrap();
    catalog["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|account| account["id"].as_str() == Some("remote"))
        .and_then(|account| account["secret_reference_id"].as_str())
        .unwrap()
        .to_owned()
}

fn scope(tenant: &str) -> Scope {
    Scope {
        tenant: ScopeName::parse(tenant).unwrap(),
        ..Scope::local()
    }
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

fn fresh() -> Arc<mock::Store> {
    mock::Store::new().unwrap()
}

/// Writes `value` at `name` in `scope` through the library's own keychain backend.
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

/// A rendering of a refusal as a caller would log it: the bare code and the reference error.
fn rendered(error: SecretError, reference: &SecretRef) -> String {
    let named = ReferenceError::new(error, reference.clone());
    format!("{error} {error:?} {named} {named:?}")
}

/// A fixture storage that answers every read with one refusal and counts the reads it saw.
struct Refusing {
    answer: StorageError,
    capabilities: &'static [Capability],
    reads: AtomicUsize,
}
impl Refusing {
    fn new(answer: StorageError, capabilities: &'static [Capability]) -> Self {
        Self {
            answer,
            capabilities,
            reads: AtomicUsize::new(0),
        }
    }
}
#[async_trait::async_trait]
impl SecretStorage for Refusing {
    fn capabilities(&self) -> &[Capability] {
        self.capabilities
    }
    async fn read(&self, _: &Target) -> Result<Revealed, StorageError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Err(self.answer)
    }
}

/// A fixture storage that holds one value and reports no version for it.
struct Unversioned {
    value: Mutex<Vec<u8>>,
}
#[async_trait::async_trait]
impl SecretStorage for Unversioned {
    fn capabilities(&self) -> &[Capability] {
        &[Capability::Read]
    }
    async fn read(&self, _: &Target) -> Result<Revealed, StorageError> {
        let value = self.value.lock().unwrap().clone();
        Ok(Revealed {
            value: SecretValue::new(value)?,
            version: None,
        })
    }
}

/// The acceptance's first clause: the same route reference, the keychain backend, exact bytes.
#[tokio::test]
async fn the_route_reference_resolves_through_the_keychain_backend() {
    let name = route_reference();
    assert_eq!(
        name, "lab-llm-token",
        "the example catalog's reference moved"
    );
    let store = fresh();
    let content = format!("{CANARY}\n raw content \n");
    put(&store, &Scope::local(), &name, content.as_bytes()).await;
    // A value under another name, and the same name in another namespace, are never read.
    put(&store, &Scope::local(), "other-token", b"wrong-name").await;
    let elsewhere = Scope {
        namespace: ScopeName::parse("elsewhere").unwrap(),
        ..Scope::local()
    };
    put(&store, &elsewhere, &name, b"wrong-namespace").await;

    let resolver = keychain(&store, Scope::local());
    let answer = resolver.resolve(&reference(&name)).await.unwrap();
    assert_eq!(answer.secret.expose(), content.as_bytes());
    let diagnostics = format!("{answer:?} {resolver:?}");
    assert!(!diagnostics.contains(CANARY), "{diagnostics}");
    assert!(!diagnostics.contains("raw content"), "{diagnostics}");
}

/// Rotated: every write is a new version, also of the same bytes; `refresh` re-reads and is
/// `RefreshRejected` exactly while the version is unchanged.
#[tokio::test]
async fn a_rotation_is_a_new_version_and_refresh_rereads() {
    let store = fresh();
    let name = route_reference();
    let reference = reference(&name);
    put(&store, &Scope::local(), &name, b"first").await;
    let resolver = keychain(&store, Scope::local());

    let first = resolver.resolve(&reference).await.unwrap();
    assert_eq!(
        resolver.refresh(&reference, &first.version).await,
        Err(SecretError::RefreshRejected)
    );

    put(&store, &Scope::local(), &name, b"first").await;
    let rewritten = resolver.resolve(&reference).await.unwrap();
    assert_eq!(rewritten.secret.expose(), b"first");
    assert_ne!(
        rewritten.version, first.version,
        "rewriting the same value is a new version"
    );
    assert_eq!(resolver.refresh(&reference, &first.version).await, Ok(()));
    assert_eq!(
        resolver.refresh(&reference, &rewritten.version).await,
        Err(SecretError::RefreshRejected)
    );

    put(&store, &Scope::local(), &name, b"second").await;
    let rotated = resolver.resolve(&reference).await.unwrap();
    assert_eq!(rotated.secret.expose(), b"second");
    assert_ne!(rotated.version, rewritten.version);

    // Through the coordinated resolver, a rejected generation that was rotated is not renewed.
    let coordinated = CoordinatedResolver::new(Arc::new(resolver), 4);
    assert_eq!(
        coordinated.refresh(&reference, &first.version).await,
        Ok(())
    );
    assert_eq!(
        coordinated.refresh(&reference, &rotated.version).await,
        Err(SecretError::RefreshRejected)
    );
}

/// Missing: the port's `not-found`.
#[tokio::test]
async fn a_name_the_library_does_not_hold_is_missing() {
    let store = fresh();
    put(&store, &Scope::local(), "other-token", b"wrong-name").await;
    let resolver = keychain(&store, Scope::local());
    let reference = reference(&route_reference());
    assert_eq!(
        resolver.resolve(&reference).await.unwrap_err(),
        SecretError::Missing
    );
    assert_eq!(
        resolver
            .refresh(
                &reference,
                &llm_credentials::SecretVersion::new("v".into()).unwrap()
            )
            .await,
        Err(SecretError::Missing)
    );
}

/// Denied: a scope the local authorizer refuses is `UnsafeSource`, although the keychain holds
/// a value at exactly that address, and no backend read happens.
#[tokio::test]
async fn a_denied_scope_is_unsafe_source_and_reaches_no_backend() {
    let other = scope("other");
    let name = route_reference();
    let store = fresh();
    put(&store, &other, &name, CANARY.as_bytes()).await;
    let reference = reference(&name);
    let error = keychain(&store, other.clone())
        .resolve(&reference)
        .await
        .unwrap_err();
    assert_eq!(error, SecretError::UnsafeSource);
    assert!(!rendered(error, &reference).contains(CANARY));

    let backend = Arc::new(Authorized::local(Refusing::new(
        StorageError::Unavailable,
        &Capability::ALL,
    )));
    let resolver = SecretsResolver::new(backend.clone(), other);
    assert_eq!(
        resolver.resolve(&reference).await.unwrap_err(),
        SecretError::UnsafeSource
    );
    assert_eq!(backend.inner().reads.load(Ordering::SeqCst), 0);
}

/// Unsupported: a backend that offers no read is `UnsupportedPlatform`.
#[tokio::test]
async fn a_backend_without_read_is_unsupported_platform() {
    let resolver = SecretsResolver::new(
        Arc::new(Refusing::new(StorageError::Unsupported, &[])),
        Scope::local(),
    );
    assert_eq!(
        resolver
            .resolve(&reference(&route_reference()))
            .await
            .unwrap_err(),
        SecretError::UnsupportedPlatform
    );
}

/// A backend fault is `Unavailable`, and its own text reaches no diagnostic.
#[tokio::test]
async fn a_backend_fault_is_unavailable_and_carries_no_backend_text() {
    let name = route_reference();
    let store = fresh();
    put(&store, &Scope::local(), &name, CANARY.as_bytes()).await;
    store
        .build(
            DEFAULT_SERVICE,
            &entry_user(&address(&Scope::local(), &name)),
            None,
        )
        .unwrap()
        .as_any()
        .downcast_ref::<mock::Cred>()
        .unwrap()
        .set_error(keyring_core::Error::Invalid(CANARY.into(), CANARY.into()));
    let resolver = keychain(&store, Scope::local());
    let reference = reference(&name);
    let error = resolver.resolve(&reference).await.unwrap_err();
    assert_eq!(error, SecretError::Unavailable);
    let diagnostics = format!("{} {resolver:?}", rendered(error, &reference));
    assert!(!diagnostics.contains(CANARY), "{diagnostics}");
}

/// A reference that is not a `SecretName` is refused before any backend is asked.
#[tokio::test]
async fn a_reference_that_is_not_a_secret_name_is_invalid_and_reaches_no_backend() {
    let backend = Arc::new(Refusing::new(StorageError::Unavailable, &Capability::ALL));
    let resolver = SecretsResolver::new(backend.clone(), Scope::local());
    let too_long = "a".repeat(129);
    for name in ["Lab_LLM_Token", "team//token", "-token", too_long.as_str()] {
        assert_eq!(
            resolver.resolve(&reference(name)).await.unwrap_err(),
            SecretError::InvalidReference,
            "{name}"
        );
    }
    assert_eq!(backend.reads.load(Ordering::SeqCst), 0);
}

/// Every code the port can answer maps to an existing `SecretError`. The match has no wildcard,
/// so a code added to the port does not compile here until it is mapped.
#[tokio::test]
async fn every_storage_code_maps_to_an_existing_secret_error() {
    let reference = reference(&route_reference());
    for code in StorageError::ALL {
        let expected = match code {
            StorageError::NotFound => SecretError::Missing,
            StorageError::Denied => SecretError::UnsafeSource,
            StorageError::Unsupported => SecretError::UnsupportedPlatform,
            StorageError::InvalidName => SecretError::InvalidReference,
            StorageError::TooLarge => SecretError::TooLarge,
            StorageError::Unavailable | StorageError::Conflict => SecretError::Unavailable,
        };
        let resolver = SecretsResolver::new(
            Arc::new(Refusing::new(code, &Capability::ALL)),
            Scope::local(),
        );
        assert_eq!(
            resolver.resolve(&reference).await.unwrap_err(),
            expected,
            "{code}"
        );
    }
}

/// A backend that gives no version gets content identity, as the read-only local adapters do.
#[tokio::test]
async fn an_unversioned_backend_gets_content_identity() {
    let backend = Arc::new(Unversioned {
        value: Mutex::new(b"first".to_vec()),
    });
    let resolver = SecretsResolver::new(backend.clone(), Scope::local());
    let reference = reference(&route_reference());
    let first = resolver.resolve(&reference).await.unwrap();
    let again = resolver.resolve(&reference).await.unwrap();
    assert_eq!(first.version, again.version);
    *backend.value.lock().unwrap() = b"second".to_vec();
    let rotated = resolver.resolve(&reference).await.unwrap();
    assert_eq!(rotated.secret.expose(), b"second");
    assert_ne!(rotated.version, first.version);
    assert_eq!(resolver.refresh(&reference, &first.version).await, Ok(()));
}
