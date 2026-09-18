//! Launcher-owned Account catalog.
//!
//! Account metadata is deliberately separate from game profiles and Keychain
//! payloads. The catalog stores no password; `CredentialStore` owns that
//! boundary. Profile files are never removed here: a later integration layer
//! must coordinate WebKit's out-of-tree store before offering destructive
//! private-data deletion.

use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::time::Duration;

use objc2_foundation::NSUUID;
use serde::{Deserialize, Serialize};

use crate::instance;

const FORMAT: u32 = 1;
const CATALOG_DIR: &str = "launcher";
const CATALOG_FILE: &str = "accounts.json";
const LOCK_FILE: &str = "accounts.lock";

/// Profile activity supplied by launcher session tracking.
pub trait ProfileBusy {
    fn is_busy(&self, profile_id: &str) -> bool;
    fn acquire_exclusive(&self, profile_id: &str) -> Result<ProfileLease, String> {
        if self.is_busy(profile_id) {
            Err("close the Account's game before changing its private context".into())
        } else {
            Ok(ProfileLease::unlocked())
        }
    }
}

/// Guard held through credential and catalog mutation. Launcher session
/// integration can attach its profile lock; tests use unlocked guard.
pub struct ProfileLease {
    _lock: Option<instance::Instance>,
}

impl ProfileLease {
    pub fn unlocked() -> Self {
        Self { _lock: None }
    }
    pub fn from_instance(lock: instance::Instance) -> Self {
        Self { _lock: Some(lock) }
    }
}

/// A no-op activity check for creation/import and tests.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoBusyProfiles;

impl ProfileBusy for NoBusyProfiles {
    fn is_busy(&self, _profile_id: &str) -> bool {
        false
    }
}

/// Credential operations needed by Account mutations.
///
/// Implementations must keep password material out of catalog serialization.
/// `KeychainCredentialStore` adapts existing process-local Keychain functions;
/// launcher integration should provide one instance per operation boundary and
/// must prevent game-side save/clear callbacks from using this authority.
pub trait CredentialStore {
    fn read(&self, profile_id: &str) -> Result<Option<StoredCredentials>, String>;
    fn save(&self, profile_id: &str, username: &str, password: &str) -> Result<(), String>;
    fn clear(&self, profile_id: &str) -> Result<(), String>;
}

/// Password-bearing value used only inside credential operations.
///
/// It has no `Serialize` implementation, so accidentally adding it to the
/// catalog schema is a compile-time/API mistake rather than a silent field.
#[derive(PartialEq, Eq)]
pub struct StoredCredentials {
    username: String,
    password: String,
}

impl std::fmt::Debug for StoredCredentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredCredentials")
            .field("username", &self.username)
            .field("password", &"[redacted]")
            .finish()
    }
}

impl Drop for StoredCredentials {
    fn drop(&mut self) {
        crate::log::wipe_string(&mut self.username);
        crate::log::wipe_string(&mut self.password);
    }
}

impl StoredCredentials {
    pub fn new(username: String, password: String) -> Self {
        Self { username, password }
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn password(&self) -> &str {
        &self.password
    }
}

/// Existing Keychain adapter. It reads the complete protected pair internally
/// because the current Keychain API has no password-free existence query.
#[derive(Clone, Copy, Debug, Default)]
pub struct KeychainCredentialStore;

impl CredentialStore for KeychainCredentialStore {
    fn read(&self, profile_id: &str) -> Result<Option<StoredCredentials>, String> {
        let account = keychain_account(profile_id);
        crate::keychain::launcher_read(&account).map(|credentials| {
            credentials.map(|(username, password)| StoredCredentials::new(username, password))
        })
    }

    fn save(&self, profile_id: &str, username: &str, password: &str) -> Result<(), String> {
        let account = keychain_account(profile_id);
        crate::keychain::launcher_store(&account, username, password)
    }

    fn clear(&self, profile_id: &str) -> Result<(), String> {
        crate::keychain::launcher_clear(&keychain_account(profile_id))
    }
}

fn keychain_account(profile_id: &str) -> String {
    if profile_id == "default" {
        "login".to_owned()
    } else {
        format!("login:{profile_id}")
    }
}

/// Launcher Account metadata. Password is represented only by `has_password`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub format_version: u32,
    pub id: String,
    pub profile_id: String,
    pub nickname: String,
    pub email: String,
    pub auto_login: bool,
    pub auto_launch: bool,
    pub has_password: bool,
}

/// Input for adding an Account. `profile_id` is supplied for adoption; normal
/// creation leaves it absent and receives a stable private profile identity.
#[derive(PartialEq, Eq)]
pub struct AccountDraft {
    pub profile_id: Option<String>,
    pub nickname: String,
    pub email: String,
    pub password: Option<String>,
    pub auto_login: Option<bool>,
    pub auto_launch: bool,
}

impl std::fmt::Debug for AccountDraft {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccountDraft")
            .field("profile_id", &self.profile_id)
            .field("nickname", &self.nickname)
            .field("email", &self.email)
            .field("password", &self.password.as_ref().map(|_| "[redacted]"))
            .field("auto_login", &self.auto_login)
            .field("auto_launch", &self.auto_launch)
            .finish()
    }
}

impl Drop for AccountDraft {
    fn drop(&mut self) {
        if let Some(password) = self.password.as_mut() {
            crate::log::wipe_string(password);
        }
    }
}

/// Editable fields. Email changes require `preserve_context = true` and a
/// non-busy profile; that operation clears credentials and Auto-login.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountPatch {
    pub nickname: Option<String>,
    pub email: Option<String>,
    pub auto_login: Option<bool>,
    pub auto_launch: Option<bool>,
    pub preserve_context: bool,
}

/// Password operation submitted with one complete launcher form.
pub enum PasswordChange {
    Keep,
    Set(String),
    Remove,
}

impl Drop for PasswordChange {
    fn drop(&mut self) {
        if let Self::Set(password) = self {
            crate::log::wipe_string(password);
        }
    }
}

/// Removal result. Profile files remain on disk for later explicit reuse/fresh
/// choice; no WebKit data is claimed to have been deleted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovedAccount {
    pub account: Account,
    pub credentials_cleared: bool,
    pub profile_retained: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetainedAccount {
    pub id: String,
    pub profile_id: String,
    pub nickname: String,
    pub email: String,
}

/// Username-only import/adoption preview. Password never crosses this API.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CredentialPreview {
    pub username: Option<String>,
    pub has_password: bool,
}

/// Durable Account catalog rooted at Application Support/gwnative.
pub struct AccountRepository<S = KeychainCredentialStore> {
    base: PathBuf,
    credentials: S,
}

impl AccountRepository<KeychainCredentialStore> {
    pub fn open(base: impl Into<PathBuf>) -> Self {
        Self::with_credentials(base, KeychainCredentialStore)
    }
}

impl<S: CredentialStore> AccountRepository<S> {
    pub fn with_credentials(base: impl Into<PathBuf>, credentials: S) -> Self {
        Self {
            base: base.into(),
            credentials,
        }
    }

    pub fn list(&self) -> Result<Vec<Account>, String> {
        let _lock = self.lock()?;
        Ok(self.read_state()?.accounts)
    }

    pub fn retained(&self) -> Result<Vec<RetainedAccount>, String> {
        let _lock = self.lock()?;
        Ok(self.read_state()?.retained)
    }

    pub fn credential_preview(&self, profile_id: &str) -> Result<CredentialPreview, String> {
        let credentials = self.credentials.read(profile_id)?;
        Ok(CredentialPreview {
            has_password: credentials.is_some(),
            username: credentials.map(|value| value.username().to_owned()),
        })
    }

    pub fn forget_retained<B: ProfileBusy>(&self, id: &str, busy: &B) -> Result<(), String> {
        let _lock = self.lock()?;
        let mut state = self.read_state()?;
        let index = state
            .retained
            .iter()
            .position(|value| value.id == id)
            .ok_or_else(|| format!("unknown retained Account {id:?}"))?;
        let _profile_lock = busy.acquire_exclusive(&state.retained[index].profile_id)?;
        state.retained.remove(index);
        self.write_catalog(&state.accounts, &state.retained)
    }

    #[cfg(test)]
    pub fn create<B: ProfileBusy>(&self, draft: AccountDraft, busy: &B) -> Result<Account, String> {
        self.create_with_profile(draft, busy, |_| Ok(()))
    }

    pub fn create_with_profile<B, F>(
        &self,
        draft: AccountDraft,
        busy: &B,
        prepare: F,
    ) -> Result<Account, String>
    where
        B: ProfileBusy,
        F: FnOnce(&str) -> Result<(), String>,
    {
        let _lock = self.lock()?;
        let mut state = self.read_state()?;
        let accounts = &mut state.accounts;
        let email = clean_email(&draft.email)?;
        ensure_unique_email(accounts, &email, None)?;
        let id = new_id();
        let profile_id = draft
            .profile_id
            .clone()
            .unwrap_or_else(|| profile_id_for(&id));
        validate_profile_id(&profile_id)?;
        if accounts
            .iter()
            .any(|account| account.profile_id == profile_id)
        {
            return Err(format!(
                "profile {profile_id:?} is already assigned to an Account"
            ));
        }
        let _profile_lock = busy.acquire_exclusive(&profile_id)?;
        let nickname = clean_nickname(&draft.nickname)?;
        if draft
            .password
            .as_deref()
            .is_some_and(|password| password.is_empty())
        {
            return Err("password cannot be empty".into());
        }
        let has_password = draft.password.is_some();
        if draft.auto_login == Some(true) && !has_password {
            return Err("Auto-login requires a saved password".into());
        }
        let auto_login = draft.auto_login.unwrap_or(has_password) && has_password;
        let account = Account {
            format_version: FORMAT,
            id,
            profile_id,
            nickname,
            email,
            auto_login,
            auto_launch: draft.auto_launch,
            has_password: false,
        };
        prepare(&account.profile_id)?;
        let old_credentials = self.credentials.read(&account.profile_id)?;
        let credential_result = if let Some(password) = draft.password.as_deref() {
            self.credentials
                .save(&account.profile_id, &account.email, password)
        } else {
            self.credentials.clear(&account.profile_id)
        };
        if let Err(error) = credential_result {
            if let Some(old) = old_credentials.as_ref() {
                let _ = self
                    .credentials
                    .save(&account.profile_id, old.username(), old.password());
            }
            return Err(error);
        }
        let mut persisted = account.clone();
        persisted.has_password = has_password;
        accounts.push(persisted.clone());
        state
            .retained
            .retain(|value| value.profile_id != account.profile_id);
        if let Err(error) = self.write_catalog(&state.accounts, &state.retained) {
            if let Some(old) = old_credentials {
                let _ = self
                    .credentials
                    .save(&account.profile_id, old.username(), old.password());
            } else {
                let _ = self.credentials.clear(&account.profile_id);
            }
            return Err(error);
        }
        Ok(persisted)
    }

    pub fn update<B: ProfileBusy>(
        &self,
        id: &str,
        patch: AccountPatch,
        busy: &B,
    ) -> Result<Account, String> {
        let _lock = self.lock()?;
        let mut state = self.read_state()?;
        let accounts = &mut state.accounts;
        let index = find_index(accounts, id)?;
        let current = accounts[index].clone();
        let profile_busy = busy.is_busy(&current.profile_id);
        let email_changed = patch.email.as_deref().is_some_and(|email| {
            clean_email(email).ok().as_deref() != Some(current.email.as_str())
        });
        if profile_busy && email_changed {
            return Err("close the Account's game before changing its login email".into());
        }
        if patch.preserve_context && !email_changed {
            return Err("context confirmation requires a login email change".into());
        }
        let mut next = current.clone();
        if let Some(nickname) = patch.nickname {
            next.nickname = clean_nickname(&nickname)?;
        }
        if let Some(email) = patch.email {
            let email = clean_email(&email)?;
            if email != current.email {
                if !patch.preserve_context {
                    return Err("confirm that this Account keeps its private context".into());
                }
                ensure_unique_email(accounts, &email, Some(id))?;
                next.email = email;
                next.auto_login = false;
                next.has_password = false;
            }
        }
        if let Some(auto_launch) = patch.auto_launch {
            next.auto_launch = auto_launch;
        }
        if let Some(auto_login) = patch.auto_login {
            if auto_login && !next.has_password {
                return Err("Auto-login requires a saved password".into());
            }
            next.auto_login = auto_login;
        }

        if email_changed {
            let _profile_lock = busy.acquire_exclusive(&current.profile_id)?;
            let old_credentials = self.credentials.read(&current.profile_id)?;
            if old_credentials.is_some() {
                self.credentials.clear(&current.profile_id)?;
            }
            accounts[index] = next.clone();
            if let Err(error) = self.write_catalog(&state.accounts, &state.retained) {
                if let Some(old) = old_credentials {
                    let _ =
                        self.credentials
                            .save(&current.profile_id, old.username(), old.password());
                }
                return Err(error);
            }
            return Ok(next);
        }

        accounts[index] = next.clone();
        self.write_catalog(&state.accounts, &state.retained)?;
        Ok(next)
    }

    /// Apply metadata and password fields as one catalog/credential mutation.
    /// Validation completes before Keychain changes; failures restore prior
    /// credential bytes before returning.
    pub fn save_form<B: ProfileBusy>(
        &self,
        id: &str,
        patch: AccountPatch,
        password: PasswordChange,
        busy: &B,
    ) -> Result<Account, String> {
        let _lock = self.lock()?;
        let mut state = self.read_state()?;
        let index = find_index(&state.accounts, id)?;
        let current = state.accounts[index].clone();
        let email_changed = patch.email.as_deref().is_some_and(|email| {
            clean_email(email).ok().as_deref() != Some(current.email.as_str())
        });
        if patch.preserve_context && !email_changed {
            return Err("context confirmation requires a login email change".into());
        }
        if email_changed && !patch.preserve_context {
            return Err("confirm that this Account keeps its private context".into());
        }
        let password_changes = !matches!(password, PasswordChange::Keep);
        let _profile_lock = if email_changed || password_changes {
            Some(busy.acquire_exclusive(&current.profile_id)?)
        } else {
            None
        };
        let mut next = current.clone();
        if let Some(nickname) = patch.nickname {
            next.nickname = clean_nickname(&nickname)?;
        }
        if let Some(email) = patch.email {
            let email = clean_email(&email)?;
            if email != current.email {
                ensure_unique_email(&state.accounts, &email, Some(id))?;
                next.email = email;
                next.auto_login = false;
                next.has_password = false;
            }
        }
        if let Some(auto_launch) = patch.auto_launch {
            next.auto_launch = auto_launch;
        }
        if let Some(auto_login) = patch.auto_login {
            next.auto_login = auto_login;
        }
        match &password {
            PasswordChange::Keep if email_changed => {
                next.has_password = false;
                next.auto_login = false;
            }
            PasswordChange::Set(value) if value.is_empty() => {
                return Err("password cannot be empty".into());
            }
            PasswordChange::Set(_) => next.has_password = true,
            PasswordChange::Remove => {
                next.has_password = false;
                next.auto_login = false;
            }
            PasswordChange::Keep => {}
        }
        if next.auto_login && !next.has_password {
            return Err("Auto-login requires a saved password".into());
        }
        let mut candidate = state.accounts.clone();
        candidate[index] = next.clone();
        validate_accounts(&candidate)?;

        let old_credentials = if email_changed || password_changes {
            self.credentials.read(&current.profile_id)?
        } else {
            None
        };
        if email_changed || password_changes {
            let credential_result = match &password {
                PasswordChange::Set(value) => {
                    self.credentials
                        .save(&current.profile_id, &next.email, value)
                }
                PasswordChange::Remove | PasswordChange::Keep => {
                    self.credentials.clear(&current.profile_id)
                }
            };
            if let Err(error) = credential_result {
                if let Some(old) = old_credentials.as_ref() {
                    let _ =
                        self.credentials
                            .save(&current.profile_id, old.username(), old.password());
                } else {
                    let _ = self.credentials.clear(&current.profile_id);
                }
                return Err(error);
            }
        }
        state.accounts = candidate;
        if let Err(error) = self.write_catalog(&state.accounts, &state.retained) {
            if let Some(old) = old_credentials {
                let _ = self
                    .credentials
                    .save(&current.profile_id, old.username(), old.password());
            } else if email_changed || password_changes {
                let _ = self.credentials.clear(&current.profile_id);
            }
            return Err(error);
        }
        Ok(next)
    }

    #[cfg(test)]
    pub fn save_password<B: ProfileBusy>(
        &self,
        id: &str,
        password: &str,
        busy: &B,
    ) -> Result<Account, String> {
        let _lock = self.lock()?;
        let mut state = self.read_state()?;
        let accounts = &mut state.accounts;
        let index = find_index(accounts, id)?;
        let current = accounts[index].clone();
        let _profile_lock = busy.acquire_exclusive(&current.profile_id)?;
        if password.is_empty() {
            return Err("password cannot be empty".into());
        }
        let old_credentials = self.credentials.read(&current.profile_id)?;
        self.credentials
            .save(&current.profile_id, &current.email, password)?;
        let mut next = current.clone();
        next.has_password = true;
        accounts[index] = next.clone();
        if let Err(error) = self.write_catalog(&state.accounts, &state.retained) {
            if let Some(old) = old_credentials {
                let _ = self
                    .credentials
                    .save(&next.profile_id, old.username(), old.password());
            } else {
                let _ = self.credentials.clear(&next.profile_id);
            }
            return Err(error);
        }
        Ok(next)
    }

    #[cfg(test)]
    pub fn clear_password<B: ProfileBusy>(&self, id: &str, busy: &B) -> Result<Account, String> {
        let _lock = self.lock()?;
        let mut state = self.read_state()?;
        let accounts = &mut state.accounts;
        let index = find_index(accounts, id)?;
        let current = accounts[index].clone();
        let _profile_lock = busy.acquire_exclusive(&current.profile_id)?;
        let old_credentials = self.credentials.read(&current.profile_id)?;
        self.credentials.clear(&current.profile_id)?;
        let mut next = current.clone();
        next.has_password = false;
        next.auto_login = false;
        accounts[index] = next.clone();
        if let Err(error) = self.write_catalog(&state.accounts, &state.retained) {
            if let Some(old) = old_credentials {
                let _ = self
                    .credentials
                    .save(&current.profile_id, old.username(), old.password());
            }
            return Err(format!(
                "password cleared but Account metadata was not saved: {error}"
            ));
        }
        Ok(next)
    }

    pub fn remove<B: ProfileBusy>(&self, id: &str, busy: &B) -> Result<RemovedAccount, String> {
        let _lock = self.lock()?;
        let mut state = self.read_state()?;
        let accounts = &mut state.accounts;
        let index = find_index(accounts, id)?;
        let account = accounts[index].clone();
        let _profile_lock = busy.acquire_exclusive(&account.profile_id)?;
        let old_credentials = self.credentials.read(&account.profile_id)?;
        let credentials_cleared = old_credentials.is_some();
        self.credentials.clear(&account.profile_id)?;
        accounts.remove(index);
        state.retained.push(RetainedAccount {
            id: account.id.clone(),
            profile_id: account.profile_id.clone(),
            nickname: account.nickname.clone(),
            email: account.email.clone(),
        });
        if let Err(error) = self.write_catalog(&state.accounts, &state.retained) {
            if let Some(old) = old_credentials {
                let _ = self
                    .credentials
                    .save(&account.profile_id, old.username(), old.password());
            }
            return Err(format!(
                "credentials cleared but Account metadata was not saved: {error}"
            ));
        }
        Ok(RemovedAccount {
            account,
            credentials_cleared,
            profile_retained: true,
        })
    }

    fn lock(&self) -> Result<instance::Instance, String> {
        instance::acquire(&self.lock_path(), Duration::from_secs(5))
    }

    fn catalog_path(&self) -> PathBuf {
        self.base.join(CATALOG_DIR).join(CATALOG_FILE)
    }

    fn lock_path(&self) -> PathBuf {
        self.base.join(CATALOG_DIR).join(LOCK_FILE)
    }

    fn read_state(&self) -> Result<CatalogState, String> {
        let path = self.catalog_path();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(CatalogState::default());
            }
            Err(error) => return Err(format!("could not read {}: {error}", path.display())),
        };
        let file: Catalog = serde_json::from_slice(&bytes)
            .map_err(|error| format!("could not parse {}: {error}", path.display()))?;
        if file.format_version != FORMAT {
            return Err(format!(
                "{} uses unsupported account format {}",
                path.display(),
                file.format_version
            ));
        }
        validate_accounts(&file.accounts)?;
        validate_retained(&file.retained)?;
        Ok(CatalogState {
            accounts: file.accounts,
            retained: file.retained,
        })
    }

    fn write_catalog(
        &self,
        accounts: &[Account],
        retained: &[RetainedAccount],
    ) -> Result<(), String> {
        validate_accounts(accounts)?;
        validate_retained(retained)?;
        let path = self.catalog_path();
        let parent = path.parent().expect("catalog has a parent");
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        let bytes = serde_json::to_vec_pretty(&Catalog {
            format_version: FORMAT,
            accounts: accounts.to_vec(),
            retained: retained.to_vec(),
        })
        .map_err(|error| error.to_string())?;
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        let result = (|| -> std::io::Result<()> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, &path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.map_err(|error| format!("could not save {}: {error}", path.display()))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
    format_version: u32,
    accounts: Vec<Account>,
    #[serde(default)]
    retained: Vec<RetainedAccount>,
}

#[derive(Default)]
struct CatalogState {
    accounts: Vec<Account>,
    retained: Vec<RetainedAccount>,
}

fn new_id() -> String {
    NSUUID::UUID().UUIDString().to_string().to_ascii_lowercase()
}

fn profile_id_for(id: &str) -> String {
    format!("account-{}", id.replace('-', ""))
}

fn clean_nickname(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("nickname cannot be empty".into());
    }
    if value.chars().any(char::is_control) {
        return Err("nickname contains a control character".into());
    }
    Ok(value.to_owned())
}

fn clean_email(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err("login email cannot be empty or contain control characters".into());
    }
    Ok(value.to_owned())
}

fn email_key(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn ensure_unique_email(
    accounts: &[Account],
    email: &str,
    except_id: Option<&str>,
) -> Result<(), String> {
    let key = email_key(email);
    if accounts
        .iter()
        .any(|account| Some(account.id.as_str()) != except_id && email_key(&account.email) == key)
    {
        return Err("that login email already belongs to another Account".into());
    }
    Ok(())
}

fn validate_profile_id(profile_id: &str) -> Result<(), String> {
    if profile_id == "default"
        || (profile_id != "."
            && profile_id != ".."
            && (1..=64).contains(&profile_id.len())
            && profile_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')))
    {
        return Ok(());
    }
    Err(format!(
        "profile ID {profile_id:?} is not a safe directory name"
    ))
}

fn validate_accounts(accounts: &[Account]) -> Result<(), String> {
    let mut ids = Vec::new();
    let mut profiles = Vec::new();
    for account in accounts {
        if account.format_version != FORMAT {
            return Err(format!("Account {:?} uses unsupported format", account.id));
        }
        if account.id.trim().is_empty() || ids.iter().any(|id| id == &account.id) {
            return Err(format!("duplicate or empty Account ID {:?}", account.id));
        }
        validate_profile_id(&account.profile_id)?;
        if profiles
            .iter()
            .any(|profile| profile == &account.profile_id)
        {
            return Err(format!(
                "profile {:?} is assigned more than once",
                account.profile_id
            ));
        }
        clean_nickname(&account.nickname)?;
        let email = clean_email(&account.email)?;
        ensure_unique_email(accounts, &email, Some(&account.id))?;
        if account.auto_login && !account.has_password {
            return Err(format!(
                "Account {:?} enables Auto-login without a password",
                account.id
            ));
        }
        ids.push(account.id.clone());
        profiles.push(account.profile_id.clone());
    }
    Ok(())
}

fn validate_retained(retained: &[RetainedAccount]) -> Result<(), String> {
    for value in retained {
        if value.id.trim().is_empty() {
            return Err("retained Account has empty ID".into());
        }
        validate_profile_id(&value.profile_id)?;
        clean_nickname(&value.nickname)?;
        clean_email(&value.email)?;
    }
    Ok(())
}

fn find_index(accounts: &[Account], id: &str) -> Result<usize, String> {
    accounts
        .iter()
        .position(|account| account.id == id)
        .ok_or_else(|| format!("unknown Account {id:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::TempDir;
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeCredentials {
        values: RefCell<std::collections::HashMap<String, StoredCredentials>>,
    }

    impl CredentialStore for FakeCredentials {
        fn read(&self, profile_id: &str) -> Result<Option<StoredCredentials>, String> {
            Ok(self.values.borrow().get(profile_id).map(|value| {
                StoredCredentials::new(value.username().to_owned(), value.password().to_owned())
            }))
        }

        fn save(&self, profile_id: &str, username: &str, password: &str) -> Result<(), String> {
            self.values.borrow_mut().insert(
                profile_id.to_owned(),
                StoredCredentials::new(username.to_owned(), password.to_owned()),
            );
            Ok(())
        }

        fn clear(&self, profile_id: &str) -> Result<(), String> {
            self.values.borrow_mut().remove(profile_id);
            Ok(())
        }
    }

    struct Busy(String);

    impl ProfileBusy for Busy {
        fn is_busy(&self, profile_id: &str) -> bool {
            self.0 == profile_id
        }
    }

    fn repo() -> (TempDir, AccountRepository<FakeCredentials>) {
        let temp = TempDir::new("launcher-accounts");
        let repo = AccountRepository::with_credentials(&temp.0, FakeCredentials::default());
        (temp, repo)
    }

    fn draft(email: &str) -> AccountDraft {
        AccountDraft {
            profile_id: None,
            nickname: "Main".into(),
            email: email.into(),
            password: Some("secret".into()),
            auto_login: None,
            auto_launch: false,
        }
    }

    #[test]
    fn creates_stable_profile_and_never_serializes_password() {
        let (_temp, repo) = repo();
        let account = repo
            .create(draft("Player@example.test"), &NoBusyProfiles)
            .unwrap();
        let listed = repo.list().unwrap();
        assert_eq!(listed, vec![account.clone()]);
        assert!(account.auto_login);
        assert!(account.has_password);
        let json = std::fs::read_to_string(repo.catalog_path()).unwrap();
        assert!(!json.contains("secret"));
        assert!(!json.contains("password"));
        assert!(account.profile_id.starts_with("account-"));
    }

    #[test]
    fn password_debug_output_is_redacted_and_preview_exposes_username_only() {
        let (_temp, repo) = repo();
        let account = repo
            .create(draft("player@example.test"), &NoBusyProfiles)
            .unwrap();
        let credentials = StoredCredentials::new("player@example.test".into(), "secret".into());
        assert!(!format!("{credentials:?}").contains("secret"));
        let preview = repo.credential_preview(&account.profile_id).unwrap();
        assert_eq!(preview.username.as_deref(), Some("player@example.test"));
        assert!(preview.has_password);
    }

    #[test]
    fn duplicate_email_is_rejected_case_insensitively() {
        let (_temp, repo) = repo();
        repo.create(draft("player@example.test"), &NoBusyProfiles)
            .unwrap();
        let error = repo
            .create(draft(" PLAYER@EXAMPLE.TEST "), &NoBusyProfiles)
            .unwrap_err();
        assert!(error.contains("already belongs"));
    }

    #[test]
    fn password_rules_are_enforced() {
        let (_temp, repo) = repo();
        let mut no_password = draft("one@example.test");
        no_password.password = None;
        no_password.auto_login = Some(true);
        let error = repo.create(no_password, &NoBusyProfiles).unwrap_err();
        assert!(error.contains("Auto-login"));

        let account = repo
            .create(draft("one@example.test"), &NoBusyProfiles)
            .unwrap();
        let account = repo.clear_password(&account.id, &NoBusyProfiles).unwrap();
        assert!(!account.has_password);
        assert!(!account.auto_login);
        let account = repo
            .save_password(&account.id, "new-secret", &NoBusyProfiles)
            .unwrap();
        assert!(account.has_password);
        assert!(!account.auto_login);
    }

    #[test]
    fn save_form_commits_metadata_and_password_together() {
        let (_temp, repo) = repo();
        let account = repo
            .create(draft("one@example.test"), &NoBusyProfiles)
            .unwrap();
        let edited = repo
            .save_form(
                &account.id,
                AccountPatch {
                    nickname: Some("Renamed".into()),
                    auto_login: Some(true),
                    ..Default::default()
                },
                PasswordChange::Set("replacement".into()),
                &NoBusyProfiles,
            )
            .unwrap();
        assert_eq!(edited.nickname, "Renamed");
        assert!(edited.has_password && edited.auto_login);
        assert_eq!(
            repo.credential_preview(&edited.profile_id)
                .unwrap()
                .username
                .as_deref(),
            Some("one@example.test")
        );
    }

    #[test]
    fn save_form_validates_before_changing_existing_credential() {
        let (_temp, repo) = repo();
        let account = repo
            .create(draft("one@example.test"), &NoBusyProfiles)
            .unwrap();
        let error = repo
            .save_form(
                &account.id,
                AccountPatch {
                    nickname: Some("".into()),
                    ..Default::default()
                },
                PasswordChange::Set("replacement".into()),
                &NoBusyProfiles,
            )
            .unwrap_err();
        assert!(error.contains("nickname"));
        let preview = repo.credential_preview(&account.profile_id).unwrap();
        assert_eq!(preview.username.as_deref(), Some("one@example.test"));
    }

    #[test]
    fn email_change_requires_confirmation_and_clears_password() {
        let (_temp, repo) = repo();
        let account = repo
            .create(draft("one@example.test"), &NoBusyProfiles)
            .unwrap();
        let error = repo
            .update(
                &account.id,
                AccountPatch {
                    email: Some("two@example.test".into()),
                    ..Default::default()
                },
                &NoBusyProfiles,
            )
            .unwrap_err();
        assert!(error.contains("confirm"));
        let changed = repo
            .update(
                &account.id,
                AccountPatch {
                    email: Some("two@example.test".into()),
                    preserve_context: true,
                    ..Default::default()
                },
                &NoBusyProfiles,
            )
            .unwrap();
        assert_eq!(changed.email, "two@example.test");
        assert!(!changed.has_password);
        assert!(!changed.auto_login);
    }

    #[test]
    fn running_profile_allows_nickname_and_toggles_but_blocks_identity_changes() {
        let (_temp, repo) = repo();
        let account = repo
            .create(draft("one@example.test"), &NoBusyProfiles)
            .unwrap();
        let edited = repo
            .update(
                &account.id,
                AccountPatch {
                    nickname: Some("Playing".into()),
                    auto_launch: Some(true),
                    ..Default::default()
                },
                &Busy(account.profile_id.clone()),
            )
            .unwrap();
        assert_eq!(edited.nickname, "Playing");
        let error = repo
            .save_password(&account.id, "other", &Busy(account.profile_id.clone()))
            .unwrap_err();
        assert!(error.contains("close"));
    }

    #[test]
    fn removal_clears_credentials_but_retains_profile() {
        let (_temp, repo) = repo();
        let account = repo
            .create(draft("one@example.test"), &NoBusyProfiles)
            .unwrap();
        let removed = repo.remove(&account.id, &NoBusyProfiles).unwrap();
        assert!(removed.credentials_cleared);
        assert!(removed.profile_retained);
        assert!(repo.list().unwrap().is_empty());
        assert_eq!(repo.retained().unwrap()[0].profile_id, account.profile_id);
        repo.forget_retained(&account.id, &NoBusyProfiles).unwrap();
        assert!(repo.retained().unwrap().is_empty());
    }

    #[test]
    fn adoption_can_bind_default_profile_without_creating_a_password() {
        let (_temp, repo) = repo();
        let mut imported = draft("one@example.test");
        imported.profile_id = Some("default".into());
        imported.password = None;
        imported.auto_login = Some(false);
        let account = repo.create(imported, &NoBusyProfiles).unwrap();
        assert_eq!(account.profile_id, "default");
        assert!(!account.auto_launch);
        assert!(!account.has_password);
    }

    #[test]
    fn adoption_without_password_clears_existing_profile_credential() {
        let (_temp, repo) = repo();
        repo.credentials
            .save("default", "old@example.test", "old-secret")
            .unwrap();
        let mut imported = draft("new@example.test");
        imported.profile_id = Some("default".into());
        imported.password = None;
        imported.auto_login = Some(false);
        repo.create(imported, &NoBusyProfiles).unwrap();
        assert!(!repo.credential_preview("default").unwrap().has_password);
    }

    #[test]
    fn profile_preparation_failure_leaves_catalog_and_credentials_unchanged() {
        let (_temp, repo) = repo();
        let mut imported = draft("one@example.test");
        imported.profile_id = Some("default".into());
        let error = repo
            .create_with_profile(imported, &NoBusyProfiles, |_| Err("prepare failed".into()))
            .unwrap_err();
        assert_eq!(error, "prepare failed");
        assert!(repo.list().unwrap().is_empty());
        assert!(!repo.credential_preview("default").unwrap().has_password);
    }
}
