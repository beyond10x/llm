#![cfg(any(feature = "file", feature = "keychain"))]
use llm_credentials::{SecretError, SecretRef, SecretResolver};

fn reference() -> SecretRef {
    SecretRef::new("explicit").unwrap()
}

#[cfg(all(feature = "file", target_os = "linux"))]
mod file {
    use super::*;
    use llm_credentials::{MAX_SECRET_BYTES, file::FileResolver};
    use std::{
        collections::BTreeMap,
        fs,
        os::unix::fs::{PermissionsExt, symlink},
        path::Path,
    };

    fn resolver(path: &Path) -> FileResolver {
        FileResolver::new(BTreeMap::from([(reference(), path.to_owned())])).unwrap()
    }
    fn protected(path: &Path, content: &[u8]) {
        fs::write(path, content).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    // Canonicalize the test's own temp directory (macOS /var is a symlink). The
    // production resolver deliberately never canonicalizes operator paths.
    #[tokio::test]
    async fn raw_bytes_rotation_and_read_only_refresh() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap().join("credential");
        let resolver = resolver(&path);
        for bytes in [b"".as_slice(), b"\0\xff\n raw \n"] {
            protected(&path, bytes);
            let first = resolver.resolve(&reference()).await.unwrap();
            assert_eq!(first.secret.expose(), bytes);
            assert_eq!(
                first.version,
                resolver.resolve(&reference()).await.unwrap().version
            );
            assert_eq!(
                resolver.refresh(&reference(), &first.version).await,
                Err(SecretError::RefreshUnsupported)
            );
            protected(&path, b"replacement");
            let second = resolver.resolve(&reference()).await.unwrap();
            assert_ne!(first.version, second.version);
            assert_eq!(second.secret.expose(), b"replacement");
            protected(&path, bytes);
            assert_eq!(
                first.version,
                resolver.resolve(&reference()).await.unwrap().version
            );
        }
    }

    #[tokio::test]
    async fn refuses_missing_unsafe_and_oversized_sources() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().canonicalize().unwrap();
        let path = dir.join("credential");
        let adapter = resolver(&path);
        assert_eq!(
            adapter.resolve(&reference()).await.unwrap_err(),
            SecretError::Missing
        );
        protected(&path, b"fixture");
        assert_eq!(
            adapter
                .resolve(&SecretRef::new("other").unwrap())
                .await
                .unwrap_err(),
            SecretError::Missing
        );
        for mode in [0o604, 0o640, 0o700, 0o4600] {
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            assert_eq!(
                adapter.resolve(&reference()).await.unwrap_err(),
                SecretError::UnsafeSource
            );
        }
        protected(&path, b"fixture");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(
            adapter.resolve(&reference()).await.unwrap_err(),
            SecretError::UnsafeSource
        );
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len((MAX_SECRET_BYTES + 1) as u64)
            .unwrap();
        assert_eq!(
            adapter.resolve(&reference()).await.unwrap_err(),
            SecretError::TooLarge
        );
        assert_eq!(
            resolver(&dir).resolve(&reference()).await.unwrap_err(),
            SecretError::UnsafeSource
        );
    }

    #[tokio::test]
    async fn refuses_leaf_parent_symlinks_hardlinks_and_fifo_without_waiting_for_writer() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().canonicalize().unwrap();
        let real = dir.join("real");
        protected(&real, b"fixture");
        let leaf = dir.join("link");
        symlink(&real, &leaf).unwrap();
        assert_eq!(
            resolver(&leaf).resolve(&reference()).await.unwrap_err(),
            SecretError::UnsafeSource
        );
        let parent = dir.join("parent-link");
        symlink(&dir, &parent).unwrap();
        assert_eq!(
            resolver(&parent.join("real"))
                .resolve(&reference())
                .await
                .unwrap_err(),
            SecretError::UnsafeSource
        );
        fs::hard_link(&real, dir.join("hardlink")).unwrap();
        assert_eq!(
            resolver(&real).resolve(&reference()).await.unwrap_err(),
            SecretError::UnsafeSource
        );
        let fifo = dir.join("fifo");
        rustix::fs::mknodat(
            rustix::fs::CWD,
            &fifo,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
            0,
        )
        .unwrap();
        assert_eq!(
            resolver(&fifo).resolve(&reference()).await.unwrap_err(),
            SecretError::UnsafeSource
        );
    }

    #[test]
    fn rejects_path_traversal_and_redacts_paths() {
        for path in ["relative", "/", "/safe/../unsafe"] {
            assert_eq!(
                FileResolver::new(BTreeMap::from([(reference(), path.into())])).unwrap_err(),
                SecretError::UnsafeSource
            );
        }
        assert!(
            !format!("{:?}", resolver(Path::new("/private/path-canary"))).contains("path-canary")
        );
    }
}

#[cfg(all(feature = "file", not(target_os = "linux")))]
#[tokio::test]
async fn file_adapter_refuses_unimplemented_platform_protections() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().canonicalize().unwrap().join("unused");
    let resolver = llm_credentials::file::FileResolver::new(std::collections::BTreeMap::from([(
        reference(),
        path,
    )]))
    .unwrap();
    assert_eq!(
        resolver.resolve(&reference()).await.unwrap_err(),
        SecretError::UnsupportedPlatform
    );
}

#[cfg(feature = "keychain")]
mod keychain {
    use super::*;
    use keyring_core::{api::CredentialStoreApi, mock};
    use llm_core::Id;
    use llm_credentials::{
        MAX_SECRET_BYTES,
        keychain::{KeychainEntry, KeychainResolver},
    };
    use std::collections::BTreeMap;

    #[tokio::test]
    async fn exact_store_lookup_rotation_and_read_only_refresh() {
        let store = mock::Store::new().unwrap();
        let entry = store.build("service", "selected", None).unwrap();
        store
            .build("service", "other", None)
            .unwrap()
            .set_secret(b"wrong-user")
            .unwrap();
        store
            .build("other-service", "selected", None)
            .unwrap()
            .set_secret(b"wrong-service")
            .unwrap();
        let resolver = KeychainResolver::new(
            store,
            BTreeMap::from([(
                reference(),
                KeychainEntry {
                    service: Id::new("service").unwrap(),
                    entry: Id::new("selected").unwrap(),
                },
            )]),
        )
        .unwrap();
        assert_eq!(
            resolver.resolve(&reference()).await.unwrap_err(),
            SecretError::Missing
        );
        entry.set_secret(b"\0\xff\n credential\n").unwrap();
        let first = resolver.resolve(&reference()).await.unwrap();
        assert_eq!(first.secret.expose(), b"\0\xff\n credential\n");
        assert_eq!(
            first.version,
            resolver.resolve(&reference()).await.unwrap().version
        );
        assert_eq!(
            resolver
                .resolve(&SecretRef::new("unknown").unwrap())
                .await
                .unwrap_err(),
            SecretError::Missing
        );
        assert_eq!(
            resolver.refresh(&reference(), &first.version).await,
            Err(SecretError::RefreshUnsupported)
        );
        entry.set_secret(b"rotated").unwrap();
        let second = resolver.resolve(&reference()).await.unwrap();
        assert_ne!(first.version, second.version);
        assert_eq!(second.secret.expose(), b"rotated");
        entry
            .as_any()
            .downcast_ref::<mock::Cred>()
            .unwrap()
            .set_error(keyring_core::Error::Invalid(
                "private-canary".into(),
                "private-canary".into(),
            ));
        let error = resolver.resolve(&reference()).await.unwrap_err();
        assert_eq!(error, SecretError::Unavailable);
        assert!(!format!("{error:?} {error} {resolver:?}").contains("private-canary"));
        entry.set_secret(&vec![0; MAX_SECRET_BYTES + 1]).unwrap();
        assert_eq!(
            resolver.resolve(&reference()).await.unwrap_err(),
            SecretError::TooLarge
        );
    }
}
