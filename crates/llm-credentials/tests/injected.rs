use llm_core::BoxFuture;
use llm_credentials::{
    CoordinatedResolver, ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver,
    SecretVersion,
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

#[derive(Default)]
struct Memory {
    generation: AtomicU64,
    refreshes: AtomicUsize,
    resolves: AtomicUsize,
}
impl SecretResolver for Memory {
    fn resolve<'a>(
        &'a self,
        _: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async move {
            self.resolves.fetch_add(1, Ordering::SeqCst);
            let generation = self.generation.load(Ordering::SeqCst);
            Ok(ResolvedSecret {
                secret: Secret::new(format!("test-value-{generation}").into_bytes())?,
                version: SecretVersion::new(generation.to_string())?,
            })
        })
    }
    fn refresh<'a>(
        &'a self,
        _: &'a SecretRef,
        _: &'a SecretVersion,
    ) -> BoxFuture<'a, Result<(), SecretError>> {
        Box::pin(async move {
            self.refreshes.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            self.generation.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

#[tokio::test]
async fn rotation_is_observed_on_each_request_without_changing_the_reference() {
    let source = Arc::new(Memory::default());
    let resolver = CoordinatedResolver::new(source.clone(), 1);
    let reference = SecretRef::new("runtime/model-access").unwrap();
    let config = serde_json::to_string(&reference).unwrap();
    let before = resolver.resolve(&reference).await.unwrap();
    source.generation.store(7, Ordering::SeqCst);
    let after = resolver.resolve(&reference).await.unwrap();
    assert_eq!(before.secret.expose(), b"test-value-0");
    assert_eq!(after.secret.expose(), b"test-value-7");
    assert_ne!(before.version, after.version);
    assert_eq!(serde_json::to_string(&reference).unwrap(), config);
    assert!(!format!("{after:?}").contains("test-value"));
    assert_eq!(source.refreshes.load(Ordering::SeqCst), 0);
    assert_eq!(source.resolves.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn concurrent_rejections_refresh_one_generation_once() {
    let source = Arc::new(Memory::default());
    let resolver = Arc::new(CoordinatedResolver::new(source.clone(), 1));
    let reference = SecretRef::new("account").unwrap();
    let rejected = resolver.resolve(&reference).await.unwrap().version;
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..20 {
        let (resolver, reference, rejected) =
            (resolver.clone(), reference.clone(), rejected.clone());
        tasks.spawn(async move { resolver.refresh(&reference, &rejected).await });
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap().unwrap();
    }
    assert_eq!(source.refreshes.load(Ordering::SeqCst), 1);
    assert_eq!(
        resolver.resolve(&reference).await.unwrap().secret.expose(),
        b"test-value-1"
    );
}

struct Interrupted {
    memory: Memory,
    started: tokio::sync::Notify,
}
impl SecretResolver for Interrupted {
    fn resolve<'a>(
        &'a self,
        reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        self.memory.resolve(reference)
    }
    fn refresh<'a>(
        &'a self,
        _: &'a SecretRef,
        _: &'a SecretVersion,
    ) -> BoxFuture<'a, Result<(), SecretError>> {
        Box::pin(async move {
            self.memory.refreshes.fetch_add(1, Ordering::SeqCst);
            self.started.notify_one();
            std::future::pending().await
        })
    }
}

#[tokio::test]
async fn cancelled_refresh_is_not_repeated_until_the_caller_reconciles_state() {
    let source = Arc::new(Interrupted {
        memory: Memory::default(),
        started: tokio::sync::Notify::new(),
    });
    let resolver = Arc::new(CoordinatedResolver::new(source.clone(), 1));
    let reference = SecretRef::new("account").unwrap();
    let rejected = resolver.resolve(&reference).await.unwrap().version;
    let worker = tokio::spawn({
        let (resolver, reference, rejected) =
            (resolver.clone(), reference.clone(), rejected.clone());
        async move { resolver.refresh(&reference, &rejected).await }
    });
    source.started.notified().await;
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());
    assert_eq!(
        resolver.refresh(&reference, &rejected).await,
        Err(SecretError::RefreshUncertain)
    );
    assert_eq!(source.memory.refreshes.load(Ordering::SeqCst), 1);
    source.memory.generation.store(1, Ordering::SeqCst);
    resolver.refresh(&reference, &rejected).await.unwrap();
}

struct Missing;
impl SecretResolver for Missing {
    fn resolve<'a>(
        &'a self,
        _: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        Box::pin(async { Err(SecretError::Missing) })
    }
}

#[tokio::test]
async fn missing_credentials_never_fall_back_and_reference_memory_is_bounded() {
    let resolver = CoordinatedResolver::new(Arc::new(Missing), 1);
    assert_eq!(
        resolver
            .resolve(&SecretRef::new("a").unwrap())
            .await
            .unwrap_err(),
        SecretError::Missing
    );
    assert_eq!(
        resolver
            .resolve(&SecretRef::new("b").unwrap())
            .await
            .unwrap_err(),
        SecretError::TooManyReferences
    );
}

#[test]
fn secret_material_is_not_a_config_value_and_binary_custody_is_supported() {
    let secret = Secret::new(vec![0, 255, b'\n']).unwrap();
    assert_eq!(secret.expose(), &[0, 255, b'\n']);
    assert_eq!(format!("{secret:?}"), "Secret([REDACTED])");
    assert!(serde_json::from_str::<SecretRef>("\"has whitespace\"").is_err());
    assert!(SecretRef::new("").is_err());
}
