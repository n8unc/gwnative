//! Account launcher. Its private WebView has no network authority or game state.
//! Game processes retain their own lifecycle, profile locks and storage.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use block2::DynBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate,
    NSApplicationTerminateReply, NSBackingStoreType, NSMenu, NSMenuItem, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};
use objc2_web_kit::{
    WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKScriptMessage,
    WKScriptMessageHandler, WKUserContentController, WKWebView, WKWebViewConfiguration,
    WKWebsiteDataStore,
};
use serde_json::{Value, json};

use crate::launcher_accounts::{
    Account, AccountDraft, AccountPatch, AccountRepository, CredentialStore,
    KeychainCredentialStore, LaunchGroupDraft, PasswordChange, ProfileBusy, ProfileLease,
};
use crate::launcher_preferences::{
    self as launch_options, FrameRateLimit, InvocationOverrides, LaunchDefaults, LaunchFrame,
    WindowMode,
};
use crate::launcher_sessions::{
    CurrentExecutable, GameHost, HostCommand, IpcProbe, LaunchSpec, SessionController, SessionState,
};
use crate::{app, cli, dock, instance, paths, profile};

type Sessions = SessionController<CurrentExecutable, IpcProbe>;
thread_local! { static STATE: RefCell<Option<State>> = const { RefCell::new(None) }; }
static MANAGED_GAME: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static MANAGED_AUTO_LOGIN: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn managed_game() -> bool {
    MANAGED_GAME.load(std::sync::atomic::Ordering::Acquire)
}

/// Auto-login choice captured with Account selected for this game process.
///
/// WebView preamble must not reread mutable launcher metadata after startup:
/// that could pair later Account edit with already selected profile.
pub fn managed_auto_login() -> bool {
    MANAGED_AUTO_LOGIN.load(std::sync::atomic::Ordering::Acquire)
}

pub fn game_account(profile_id: &str) -> Result<Option<Account>, String> {
    Ok(AccountRepository::open(paths::base_support_dir())
        .list()?
        .into_iter()
        .find(|a| a.profile_id == profile_id))
}

pub fn register_game(profile_id: &str) -> Result<Option<Account>, String> {
    let account = game_account(profile_id)?;
    MANAGED_GAME.store(account.is_some(), std::sync::atomic::Ordering::Release);
    MANAGED_AUTO_LOGIN.store(
        std::env::var("GWNATIVE_ACCOUNT_AUTO_LOGIN")
            .ok()
            .and_then(|s| s.parse::<bool>().ok())
            .unwrap_or_else(|| {
                account
                    .as_ref()
                    .is_some_and(|account| account.auto_login && account.has_password)
            }),
        std::sync::atomic::Ordering::Release,
    );
    Ok(account)
}

struct Busy {
    base: PathBuf,
    queued: HashSet<String>,
}
impl ProfileBusy for Busy {
    fn is_busy(&self, id: &str) -> bool {
        self.queued.contains(id)
            || instance::acquire(
                &support(&self.base, id).join("gwnative.lock"),
                Duration::ZERO,
            )
            .is_err()
    }
    fn acquire_exclusive(&self, id: &str) -> Result<ProfileLease, String> {
        if self.queued.contains(id) {
            return Err("Close this Account’s game before changing its login or files.".into());
        }
        instance::acquire(
            &support(&self.base, id).join("gwnative.lock"),
            Duration::ZERO,
        )
        .map(ProfileLease::from_instance)
    }
}
fn support(base: &Path, id: &str) -> PathBuf {
    if id == "default" {
        base.to_owned()
    } else {
        base.join("profiles").join(id)
    }
}

struct State {
    base: PathBuf,
    repository: Arc<AccountRepository>,
    sessions: Sessions,
    window: Retained<NSWindow>,
    webview: Retained<WKWebView>,
    _lock: instance::Instance,
    update: Arc<Mutex<(bool, String)>>,
    offline: bool,
    invocation_overrides: InvocationOverrides,
    textures: crate::launcher_textures::LibraryWorker,
    auto_pending: Vec<String>,
    auto_pending_since: Instant,
    group_results: std::collections::HashMap<String, Vec<Value>>,
    launch_notices: std::collections::HashMap<String, String>,
    quit_all: bool,
    closing_since: std::collections::HashMap<String, Instant>,
    deleting: HashSet<String>,
}

impl State {
    fn busy(&self) -> Busy {
        Busy {
            base: self.base.clone(),
            queued: self
                .sessions
                .snapshots()
                .into_iter()
                .filter(|s| !matches!(s.state, SessionState::Failed { .. }))
                .map(|s| s.profile_id)
                .chain(self.deleting.iter().cloned())
                .collect(),
        }
    }
    fn account(&self, id: &str) -> Result<Account, String> {
        self.repository
            .list()?
            .into_iter()
            .find(|a| a.id == id)
            .ok_or_else(|| "This Account no longer exists.".into())
    }
    fn reconcile(&mut self) {
        if std::fs::remove_file(self.base.join("launcher/reopen.request")).is_ok() {
            self.window.makeKeyAndOrderFront(None);
            NSApplication::sharedApplication(MainThreadMarker::new().expect("main thread"))
                .activate();
        }
        if let Ok(accounts) = self.repository.list() {
            for account in accounts {
                let _ = self.sessions.reconnect(
                    &account.profile_id,
                    support(&self.base, &account.profile_id),
                );
            }
        }
        if !self.auto_pending.is_empty() {
            let pending = std::mem::take(&mut self.auto_pending);
            let ready = self.textures.ready()
                || self.auto_pending_since.elapsed() >= Duration::from_secs(5);
            let (ids, waiting): (Vec<_>, Vec<_>) = pending.into_iter().partition(|id| {
                ready
                    || self
                        .account(id)
                        .map_or(true, |a| a.launch_preferences.texture_pack_ids.is_empty())
            });
            self.auto_pending = waiting;
            let _ = self.launch_accounts(&ids);
        }
        self.sessions.tick();
        self.update_group_results();
        self.textures.release_finished(&self.sessions.snapshots());
    }
    fn snapshot(&mut self) -> Result<Value, String> {
        self.update_group_results();
        let snapshots = self.sessions.snapshots();
        let busy = self.busy();
        let accounts = self
            .repository
            .list()?
            .into_iter()
            .map(|a| {
                let session = snapshots.iter().find(|s| s.profile_id == a.profile_id);
                let held = busy.is_busy(&a.profile_id);
                let (status, label, show, close) = match session.map(|s| &s.state) {
                    Some(SessionState::Queued) => ("queued", "Queued".into(), false, false),
                    Some(SessionState::AwaitingWindow) => {
                        ("starting", "Preparing game…".into(), false, true)
                    }
                    Some(SessionState::Running { .. }) => ("running", "Running".into(), true, true),
                    Some(SessionState::Closing { .. }) => {
                        ("closing", "Waiting for game to close…".into(), true, false)
                    }
                    Some(SessionState::Failed { reason }) => {
                        ("failed", reason.clone(), false, false)
                    }
                    None if self.deleting.contains(&a.profile_id) => (
                        "unavailable",
                        "Deleting private files…".into(),
                        false,
                        false,
                    ),
                    None if held => (
                        "unavailable",
                        "Game is starting or running outside this launcher".into(),
                        false,
                        false,
                    ),
                    None => ("ready", "Ready".into(), false, false),
                };
                let force = status == "closing"
                    && self
                        .closing_since
                        .get(&a.profile_id)
                        .is_some_and(|t| t.elapsed() > Duration::from_secs(5));
                let mut value = serde_json::to_value(&a).expect("Account metadata serializes");
                value["windowPreferences"] = json!(crate::window::state::preferences(
                    &support(&self.base, &a.profile_id).join("window.json")
                ));
                value["lastSeenCharacters"] = json!(crate::character_bridge::load_observations(
                    &support(&self.base, &a.profile_id)
                ));
                value["status"] = json!(status);
                value["statusLabel"] = json!(label);
                value["launchWarning"] = json!(self.launch_notices.get(&a.profile_id));
                value["busy"] = json!(held);
                value["canShow"] = json!(show);
                value["canClose"] = json!(close);
                value["canForceQuit"] = json!(force);
                value
            })
            .collect::<Vec<_>>();
        let groups = self
            .repository
            .groups()?
            .into_iter()
            .map(|group| {
                let mut value = json!(group);
                if let Some(members) = self.group_results.get(&group.id) {
                    let results = members;
                    value["lastLaunch"] = json!(results);
                }
                value
            })
            .collect::<Vec<_>>();
        let update = self
            .update
            .lock()
            .map_err(|_| "Update status unavailable")?;
        Ok(
            json!({ "accounts": accounts, "groups": groups, "textureLibrary": self.textures.snapshot(), "retained": self.repository.retained()?, "updating": update.0, "updateMessage": update.1 }),
        )
    }
    fn update_group_results(&mut self) {
        let snapshots = self.sessions.snapshots();
        for members in self.group_results.values_mut() {
            for member in members.iter_mut().filter(|member| member["queued"] == true) {
                let current = snapshots
                    .iter()
                    .find(|s| Some(s.profile_id.as_str()) == member["profileId"].as_str());
                let (status, final_result) = match current.map(|s| &s.state) {
                    Some(SessionState::Queued) => ("Queued".to_owned(), false),
                    Some(SessionState::AwaitingWindow) => ("Starting".into(), false),
                    Some(SessionState::Running { .. } | SessionState::Closing { .. }) => {
                        ("Started".into(), true)
                    }
                    Some(SessionState::Failed { reason }) => (format!("Failed: {reason}"), true),
                    None => ("Cancelled before start".into(), true),
                };
                member["status"] = json!(status);
                if final_result {
                    member["queued"] = json!(false);
                }
            }
        }
    }
    fn launch_accounts(&mut self, ids: &[String]) -> Result<Value, String> {
        if self.quit_all {
            return Err("Games are closing. Finish Quit all first.".into());
        }
        self.update_group_results();
        let mut members = Vec::new();
        for id in ids {
            let account = match self.account(id) {
                Ok(account) => account,
                Err(error) => {
                    members.push(json!({"id":id,"name":"Missing Account","status":error}));
                    continue;
                }
            };
            if self.busy().is_busy(&account.profile_id) {
                members.push(json!({"id":id,"name":account.nickname,"status":"Skipped: already queued or running"}));
                continue;
            }
            let mut spec = match launch_spec(&self.base, &account, &self.invocation_overrides) {
                Ok(spec) => spec,
                Err(error) => {
                    members.push(json!({"id":id,"name":account.nickname,"status":format!("Failed: {error}")}));
                    continue;
                }
            };
            spec.env.insert(
                "GWNATIVE_ACCOUNT_AUTO_LOGIN".into(),
                (account.auto_login && account.has_password).to_string(),
            );
            let mut texture_warnings = self.textures.warnings(&spec.options.texture_pack_ids);
            let texture_warning = match self
                .textures
                .pin(&account.profile_id, &spec.options.texture_pack_ids)
            {
                Ok(Some(path)) => {
                    spec.env.insert(
                        "GWNATIVE_TEXTURE_MANIFEST".into(),
                        path.to_string_lossy().into_owned(),
                    );
                    None
                }
                Ok(None) => None,
                Err(error) => Some(error),
            };
            if let Some(warning) = &texture_warning {
                texture_warnings.push(warning.clone());
            }
            if texture_warnings.is_empty() {
                self.launch_notices.remove(&account.profile_id);
            } else {
                self.launch_notices
                    .insert(account.profile_id.clone(), texture_warnings.join(" · "));
            }
            // An inherited session manifest must never reach another Account.
            spec.env
                .entry("GWNATIVE_TEXTURE_MANIFEST".into())
                .or_default();
            let queued = self
                .sessions
                .request_spec(spec, support(&self.base, &account.profile_id));
            members.push(json!({"id":id,"name":account.nickname,"profileId":account.profile_id,"queued":queued,"status":if queued { texture_warning.map(|e| format!("Queued · texture packs bypassed: {e}")).unwrap_or_else(|| "Queued".into()) } else {"Skipped".into()}}));
        }
        Ok(json!({"members":members}))
    }
    fn start_updates(&self, manual: bool) -> Result<(), String> {
        if self.offline {
            return Ok(());
        }
        if !self.deleting.is_empty() {
            return Err("Wait for private-file deletion to finish before checking updates.".into());
        }
        let accounts = self.repository.list()?;
        let mut status = self
            .update
            .lock()
            .map_err(|_| "Update status unavailable")?;
        if status.0 {
            return Ok(());
        }
        *status = (true, "Checking game-client updates…".into());
        drop(status);
        let status = self.update.clone();
        let base = self.base.clone();
        let check_app = manual;
        std::thread::spawn(move || {
            let client = crate::patch::Client::from_env();
            let result = (|| -> Result<usize, String> {
                let cache = crate::cache::default_cache_dir();
                let lease = crate::cache::prepare(&cache).map_err(|e| e.to_string())?;
                crate::cache::finish_maintenance(
                    &lease,
                    &cache,
                    &client.cached_profile_chunk_names(&base),
                )
                .map_err(|e| e.to_string())?;
                let mut changed = 0;
                for account in accounts {
                    let dir = support(&base, &account.profile_id);
                    // Revalidation offers a pending manifest; it never replaces a running generation.
                    if client.revalidate(&dir).map_err(|e| e.to_string())? {
                        changed += 1;
                    }
                }
                Ok(changed)
            })();
            let mut message: String = match result {
                Ok(0) => "Client checks complete · Game content shared".into(),
                Ok(_) => "Client updates will be prepared on next launch".into(),
                Err(_) => "Update check unavailable · Installed clients remain available".into(),
            };
            let app_status = if !check_app {
                String::new()
            } else {
                match crate::release::check() {
                    crate::release::Notice::Available { latest, .. } => format!(
                        "Launcher {latest} available; installation is not enabled in this development build"
                    ),
                    crate::release::Notice::Current { .. } => "Launcher is current".into(),
                    crate::release::Notice::Unknown(_) => {
                        "Launcher update check unavailable".into()
                    }
                }
            };
            if !app_status.is_empty() {
                message.push_str(" · ");
                message.push_str(&app_status);
            }
            if let Ok(mut state) = status.lock() {
                *state = (false, message);
            }
        });
        Ok(())
    }
    fn profiles(&self) -> Result<Value, String> {
        let assigned = self
            .repository
            .list()?
            .into_iter()
            .map(|a| a.profile_id)
            .collect::<HashSet<_>>();
        let mut rows = Vec::new();
        for profile in profile::list(&self.base)? {
            if assigned.contains(&profile.id) {
                continue;
            }
            let saved = self.repository.credential_preview(&profile.id)?;
            rows.push(json!({"profileId":profile.id,"nickname":profile.display_name,"email":saved.username,"hasPassword":saved.has_password}));
        }
        Ok(json!(rows))
    }
    fn action(&mut self, value: &Value) -> Result<Value, String> {
        let action = text(value, "action")?;
        match action {
            "snapshot" => return self.snapshot(),
            "profiles" => return self.profiles(),
            "checkUpdates" => self.start_updates(true)?,
            "save" => {
                let busy = self.busy();
                let preferences = value
                    .get("launchPreferences")
                    .map(|p| {
                        serde_json::from_value::<launch_options::AccountLaunchPreferences>(
                            p.clone(),
                        )
                        .map_err(|e| e.to_string())
                    })
                    .transpose()?;
                if let Some(preferences) = &preferences {
                    launch_options::validate_account_launch_preferences(preferences)?;
                }
                let fixed = value
                    .get("fixedLaunchFrame")
                    .map(|frame| {
                        serde_json::from_value::<Option<crate::window::state::Bounds>>(
                            frame.clone(),
                        )
                        .map_err(|e| e.to_string())
                    })
                    .transpose()?;
                if let Some(Some(frame)) = fixed {
                    crate::window::state::validate_frame(frame)?;
                }
                let saved_account;
                if let Some(id) = value["accountId"].as_str() {
                    let current = self.account(id)?;
                    let email = text(value, "email")?;
                    let changed_email = !current.email.eq_ignore_ascii_case(email.trim());
                    let password = if changed_email {
                        PasswordChange::Keep
                    } else if flag(value, "removePassword") {
                        PasswordChange::Remove
                    } else if let Some(password) = value["password"].as_str() {
                        PasswordChange::Set(password.to_owned())
                    } else {
                        PasswordChange::Keep
                    };
                    saved_account = self.repository.save_form(
                        id,
                        AccountPatch {
                            nickname: Some(text(value, "nickname")?.into()),
                            email: Some(email.into()),
                            auto_login: Some(!changed_email && flag(value, "autoLogin")),
                            auto_launch: Some(flag(value, "autoLaunch")),
                            preserve_context: flag(value, "preserveContext"),
                            launch_preferences: preferences.clone(),
                        },
                        password,
                        &busy,
                    )?;
                } else {
                    let profile_id = value["profileId"].as_str().map(str::to_owned);
                    let adopted = profile_id
                        .as_ref()
                        .map(|id| KeychainCredentialStore.read(id))
                        .transpose()?
                        .flatten();
                    let email = text(value, "email")?.to_owned();
                    if adopted
                        .as_ref()
                        .is_some_and(|saved| !saved.username().eq_ignore_ascii_case(email.trim()))
                        && !flag(value, "preserveContext")
                    {
                        return Err("Confirm changing this profile’s login while keeping its private files.".into());
                    }
                    let password = value["password"].as_str().map(str::to_owned).or_else(|| {
                        adopted
                            .as_ref()
                            .filter(|c| c.username().eq_ignore_ascii_case(&email))
                            .map(|c| c.password().to_owned())
                    });
                    saved_account = self.repository.create_with_profile(
                        AccountDraft {
                            profile_id,
                            nickname: text(value, "nickname")?.into(),
                            email,
                            password,
                            auto_login: Some(flag(value, "autoLogin")),
                            auto_launch: flag(value, "autoLaunch"),
                        },
                        &busy,
                        |profile_id| profile::select(&self.base, Some(profile_id)).map(|_| ()),
                    )?;
                    if let Some(preferences) = preferences {
                        self.repository.update(
                            &saved_account.id,
                            AccountPatch {
                                launch_preferences: Some(preferences),
                                ..Default::default()
                            },
                            &busy,
                        )?;
                    }
                }
                save_fixed_launch_frame(
                    &support(&self.base, &saved_account.profile_id).join("window.json"),
                    fixed,
                )?;
            }
            "textureConflicts" => {
                let ids = serde_json::from_value::<Vec<String>>(value["packIds"].clone())
                    .map_err(|_| "Invalid pack selection")?;
                if ids.len() > 64 {
                    return Err("Too many texture packs selected".into());
                }
                return Ok(json!(self.textures.conflicts(&ids)));
            }
            "refreshTextures" => self.textures.refresh()?,
            "openTextureFolder" => {
                let folder = self.textures.folder()?;
                std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
                std::process::Command::new("/usr/bin/open")
                    .arg(folder)
                    .spawn()
                    .map_err(|e| e.to_string())?;
            }
            "saveGroup" => {
                let draft = LaunchGroupDraft {
                    name: text(value, "name")?.into(),
                    account_ids: serde_json::from_value(value["accountIds"].clone())
                        .map_err(|e| e.to_string())?,
                };
                if let Some(id) = value["groupId"].as_str() {
                    self.repository.update_group(id, draft)?;
                } else {
                    self.repository.create_group(draft)?;
                }
            }
            "deleteGroup" => {
                self.repository.delete_group(text(value, "groupId")?)?;
            }
            "launchGroup" => {
                let group = self
                    .repository
                    .groups()?
                    .into_iter()
                    .find(|g| Some(g.id.as_str()) == value["groupId"].as_str())
                    .ok_or("Group no longer exists")?;
                let result = self.launch_accounts(&group.account_ids)?;
                if let Some(members) = result["members"].as_array() {
                    self.group_results.insert(group.id, members.clone());
                }
                return Ok(result);
            }
            "toggle" => {
                let id = text(value, "accountId")?;
                let field = text(value, "field")?;
                let mut patch = AccountPatch::default();
                match field {
                    "autoLogin" => patch.auto_login = Some(flag(value, "value")),
                    "autoLaunch" => patch.auto_launch = Some(flag(value, "value")),
                    _ => return Err("Unknown Account setting".into()),
                }
                self.repository.update(id, patch, &self.busy())?;
            }
            "play" => {
                let ids = serde_json::from_value::<Vec<String>>(value["accountIds"].clone())
                    .map_err(|_| "Select Accounts to launch")?;
                return self.launch_accounts(&ids);
            }

            "show" | "close" | "cancel" | "forceQuit" => {
                let account = self.account(text(value, "accountId")?)?;
                match action {
                    "show" => {
                        self.sessions.show(&account.profile_id)?;
                    }
                    "close" => {
                        self.sessions.close(&account.profile_id)?;
                        self.closing_since
                            .insert(account.profile_id, Instant::now());
                    }
                    "forceQuit" => {
                        self.sessions.force_quit_explicit(&account.profile_id)?;
                    }
                    _ => {
                        self.sessions.cancel_queued(&account.profile_id);
                    }
                }
            }
            "remove" => {
                if flag(value, "deleteFiles") {
                    return Err("Private-file deletion must finish through the native data-store removal flow.".into());
                }
                self.repository
                    .remove(text(value, "accountId")?, &self.busy())?;
            }
            "quitAll" => {
                self.sessions.cancel_pending();
                self.quit_all = true;
                for session in self.sessions.snapshots() {
                    self.sessions.close(&session.profile_id)?;
                    self.closing_since
                        .insert(session.profile_id, Instant::now());
                }
            }
            _ => return Err("Unknown launcher action".into()),
        }
        Ok(json!({"ok":true}))
    }
}

fn invocation_overrides(invocation: &cli::Invocation) -> InvocationOverrides {
    InvocationOverrides {
        muted: invocation.legacy.mute.then_some(true),
        frame_rate_limit: invocation.legacy.fps.map(FrameRateLimit::Limit),
        window_mode: invocation.legacy.window_mode.map(|mode| match mode {
            cli::WindowMode::Windowed => WindowMode::Windowed,
            cli::WindowMode::Fullscreen => WindowMode::Fullscreen,
        }),
        preferred_character: invocation.legacy.character.clone().map(Some),
        ..Default::default()
    }
}

/// `None` means an older caller omitted this preference. `Some(None)` is the
/// explicit Restore-last-layout choice and must clear an existing fixed frame.
fn save_fixed_launch_frame(
    path: &Path,
    frame: Option<Option<crate::window::state::Bounds>>,
) -> Result<(), String> {
    if let Some(frame) = frame {
        crate::window::state::set_fixed_frame(path, frame)?;
    }
    Ok(())
}

fn launch_spec(
    base: &Path,
    account: &Account,
    overrides: &InvocationOverrides,
) -> Result<LaunchSpec, String> {
    let stored = crate::window::state::launch_snapshot(
        &support(base, &account.profile_id).join("window.json"),
    );
    let defaults = LaunchDefaults {
        window_frame: stored.map(|s| LaunchFrame {
            x: s.bounds.x,
            y: s.bounds.y,
            width: s.bounds.width,
            height: s.bounds.height,
        }),
        window_mode: if stored.is_some_and(|s| s.mode == crate::window::state::Mode::Fullscreen) {
            WindowMode::Fullscreen
        } else {
            WindowMode::Windowed
        },
        ..Default::default()
    };
    let mut spec = LaunchSpec::game(
        &std::env::current_exe().map_err(|e| e.to_string())?,
        &account.profile_id,
    );
    spec.options =
        launch_options::resolve_launch_options(&defaults, &account.launch_preferences, overrides);
    if overrides.window_mode.is_none()
        && account.launch_preferences.window_mode.is_none()
        && stored.is_some_and(|s| s.mode == crate::window::state::Mode::Maximized)
    {
        spec.env
            .insert("GWNATIVE_RESTORE_MAXIMIZED".into(), "1".into());
    } else {
        spec.env
            .insert("GWNATIVE_RESTORE_MAXIMIZED".into(), "0".into());
    }
    spec.env.insert(
        "GWNATIVE_ACCOUNT_AUTO_LOGIN".into(),
        (account.auto_login && account.has_password).to_string(),
    );
    Ok(spec)
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key].as_str().ok_or_else(|| format!("Missing {key}"))
}
fn flag(value: &Value, key: &str) -> bool {
    value[key].as_bool().unwrap_or(false)
}
fn wipe_value(value: &mut Value) {
    match value {
        Value::String(s) => crate::log::wipe_string(s),
        Value::Array(a) => a.iter_mut().for_each(wipe_value),
        Value::Object(o) => o.values_mut().for_each(wipe_value),
        _ => {}
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct LauncherDelegate;
    unsafe impl NSObjectProtocol for LauncherDelegate {}
    unsafe impl WKScriptMessageHandler for LauncherDelegate {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn received(&self, _controller: &WKUserContentController, message: &WKScriptMessage) {
            if !unsafe { message.frameInfo().isMainFrame() } {
                return;
            }
            let body = unsafe { message.body() };
            let Some(body) = body.downcast_ref::<NSString>() else {
                return;
            };
            let mut raw = body.to_string();
            if raw.len() > 16_384 {
                crate::log::wipe_string(&mut raw);
                return;
            }
            let parsed = serde_json::from_str::<Value>(&raw);
            crate::log::wipe_string(&mut raw);
            let Ok(mut request) = parsed else {
                return;
            };
            let id = request["id"].as_u64().unwrap_or(0);
            if request["action"] == "changeTextureFolder" {
                choose_texture_folder(id);
                wipe_value(&mut request);
                return;
            }
            if request["action"] == "captureLayout" {
                begin_capture(&request, id);
                wipe_value(&mut request);
                return;
            }
            if request["action"] == "remove" && flag(&request, "deleteFiles") {
                begin_removal(&request, id);
                wipe_value(&mut request);
                return;
            }
            STATE.with(|slot| {
                let mut slot = slot.borrow_mut();
                let Some(state) = slot.as_mut() else {
                    return;
                };
                let reply = match state.action(&request) {
                    Ok(result) => json!({"id":id,"result":result}),
                    Err(error) => json!({"id":id,"error":error}),
                };
                let script = NSString::from_str(&format!("window.launcherReply({reply})"));
                unsafe {
                    state
                        .webview
                        .evaluateJavaScript_completionHandler(&script, None);
                }
            });
            wipe_value(&mut request);
        }
    }
    unsafe impl WKNavigationDelegate for LauncherDelegate {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn navigate(
            &self,
            _view: &WKWebView,
            action: &WKNavigationAction,
            handler: &DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            let url = unsafe { action.request().URL() }
                .and_then(|u| u.absoluteString())
                .map(|u| u.to_string());
            handler.call((if url.as_deref() == Some("about:blank") {
                WKNavigationActionPolicy::Allow
            } else {
                WKNavigationActionPolicy::Cancel
            },));
        }
    }
    unsafe impl NSApplicationDelegate for LauncherDelegate {
        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn close(&self, _app: &NSApplication) -> bool {
            false
        }
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn reopen(&self, _app: &NSApplication, _visible: bool) -> bool {
            STATE.with(|slot| {
                if let Some(state) = slot.borrow().as_ref() {
                    state.window.makeKeyAndOrderFront(None);
                }
            });
            true
        }
        #[unsafe(method(applicationShouldTerminate:))]
        fn quit(&self, _app: &NSApplication) -> NSApplicationTerminateReply {
            let deleting = STATE.with(|slot| {
                if let Some(state) = slot.borrow_mut().as_mut() {
                    state.sessions.cancel_pending();
                    !state.deleting.is_empty()
                } else {
                    false
                }
            });
            if deleting {
                NSApplicationTerminateReply::TerminateCancel
            } else {
                NSApplicationTerminateReply::TerminateNow
            }
        }
    }
);

pub fn run(invocation: &cli::Invocation) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("Launcher requires the main thread")?;
    let base = paths::base_support_dir();
    let lock_path = base.join("launcher/launcher.lock");
    let lock = match instance::acquire(&lock_path, Duration::ZERO) {
        Ok(lock) => lock,
        Err(error) => {
            if let Some(pid) = instance::holder(&lock_path).and_then(|pid| {
                objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
            }) {
                std::fs::write(base.join("launcher/reopen.request"), b"reopen")
                    .map_err(|e| e.to_string())?;
                #[allow(deprecated)]
                pid.activateWithOptions(
                    objc2_app_kit::NSApplicationActivationOptions::ActivateAllWindows,
                );
                return Ok(());
            }
            return Err(error);
        }
    };
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    dock::set_icon(mtm);
    let delegate = LauncherDelegate::alloc(mtm).set_ivars(());
    let delegate: Retained<LauncherDelegate> = unsafe { msg_send![super(delegate), init] };
    let frame = NSRect::new(NSPoint::new(0., 0.), NSSize::new(920., 640.));
    let config = unsafe { WKWebViewConfiguration::new(mtm) };
    unsafe {
        config.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(mtm));
    }
    let content = unsafe { config.userContentController() };
    unsafe {
        content.addScriptMessageHandler_name(
            ProtocolObject::from_ref(&*delegate),
            &NSString::from_str("launcher"),
        );
    }
    let view =
        unsafe { WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &config) };
    unsafe {
        view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    }
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable;
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    window.setTitle(&NSString::from_str("Guild Wars · Accounts"));
    window.setContentView(Some(&view));
    window.setContentMinSize(NSSize::new(600., 440.));
    unsafe {
        window.setReleasedWhenClosed(false);
    }
    window.center();
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.setMainMenu(Some(&menu(mtm)));
    let repository = Arc::new(AccountRepository::open(&base));
    let accounts = repository.list()?;
    let mut sessions = SessionController::new(
        std::env::current_exe().map_err(|e| e.to_string())?,
        CurrentExecutable::with_game_options(
            invocation.offline,
            invocation.no_update,
            invocation.legacy.mute,
        ),
        IpcProbe,
    );
    for account in &accounts {
        let _ = sessions.reconnect(&account.profile_id, support(&base, &account.profile_id));
    }
    let settings = Arc::new(crate::settings::Store::open(base.join("settings.json")));
    let updates = Arc::new(crate::settings::UpdateStore::open(
        base.join("updates.json"),
        &settings.get(),
    ));
    let settings = Arc::new(crate::settings::ScopedStore::new(settings, updates));
    let textures = crate::launcher_textures::LibraryWorker::start(&base);
    let auto_pending = accounts
        .iter()
        .filter(|account| account.auto_launch)
        .map(|a| a.id.clone())
        .collect();
    let state = State {
        base,
        textures,
        auto_pending,
        auto_pending_since: Instant::now(),
        group_results: Default::default(),
        launch_notices: Default::default(),
        repository,
        sessions,
        window: window.clone(),
        webview: view.clone(),
        _lock: lock,
        update: Arc::new(Mutex::new((
            false,
            "Game content shared across accounts".into(),
        ))),
        offline: !invocation.automatic_updates_allowed(),
        invocation_overrides: invocation_overrides(invocation),
        quit_all: false,
        closing_since: Default::default(),
        deleting: HashSet::new(),
    };
    state.start_updates(false)?;
    if invocation.automatic_updates_allowed() {
        crate::menu::check_for_updates_at_launch(&settings);
    }
    STATE.with(|slot| *slot.borrow_mut() = Some(state));
    let html = include_str!("../ui/launcher.html")
        .replace("/* LAUNCHER_STYLE */", include_str!("../ui/launcher.css"))
        .replace("/* LAUNCHER_SCRIPT */", include_str!("../ui/launcher.js"));
    unsafe {
        view.loadHTMLString_baseURL(&NSString::from_str(&html), None);
    }
    window.makeKeyAndOrderFront(None);
    app.activate();
    app::about_to_run();
    schedule_tick();
    app.run();
    STATE.with(|slot| slot.borrow_mut().take());
    drop(delegate);
    Ok(())
}

fn schedule_tick() {
    app::after(300, || {
        let quit = STATE.with(|slot| {
            let mut slot = slot.borrow_mut();
            let Some(state) = slot.as_mut() else {
                return false;
            };
            state.reconcile();
            state.quit_all
                && state
                    .repository
                    .list()
                    .is_ok_and(|a| a.iter().all(|a| !state.busy().is_busy(&a.profile_id)))
        });
        if quit {
            app::request_quit();
        } else {
            schedule_tick();
        }
    });
}

fn menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let bar = NSMenu::new(mtm);
    for (name, entries) in [
        (
            "Guild Wars",
            vec![
                ("Hide Launcher", sel!(hide:), "h"),
                ("Quit Launcher", sel!(terminate:), "q"),
            ],
        ),
        (
            "Edit",
            vec![
                ("Undo", sel!(undo:), "z"),
                ("Cut", sel!(cut:), "x"),
                ("Copy", sel!(copy:), "c"),
                ("Paste", sel!(paste:), "v"),
                ("Select All", sel!(selectAll:), "a"),
            ],
        ),
    ] {
        let sub = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(name));
        for (title, action, key) in entries {
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(title),
                    Some(action),
                    &NSString::from_str(key),
                )
            };
            sub.addItem(&item);
        }
        let holder = NSMenuItem::new(mtm);
        holder.setSubmenu(Some(&sub));
        bar.addItem(&holder);
    }
    bar
}

/// Keep authenticated control alive while game preparation/main loop runs.
pub fn start_game_control(host: GameHost) -> Result<Arc<Mutex<GameHost>>, String> {
    let host = Arc::new(Mutex::new(host));
    crate::launcher_sessions::install_relaunch_host(host.clone())?;
    let worker = host.clone();
    std::thread::spawn(move || {
        loop {
            let commands = worker
                .lock()
                .ok()
                .and_then(|h| h.poll().ok())
                .unwrap_or_default();
            for command in commands {
                match command {
                    HostCommand::CaptureWindowLayout { request_id } => unsafe {
                        app::to_main(
                            Box::into_raw(Box::new(request_id)).cast(),
                            capture_game_layout,
                        )
                    },
                    HostCommand::Close => app::request_quit(),
                    HostCommand::Show => unsafe { app::to_main(std::ptr::null_mut(), show_game) },
                }
            }
            std::thread::sleep(Duration::from_millis(40));
        }
    });
    Ok(host)
}
extern "C" fn capture_game_layout(data: *mut std::ffi::c_void) {
    let request_id = unsafe { Box::from_raw(data.cast::<String>()) };
    crate::window::capture_current_layout(&request_id);
}
extern "C" fn show_game(_: *mut std::ffi::c_void) {
    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    let app = NSApplication::sharedApplication(mtm);
    app.activate();
    for window in app.windows() {
        window.makeKeyAndOrderFront(None);
    }
}

/// Refresh display metadata without changing the running session's login.
pub fn track_game_title(window: Retained<NSWindow>, profile_id: String, nickname: String) {
    app::after(1_000, move || {
        let next = game_account(&profile_id)
            .ok()
            .flatten()
            .map(|a| a.nickname)
            .unwrap_or_else(|| nickname.clone());
        if next != nickname {
            window.setTitle(&NSString::from_str(&format!("{next} · Guild Wars")));
        }
        track_game_title(window.clone(), profile_id.clone(), next);
    });
}

fn reply(id: u64, result: Result<(), String>) {
    let payload = match result {
        Ok(()) => json!({"id":id,"result":{"ok":true}}),
        Err(error) => json!({"id":id,"error":error}),
    };
    STATE.with(|slot| {
        if let Some(state) = slot.borrow().as_ref() {
            let script = NSString::from_str(&format!("window.launcherReply({payload})"));
            unsafe {
                state
                    .webview
                    .evaluateJavaScript_completionHandler(&script, None);
            }
        }
    });
}

fn choose_texture_folder(reply_id: u64) {
    // Run native modal outside STATE's RefCell borrow: AppKit pumps launcher
    // timers while the picker is open, and those timers also reconcile STATE.
    let panel =
        objc2_app_kit::NSOpenPanel::openPanel(MainThreadMarker::new().expect("main thread"));
    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(false);
    panel.setAllowsMultipleSelection(false);
    let result = if panel.runModal() == objc2_app_kit::NSModalResponseOK {
        match panel
            .URL()
            .and_then(|u| u.path())
            .map(|s| PathBuf::from(s.to_string()))
        {
            Some(path) => STATE.with(|slot| {
                slot.borrow()
                    .as_ref()
                    .ok_or_else(|| "Launcher is closing".to_owned())?
                    .textures
                    .change_folder(path)
            }),
            None => Err("No texture folder selected".into()),
        }
    } else {
        Ok(())
    };
    reply(reply_id, result);
}

fn begin_capture(request: &Value, reply_id: u64) {
    let result = STATE.with(|slot| -> Result<_, String> {
        let slot = slot.borrow();
        let state = slot.as_ref().ok_or("Launcher is closing")?;
        let account = state.account(text(request, "accountId")?)?;
        let request_id = format!(
            "{:032x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        );
        if !state
            .sessions
            .capture_window_layout(&account.profile_id, &request_id)?
        {
            return Err("Game window is not available".into());
        }
        Ok((
            support(&state.base, &account.profile_id).join("window-capture.json"),
            request_id,
        ))
    });
    match result {
        Ok((path, request_id)) => poll_capture(path, request_id, reply_id, Instant::now()),
        Err(error) => reply(reply_id, Err(error)),
    }
}

fn poll_capture(path: PathBuf, request_id: String, reply_id: u64, start: Instant) {
    app::after(40, move || {
        let value = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        if let Some(value) = value.filter(|v| v["requestId"] == request_id) {
            STATE.with(|slot| {
                if let Some(state) = slot.borrow().as_ref() {
                    let payload = json!({"id":reply_id,"result":{"frame":value["frame"]}});
                    unsafe {
                        state.webview.evaluateJavaScript_completionHandler(
                            &NSString::from_str(&format!("window.launcherReply({payload})")),
                            None,
                        );
                    }
                }
            });
        } else if start.elapsed() < Duration::from_secs(3) {
            poll_capture(path.clone(), request_id.clone(), reply_id, start);
        } else {
            reply(reply_id, Err("Game did not return its current window layout. Try again when its window is ready.".into()));
        }
    });
}

fn begin_removal(request: &Value, id: u64) {
    let prepared = STATE.with(|slot| -> Result<_, String> {
        let mut slot = slot.borrow_mut();
        let state = slot.as_mut().ok_or("Launcher is closing")?;
        if state
            .update
            .lock()
            .map_err(|_| "Update status unavailable")?
            .0
        {
            return Err(
                "Wait for the client update check to finish before deleting private files.".into(),
            );
        }
        let account_id = text(request, "accountId")?.to_owned();
        let profile_id = match state
            .repository
            .list()?
            .into_iter()
            .find(|a| a.id == account_id)
        {
            Some(account) => account.profile_id,
            None => {
                state
                    .repository
                    .retained()?
                    .into_iter()
                    .find(|a| a.id == account_id)
                    .ok_or("This Account no longer exists.")?
                    .profile_id
            }
        };
        let guard = state.busy().acquire_exclusive(&profile_id)?;
        state.deleting.insert(profile_id.clone());
        Ok((
            state.repository.clone(),
            state.base.clone(),
            account_id,
            profile_id,
            guard,
        ))
    });
    let (repository, base, account_id, profile_id, guard) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            reply(id, Err(error));
            return;
        }
    };
    let remover = crate::launcher_removal::NativeWebsiteDataRemover::new(
        MainThreadMarker::new().expect("main thread"),
    );
    crate::launcher_removal::remove_account(
        repository,
        &account_id,
        &base,
        guard,
        &remover,
        move |result| {
            STATE.with(|slot| {
                if let Some(state) = slot.borrow_mut().as_mut() {
                    state.deleting.remove(&profile_id);
                }
            });
            reply(id, result);
        },
    );
}

#[cfg(test)]
mod phase_two_tests {
    use super::*;

    #[test]
    fn launch_descriptor_freezes_layout_and_explicit_mode_wins() {
        let scratch = crate::scratch::TempDir::new("launcher-descriptor");
        let account = Account {
            format_version: 1,
            id: "account-test".into(),
            profile_id: "profile-test".into(),
            nickname: "Test".into(),
            email: "test@example.invalid".into(),
            auto_login: false,
            auto_launch: false,
            has_password: false,
            launch_preferences: Default::default(),
        };
        let path = support(&scratch.0, &account.profile_id).join("window.json");
        let observed = crate::window::state::State {
            bounds: crate::window::state::Bounds {
                x: 12.0,
                y: 34.0,
                width: 1000.0,
                height: 700.0,
            },
            mode: crate::window::state::Mode::Maximized,
        };
        crate::window::state::save(&path, observed);
        let fixed = crate::window::state::Bounds {
            x: 50.0,
            y: 70.0,
            width: 1200.0,
            height: 800.0,
        };
        crate::window::state::set_fixed_frame(&path, Some(fixed)).unwrap();
        let spec = launch_spec(&scratch.0, &account, &InvocationOverrides::default()).unwrap();
        assert_eq!(spec.options.window_frame.unwrap().x, 50.0);
        assert_eq!(spec.env["GWNATIVE_RESTORE_MAXIMIZED"], "1");
        crate::window::state::set_fixed_frame(&path, None).unwrap();
        assert_eq!(
            spec.options.window_frame.unwrap().x,
            50.0,
            "queued descriptor must not follow edits"
        );
        let explicit = launch_spec(
            &scratch.0,
            &account,
            &InvocationOverrides {
                window_mode: Some(WindowMode::Fullscreen),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(explicit.options.window_mode, WindowMode::Fullscreen);
        assert_eq!(explicit.env["GWNATIVE_RESTORE_MAXIMIZED"], "0");
        assert_eq!(explicit.options.window_frame.unwrap().x, 12.0);
    }

    #[test]
    fn restore_layout_clears_an_existing_fixed_launch_frame() {
        let scratch = crate::scratch::TempDir::new("launcher-clear-fixed-frame");
        let path = scratch.0.join("window.json");
        let fixed = crate::window::state::Bounds {
            x: 50.0,
            y: 70.0,
            width: 1200.0,
            height: 800.0,
        };
        crate::window::state::set_fixed_frame(&path, Some(fixed)).unwrap();

        save_fixed_launch_frame(&path, Some(None)).unwrap();

        assert_eq!(
            crate::window::state::preferences(&path).fixed_launch_frame,
            None
        );
    }
}
