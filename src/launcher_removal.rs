//! Explicit Account private-data deletion.
//!
//! This module is deliberately separate from normal Account removal. It first
//! forgets active credentials while retaining recovery metadata, then removes
//! the profile's WebKit store and private files. Only completed deletion removes
//! retained metadata. Shared chunks and other profiles remain untouched.

use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use block2::RcBlock;
use objc2::AnyThread;
use objc2_foundation::{MainThreadMarker, NSDate, NSString, NSUUID};
use objc2_web_kit::WKWebsiteDataStore;

use crate::launcher_accounts::{AccountRepository, CredentialStore, NoBusyProfiles, ProfileLease};

/// Async WebKit operation boundary. Implementations must invoke callback once.
pub trait WebsiteDataRemover {
    fn remove(&self, identifier: Option<&str>, completion: Box<dyn FnOnce(Result<(), String>)>);
}

/// Main-thread adapter for persistent WKWebsiteDataStore deletion.
#[derive(Clone, Copy)]
pub struct NativeWebsiteDataRemover {
    mtm: MainThreadMarker,
}

impl NativeWebsiteDataRemover {
    pub fn new(mtm: MainThreadMarker) -> Self {
        Self { mtm }
    }
}

impl WebsiteDataRemover for NativeWebsiteDataRemover {
    fn remove(&self, identifier: Option<&str>, completion: Box<dyn FnOnce(Result<(), String>)>) {
        let completion = Rc::new(RefCell::new(Some(completion)));
        if let Some(identifier) = identifier {
            let uuid = NSUUID::initWithUUIDString(NSUUID::alloc(), &NSString::from_str(identifier));
            let Some(uuid) = uuid else {
                if let Some(completion) = completion.borrow_mut().take() {
                    completion(Err(
                        "profile has invalid WebKit data-store identifier".into()
                    ));
                }
                return;
            };
            let callback = RcBlock::new(move |error: *mut objc2_foundation::NSError| {
                let result = if error.is_null() {
                    Ok(())
                } else {
                    // SAFETY: WebKit supplies live NSError for duration callback.
                    let error = unsafe { &*error };
                    Err(format!(
                        "WebKit private data could not be deleted ({error})"
                    ))
                };
                if let Some(completion) = completion.borrow_mut().take() {
                    completion(result);
                }
            });
            unsafe {
                WKWebsiteDataStore::removeDataStoreForIdentifier_completionHandler(
                    &uuid, &callback, self.mtm,
                );
            }
        } else {
            let store = unsafe { WKWebsiteDataStore::defaultDataStore(self.mtm) };
            let types = unsafe { WKWebsiteDataStore::allWebsiteDataTypes(self.mtm) };
            let date = NSDate::distantPast();
            let callback = RcBlock::new(move || {
                if let Some(completion) = completion.borrow_mut().take() {
                    completion(Ok(()));
                }
            });
            unsafe {
                store.removeDataOfTypes_modifiedSince_completionHandler(&types, &date, &callback);
            }
        }
    }
}

/// Delete private files and WebKit state, then remove Account and retained
/// metadata. `profile_guard` must be acquired by launcher session tracking and
/// remains alive until asynchronous WebKit completion.
pub fn remove_account<S, W, F>(
    repository: Arc<AccountRepository<S>>,
    account_id: &str,
    support_root: &Path,
    profile_guard: ProfileLease,
    webkit: &W,
    completion: F,
) where
    S: CredentialStore + Send + Sync + 'static,
    W: WebsiteDataRemover,
    F: FnOnce(Result<(), String>) + 'static,
{
    let (account, already_retained) = match repository.list() {
        Err(error) => {
            completion(Err(error));
            return;
        }
        Ok(accounts) => match accounts
            .into_iter()
            .find(|account| account.id == account_id)
        {
            Some(account) => (account, false),
            None => {
                // A prior run may have deleted private files but failed while
                // removing retained catalog metadata. Complete that cleanup only
                // when named profile directory proves deletion already happened.
                let retained = match repository.retained() {
                    Ok(values) => values.into_iter().find(|value| value.id == account_id),
                    Err(error) => {
                        completion(Err(error));
                        return;
                    }
                };
                let Some(retained) = retained else {
                    completion(Err("unknown Account".into()));
                    return;
                };
                match deletion_already_complete(support_root, &retained.profile_id) {
                    Ok(true) => match repository.forget_retained(account_id, &NoBusyProfiles) {
                        Ok(()) => {
                            completion(Ok(()));
                            return;
                        }
                        Err(error) => {
                            completion(Err(format!(
                                "Private files are deleted; Account catalog cleanup is still pending: {error}"
                            )));
                            return;
                        }
                    },
                    Ok(false) => (
                        crate::launcher_accounts::Account {
                            format_version: 1,
                            id: retained.id,
                            profile_id: retained.profile_id,
                            nickname: retained.nickname,
                            email: retained.email,
                            auto_login: false,
                            auto_launch: false,
                            has_password: false,
                        },
                        true,
                    ),
                    Err(error) => {
                        completion(Err(error));
                        return;
                    }
                }
            }
        },
    };
    let store_id = match private_files(support_root, &account.profile_id) {
        Ok(store_id) => store_id,
        Err(error) => {
            completion(Err(error));
            return;
        }
    };
    let account_id = account_id.to_owned();
    let support_root = support_root.to_owned();
    if !already_retained {
        let removal = repository.remove(&account_id, &NoBusyProfiles);
        if let Err(error) = removal {
            completion(Err(error));
            return;
        }
    }
    let finish = move |result: Result<(), String>| {
        let _profile_guard = profile_guard;
        if let Err(error) = result {
            completion(Err(format!(
                "Account was forgotten, but private-data deletion is incomplete; remaining files are retained for retry: {error}"
            )));
            return;
        }
        match delete_private_files(&support_root, &account.profile_id) {
            Err(error) => completion(Err(format!(
                "Account was forgotten, but private-data deletion is incomplete; remaining files are retained for retry: {error}"
            ))),
            Ok(()) => match repository.forget_retained(&account_id, &NoBusyProfiles) {
                Ok(()) => completion(Ok(())),
                Err(error) => completion(Err(format!(
                    "Private files were deleted, but Account catalog cleanup is still pending; retry cleanup: {error}"
                ))),
            },
        }
    };
    webkit.remove(store_id.as_deref(), Box::new(finish));
}

/// Resolve store identifier without creating a missing profile descriptor.
fn private_files(support_root: &Path, profile_id: &str) -> Result<Option<String>, String> {
    if profile_id == "default" {
        return Ok(None);
    }
    validate_profile_id(profile_id)?;
    let descriptor = support_root
        .join("profiles")
        .join(profile_id)
        .join("profile.json");
    let profile_dir = descriptor.parent().expect("profile descriptor has parent");
    let metadata = fs::symlink_metadata(profile_dir)
        .map_err(|error| format!("could not inspect {}: {error}", profile_dir.display()))?;
    if !metadata.is_dir() {
        return Err("profile directory is not a real directory".into());
    }
    match fs::read(&descriptor) {
        Ok(bytes) => {
            let profile: crate::profile::Profile = serde_json::from_slice(&bytes)
                .map_err(|error| format!("could not parse {}: {error}", descriptor.display()))?;
            if profile.id != profile_id {
                return Err("profile descriptor does not match Account profile".into());
            }
            Ok(profile.website_data_store_id().map(str::to_owned))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(
            "named profile descriptor is missing; refusing to target default WebKit data".into(),
        ),
        Err(error) => Err(format!("could not read {}: {error}", descriptor.display())),
    }
}

fn delete_private_files(support_root: &Path, profile_id: &str) -> Result<(), String> {
    if profile_id == "default" {
        // Default profile historically shares support root with launcher and
        // chunk cache. Delete only known mutable profile-owned locations.
        for name in [
            "web",
            "shells",
            "derived",
            "generations",
            "diagnostics",
            "settings.json",
            "window.json",
        ] {
            remove_path(&support_root.join(name))?;
        }
        return Ok(());
    }
    validate_profile_id(profile_id)?;
    let profile_dir = support_root.join("profiles").join(profile_id);
    let metadata = fs::symlink_metadata(&profile_dir)
        .map_err(|error| format!("could not inspect {}: {error}", profile_dir.display()))?;
    if !metadata.is_dir() {
        return Err("profile directory is not a real directory".into());
    }
    let entries = match fs::read_dir(&profile_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("could not list {}: {error}", profile_dir.display())),
    };
    for entry in entries {
        let entry = entry.map_err(|error| format!("could not read profile directory: {error}"))?;
        if matches!(
            entry.file_name().to_str(),
            Some("gwnative.lock" | "profile.json")
        ) {
            continue;
        }
        remove_path(&entry.path())?;
    }
    // Keep descriptor, and therefore WebKit UUID, until all other deletion
    // work succeeds. A failed run can then retry against same named store.
    remove_path(&profile_dir.join("profile.json"))?;
    Ok(())
}

fn deletion_already_complete(support_root: &Path, profile_id: &str) -> Result<bool, String> {
    if profile_id == "default" {
        return Ok(false);
    }
    validate_profile_id(profile_id)?;
    let profile_dir = support_root.join("profiles").join(profile_id);
    let metadata = match fs::symlink_metadata(&profile_dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "could not inspect {}: {error}",
                profile_dir.display()
            ));
        }
    };
    if !metadata.is_dir() {
        return Ok(false);
    }
    let mut entries = fs::read_dir(&profile_dir)
        .map_err(|error| format!("could not list {}: {error}", profile_dir.display()))?;
    let Some(entry) = entries.next() else {
        return Ok(false);
    };
    if entry
        .map_err(|error| format!("could not read {}: {error}", profile_dir.display()))?
        .file_name()
        != "gwnative.lock"
    {
        return Ok(false);
    }
    Ok(entries.next().is_none())
}

fn remove_path(path: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("could not inspect {}: {error}", path.display())),
    };
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path)
    } else if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        return Err(format!("unsupported private path {}", path.display()));
    }
    .map_err(|error| format!("could not delete {}: {error}", path.display()))
}

fn validate_profile_id(profile_id: &str) -> Result<(), String> {
    if profile_id != "."
        && profile_id != ".."
        && (1..=64).contains(&profile_id.len())
        && profile_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        Ok(())
    } else {
        Err("profile ID is not a safe directory name".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launcher_accounts::{AccountDraft, StoredCredentials};
    use crate::scratch::TempDir;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct FakeCredentials(Mutex<HashMap<String, StoredCredentials>>);
    impl CredentialStore for FakeCredentials {
        fn read(&self, id: &str) -> Result<Option<StoredCredentials>, String> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .get(id)
                .map(|v| StoredCredentials::new(v.username().into(), v.password().into())))
        }
        fn save(&self, id: &str, username: &str, password: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(
                id.into(),
                StoredCredentials::new(username.into(), password.into()),
            );
            Ok(())
        }
        fn clear(&self, id: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(id);
            Ok(())
        }
    }

    type Completion = Box<dyn FnOnce(Result<(), String>)>;
    struct Deferred(RefCell<Option<Completion>>);
    impl Deferred {
        fn new() -> Self {
            Self(RefCell::new(None))
        }
        fn finish(&self, result: Result<(), String>) {
            self.0.borrow_mut().take().expect("removal callback")(result);
        }
    }
    impl WebsiteDataRemover for Deferred {
        fn remove(&self, _: Option<&str>, completion: Box<dyn FnOnce(Result<(), String>)>) {
            *self.0.borrow_mut() = Some(completion);
        }
    }

    fn descriptor(root: &Path) {
        let dir = root.join("profiles/p1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("profile.json"), r##"{"formatVersion":1,"id":"p1","displayName":"P1","color":"#fff","createdAt":0,"originPort":38113,"websiteDataStoreId":"00000000-0000-4000-8000-000000000001"}"##).unwrap();
        fs::write(dir.join("private.json"), b"private").unwrap();
        fs::write(dir.join("gwnative.lock"), b"").unwrap();
    }

    fn account(root: &TempDir) -> (Arc<AccountRepository<FakeCredentials>>, String) {
        let repository = Arc::new(AccountRepository::with_credentials(
            &root.0,
            FakeCredentials::default(),
        ));
        let account = repository
            .create(
                AccountDraft {
                    profile_id: Some("p1".into()),
                    nickname: "P1".into(),
                    email: "p1@example.test".into(),
                    password: None,
                    auto_login: Some(false),
                    auto_launch: false,
                },
                &crate::launcher_accounts::NoBusyProfiles,
            )
            .unwrap();
        descriptor(&root.0);
        (repository, account.id)
    }

    #[test]
    fn named_deletion_preserves_shared_chunks_and_lock() {
        let root = TempDir::new("launcher-removal");
        let profile = root.0.join("profiles/p1");
        fs::create_dir_all(&profile).unwrap();
        fs::write(profile.join("profile.json"), r##"{"formatVersion":1,"id":"p1","displayName":"P1","color":"#fff","createdAt":0,"originPort":38113,"websiteDataStoreId":"00000000-0000-4000-8000-000000000001"}"##).unwrap();
        fs::write(profile.join("settings.json"), b"private").unwrap();
        fs::write(profile.join("gwnative.lock"), b"").unwrap();
        fs::create_dir_all(root.0.join("chunks")).unwrap();
        fs::write(root.0.join("chunks/shared"), b"shared").unwrap();
        delete_private_files(&root.0, "p1").unwrap();
        assert!(profile.join("gwnative.lock").exists());
        assert!(!profile.join("settings.json").exists());
        assert!(root.0.join("chunks/shared").exists());
    }

    #[test]
    fn default_deletion_allowlist_preserves_catalog_and_chunks() {
        let root = TempDir::new("launcher-default-removal");
        fs::create_dir_all(root.0.join("web")).unwrap();
        fs::create_dir_all(root.0.join("chunks")).unwrap();
        fs::create_dir_all(root.0.join("launcher")).unwrap();
        fs::write(root.0.join("launcher/accounts.json"), b"catalog").unwrap();
        fs::write(root.0.join("chunks/shared"), b"shared").unwrap();
        delete_private_files(&root.0, "default").unwrap();
        assert!(!root.0.join("web").exists());
        assert!(root.0.join("launcher/accounts.json").exists());
        assert!(root.0.join("chunks/shared").exists());
    }

    #[test]
    fn malformed_profile_descriptor_fails_before_file_deletion() {
        let root = TempDir::new("launcher-removal-invalid");
        let dir = root.0.join("profiles/p1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("profile.json"), b"invalid").unwrap();
        assert!(private_files(&root.0, "p1").is_err());
    }

    #[test]
    fn only_named_profile_lock_marks_prior_file_deletion() {
        let root = TempDir::new("launcher-removal-complete-marker");
        let dir = root.0.join("profiles/p1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("gwnative.lock"), b"").unwrap();
        assert!(deletion_already_complete(&root.0, "p1").unwrap());
        fs::write(dir.join("profile.json"), b"descriptor").unwrap();
        assert!(!deletion_already_complete(&root.0, "p1").unwrap());
    }

    #[test]
    fn asynchronous_webkit_failure_keeps_retained_account_and_files() {
        let root = TempDir::new("launcher-removal-failure");
        let (repository, id) = account(&root);
        let lock = crate::instance::acquire(
            &root.0.join("profiles/p1/gwnative.lock"),
            std::time::Duration::ZERO,
        )
        .unwrap();
        let remover = Deferred::new();
        let outcome = Rc::new(RefCell::new(None));
        let received = outcome.clone();
        remove_account(
            repository.clone(),
            &id,
            &root.0,
            ProfileLease::from_instance(lock),
            &remover,
            move |result| *received.borrow_mut() = Some(result),
        );
        assert!(
            crate::instance::acquire(
                &root.0.join("profiles/p1/gwnative.lock"),
                std::time::Duration::ZERO
            )
            .is_err()
        );
        assert!(repository.list().unwrap().is_empty());
        assert_eq!(repository.retained().unwrap().len(), 1);
        assert!(root.0.join("profiles/p1/private.json").exists());
        remover.finish(Err("WebKit failed".into()));
        assert!(outcome.borrow().as_ref().unwrap().is_err());
        assert!(root.0.join("profiles/p1/private.json").exists());
        assert_eq!(repository.retained().unwrap().len(), 1);

        let retry_lock = crate::instance::acquire(
            &root.0.join("profiles/p1/gwnative.lock"),
            std::time::Duration::ZERO,
        )
        .unwrap();
        let retry_remover = Deferred::new();
        let retry_outcome = Rc::new(RefCell::new(None));
        let received = retry_outcome.clone();
        remove_account(
            repository.clone(),
            &id,
            &root.0,
            ProfileLease::from_instance(retry_lock),
            &retry_remover,
            move |result| *received.borrow_mut() = Some(result),
        );
        retry_remover.finish(Ok(()));
        assert!(retry_outcome.borrow().as_ref().unwrap().is_ok());
        assert!(!root.0.join("profiles/p1/private.json").exists());
        assert!(repository.retained().unwrap().is_empty());
    }

    #[test]
    fn asynchronous_success_removes_only_target_private_data() {
        let root = TempDir::new("launcher-removal-success");
        let (repository, id) = account(&root);
        fs::create_dir_all(root.0.join("chunks")).unwrap();
        fs::write(root.0.join("chunks/shared"), b"shared").unwrap();
        let _lock = crate::instance::acquire(
            &root.0.join("profiles/p1/gwnative.lock"),
            std::time::Duration::ZERO,
        )
        .unwrap();
        let remover = Deferred::new();
        let outcome = Rc::new(RefCell::new(None));
        let received = outcome.clone();
        remove_account(
            repository.clone(),
            &id,
            &root.0,
            ProfileLease::unlocked(),
            &remover,
            move |result| *received.borrow_mut() = Some(result),
        );
        remover.finish(Ok(()));
        assert!(outcome.borrow().as_ref().unwrap().is_ok());
        assert!(!root.0.join("profiles/p1/private.json").exists());
        assert!(!root.0.join("profiles/p1/profile.json").exists());
        assert!(root.0.join("profiles/p1/gwnative.lock").exists());
        assert!(root.0.join("chunks/shared").exists());
        assert!(repository.retained().unwrap().is_empty());
    }
}
