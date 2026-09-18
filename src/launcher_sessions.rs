//! Launcher-owned game-session coordination.
//!
//! Session record is a capability, not a pid file.  Launcher writes a 0600
//! record before spawning.  Game, after it holds its profile lock, consumes
//! record, owns 0600 Unix socket, and proves `{profile, nonce, pid, ready}` to
//! launcher.  A reconnect therefore trusts a live same-user socket plus nonce;
//! it never treats a saved PID as evidence that a game still owns a profile.
//!
//! Root integration:
//! - create `SessionController::new(base, current_exe)` in launcher process;
//! - call `request(profile)`, then `tick()` from launcher event loop;
//! - child starts `GameHost::start(profile_support, profile_id, profile_lock)`;
//! - after game window exists call `host.ready()`; poll `host.poll()` on main
//!   thread and dispatch `Show` / `Close` there.
//!
//! No credential, account email, or password enters this module or child args.

use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::instance;

const RECORD: &str = "launcher-session.json";
const VERSION: u32 = 1;
const IPC_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_REQUEST_BYTES: u64 = 512;

#[derive(Clone, Debug, PartialEq)]
pub struct LaunchSpec {
    pub profile_id: String,
    pub args: Vec<String>,
    /// Frozen, nonsecret child environment. Values are set explicitly rather
    /// than inherited from mutable launcher state.
    pub env: BTreeMap<String, String>,
    pub options: crate::launcher_preferences::ResolvedLaunchOptions,
}

impl LaunchSpec {
    pub fn game(executable: &Path, profile_id: &str) -> Self {
        Self {
            profile_id: profile_id.into(),
            args: vec![
                executable.display().to_string(),
                "--game".into(),
                "--profile".into(),
                profile_id.into(),
                "--new-instance".into(),
            ],
            env: BTreeMap::new(),
            options: crate::launcher_preferences::ResolvedLaunchOptions {
                muted: false,
                frame_rate_limit: crate::launcher_preferences::FrameRateLimit::Default,
                window_mode: crate::launcher_preferences::WindowMode::Windowed,
                window_frame: None,
                preferred_character: None,
                texture_pack_ids: Vec::new(),
            },
        }
    }
}

pub trait GameRunner {
    /// Spawn detached game process.  Return its OS PID only as a handshake
    /// expectation; never use it as liveness proof after this call.
    fn spawn(&mut self, spec: &LaunchSpec) -> Result<u32, String>;
    /// Birth identity for a child this runner has just spawned.  Production
    /// reads kernel data; fakes provide deterministic identities without
    /// pretending their synthetic PIDs exist in macOS.
    fn process_identity(&self, pid: u32) -> Option<String>;
    /// Best-effort containment for a child which could not be recorded.  This
    /// must reap an owned child before returning.
    fn terminate_reap(&mut self, pid: u32) -> Result<(), String>;
    /// `Some(true)` proves this launcher-owned child exited. `None` means this
    /// launcher never spawned it, as after reconnect.
    fn exited(&mut self, pid: u32) -> Result<Option<bool>, String>;
}

#[derive(Default)]
pub struct CurrentExecutable {
    children: BTreeMap<u32, Child>,
    game_options: Vec<String>,
}

impl CurrentExecutable {
    /// Preserve launcher update policy without inheriting arbitrary environment.
    pub fn with_game_options(offline: bool, no_update: bool, _mute: bool) -> Self {
        let mut game_options = Vec::new();
        if offline {
            game_options.push("--offline".into());
        }
        if no_update {
            game_options.push("--no-update".into());
        }
        Self {
            children: BTreeMap::new(),
            game_options,
        }
    }
}

fn supported_option_args(
    options: &crate::launcher_preferences::ResolvedLaunchOptions,
) -> Vec<String> {
    use crate::launcher_preferences::{FrameRateLimit, WindowMode};
    let mut args = Vec::new();
    if options.muted {
        args.push("-nosound".into());
    }
    if let FrameRateLimit::Limit(limit) = options.frame_rate_limit {
        args.push("-fps".into());
        args.push(limit.to_string());
    }
    match options.window_mode {
        WindowMode::Windowed => args.push("-windowed".into()),
        WindowMode::Fullscreen => args.push("-windowedfullscreen".into()),
    }
    args
}

impl GameRunner for CurrentExecutable {
    fn spawn(&mut self, spec: &LaunchSpec) -> Result<u32, String> {
        let executable = spec
            .args
            .first()
            .ok_or_else(|| "game launch omitted executable".to_owned())?;
        let mut command = Command::new(executable);
        // Clear inherited launch-scoped values before installing this child's
        // frozen map. A caller may deliberately supply WINDOW_SNAPSHOT.
        command
            .env_remove("GWNATIVE_PORT")
            .env_remove("GWNATIVE_WEB_ROOT")
            .env_remove("GWNATIVE_BENCHMARK_EPHEMERAL_WEBKIT")
            .env_remove("GWNATIVE_CONTROL_FD")
            .env_remove("GWNATIVE_ACCOUNT_AUTO_LOGIN")
            .env_remove("GWNATIVE_TEXTURE_MANIFEST")
            .env_remove("GWNATIVE_RESTORE_MAXIMIZED")
            .env_remove("GWNATIVE_LAUNCH_OPTIONS")
            .env_remove("GWNATIVE_WINDOW_SNAPSHOT");
        let child = command
            .args(&spec.args[1..])
            .args(&self.game_options)
            .args(supported_option_args(&spec.options))
            .envs(&spec.env)
            .env(
                "GWNATIVE_LAUNCH_OPTIONS",
                serde_json::to_string(&spec.options)
                    .map_err(|error| format!("could not serialize launch options: {error}"))?,
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // Parent holds profile lock until nonce and child identity record
            // are atomically published. Child waits for this short reservation.
            .env("GWNATIVE_LAUNCHER_STARTING", "1")
            .spawn()
            .map_err(|error| format!("could not start game for {}: {error}", spec.profile_id))?;
        let pid = child.id();
        self.children.insert(pid, child);
        Ok(pid)
    }

    fn exited(&mut self, pid: u32) -> Result<Option<bool>, String> {
        let Some(child) = self.children.get_mut(&pid) else {
            return Ok(None);
        };
        let exited = child
            .try_wait()
            .map_err(|error| format!("could not observe game process {pid}: {error}"))?
            .is_some();
        if exited {
            self.children.remove(&pid);
        }
        Ok(Some(exited))
    }

    fn process_identity(&self, pid: u32) -> Option<String> {
        process_identity(pid)
    }

    fn terminate_reap(&mut self, pid: u32) -> Result<(), String> {
        let Some(child) = self.children.get_mut(&pid) else {
            return Err(format!("launcher no longer owns spawned game {pid}"));
        };
        if child
            .try_wait()
            .map_err(|error| format!("could not observe unrecorded game {pid}: {error}"))?
            .is_none()
        {
            child
                .kill()
                .map_err(|error| format!("could not terminate unrecorded game {pid}: {error}"))?;
        }
        child
            .wait()
            .map_err(|error| format!("could not reap unrecorded game {pid}: {error}"))?;
        self.children.remove(&pid);
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveSession {
    pub profile_id: String,
    pub pid: u32,
    pub ready: bool,
    /// PID birth identity captured by game and independently checked by launcher.
    pub process_identity: String,
}

pub trait SessionProbe {
    /// `None` means no authenticated game host replied.  Implementations must
    /// authenticate profile and nonce; a session-record PID alone is invalid.
    fn inspect(
        &self,
        support_dir: &Path,
        expected_nonce: &str,
    ) -> Result<Option<LiveSession>, String>;
    fn command(
        &self,
        support_dir: &Path,
        expected_nonce: &str,
        command: HostCommand,
    ) -> Result<(), String>;
    /// Kernel lock state supplements reaped-child evidence; PID records do not.
    fn profile_locked(&self, support_dir: &Path) -> Result<bool, String>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostCommand {
    Show,
    Close,
    CaptureWindowLayout { request_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionState {
    Queued,
    AwaitingWindow,
    Running { pid: u32 },
    Closing { pid: u32 },
    Failed { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub profile_id: String,
    pub state: SessionState,
}

#[derive(Debug)]
struct Managed {
    support_dir: PathBuf,
    nonce: String,
    started: Option<Instant>,
    pid: Option<u32>,
    process_identity: Option<String>,
    close_requested: bool,
    close_target_identity: Option<String>,
    state: SessionState,
    launch_spec: Option<LaunchSpec>,
}

/// One queue for one launcher.  Profile map enforces one game lifecycle per
/// profile, including repeated clicks and reconnect after launcher restart.
pub struct SessionController<R, P> {
    #[cfg(test)]
    executable: PathBuf,
    runner: R,
    probe: P,
    sessions: BTreeMap<String, Managed>,
    queue: VecDeque<String>,
}

impl<R: GameRunner, P: SessionProbe> SessionController<R, P> {
    pub fn new(executable: PathBuf, runner: R, probe: P) -> Self {
        #[cfg(not(test))]
        let _ = executable;
        Self {
            #[cfg(test)]
            executable,
            runner,
            probe,
            sessions: BTreeMap::new(),
            queue: VecDeque::new(),
        }
    }

    /// Request a game only when profile has no live, starting, or queued game.
    #[cfg(test)]
    pub fn request(&mut self, profile_id: &str, support_dir: PathBuf) -> bool {
        self.request_spec(LaunchSpec::game(&self.executable, profile_id), support_dir)
    }

    /// Queue an immutable per-child snapshot. Later Account/preference edits
    /// cannot alter a request already admitted to this controller.
    pub fn request_spec(&mut self, spec: LaunchSpec, support_dir: PathBuf) -> bool {
        let profile_id = spec.profile_id.clone();
        if !valid_profile_id(&profile_id) {
            return false;
        }
        if let Some(existing) = self.sessions.get(&profile_id) {
            if existing.pid.is_some() || !matches!(existing.state, SessionState::Failed { .. }) {
                return false;
            }
            self.sessions.remove(&profile_id);
        }
        let nonce = nonce();
        self.sessions.insert(
            profile_id.clone(),
            Managed {
                support_dir,
                nonce,
                started: None,
                pid: None,
                process_identity: None,
                close_requested: false,
                close_target_identity: None,
                state: SessionState::Queued,
                launch_spec: Some(spec),
            },
        );
        self.queue.push_back(profile_id);
        true
    }

    /// Cancel has effect only before process spawn.  Child games are never
    /// killed by queue cancellation or launcher quit.
    pub fn cancel_queued(&mut self, profile_id: &str) -> bool {
        let Some(session) = self.sessions.get(profile_id) else {
            return false;
        };
        if session.state != SessionState::Queued {
            return false;
        }
        self.sessions.remove(profile_id);
        self.queue.retain(|queued| queued != profile_id);
        true
    }

    /// Launcher quit calls this; it leaves awaiting/running games untouched.
    pub fn cancel_pending(&mut self) {
        let queued = std::mem::take(&mut self.queue);
        for profile_id in queued {
            if self
                .sessions
                .get(&profile_id)
                .is_some_and(|session| session.state == SessionState::Queued)
            {
                self.sessions.remove(&profile_id);
            }
        }
    }

    /// Reconnect to independently running games.  Record is only a route to
    /// socket; successful nonce-authenticated reply is required for adoption.
    pub fn reconnect(&mut self, profile_id: &str, support_dir: PathBuf) -> Result<bool, String> {
        if self.sessions.contains_key(profile_id) || !valid_profile_id(profile_id) {
            return Ok(false);
        }
        let Some(record) = read_record(&support_dir)? else {
            return Ok(false);
        };
        if record.profile_id != profile_id {
            return Ok(false);
        }
        let live = self.probe.inspect(&support_dir, &record.nonce)?;
        // A relaunch successor is committed before predecessor drops lock. If
        // predecessor has exited but committed successor still exists before
        // `GameHost::start`, reserve successor identity rather than replacing
        // nonce record with a second launch.
        let reserved = [
            (record.pid, record.process_identity.as_str()),
            (
                record.handoff_pid.unwrap_or_default(),
                record.handoff_identity.as_deref().unwrap_or_default(),
            ),
        ]
        .into_iter()
        .find(|(pid, identity)| {
            *pid != 0
                && !identity.is_empty()
                && process_identity(*pid).as_deref() == Some(*identity)
        });
        if live.is_none()
            && let Some((pid, identity)) = reserved
        {
            self.sessions.insert(
                profile_id.into(),
                Managed {
                    support_dir,
                    nonce: record.nonce,
                    started: None,
                    pid: Some(pid),
                    process_identity: Some(identity.into()),
                    close_requested: false,
                    close_target_identity: None,
                    state: SessionState::AwaitingWindow,
                    launch_spec: None,
                },
            );
            return Ok(true);
        }
        let Some(live) = live else {
            return Ok(false);
        };
        if live.profile_id != profile_id {
            return Ok(false);
        }
        self.sessions.insert(
            profile_id.into(),
            Managed {
                support_dir,
                nonce: record.nonce,
                started: None,
                pid: Some(live.pid),
                process_identity: Some(live.process_identity),
                close_requested: false,
                close_target_identity: None,
                state: if live.ready {
                    SessionState::Running { pid: live.pid }
                } else {
                    SessionState::AwaitingWindow
                },
                launch_spec: None,
            },
        );
        Ok(true)
    }

    /// Advance reconciliation then start next request only after earlier game
    /// has proved its window ready.  Failure cannot strand later profiles.
    pub fn tick(&mut self) {
        self.reconcile();
        if self
            .sessions
            .values()
            .any(|session| matches!(session.state, SessionState::AwaitingWindow))
        {
            return;
        }
        let Some(profile_id) = self.queue.pop_front() else {
            return;
        };
        let Some(session) = self.sessions.get_mut(&profile_id) else {
            return;
        };
        if session.state != SessionState::Queued {
            return;
        }
        // Reservation is profile's real process lock.  Acquiring before record
        // publication prevents a direct game from being overwritten by a new
        // nonce between queue selection and child startup.
        let reservation =
            match instance::acquire(&session.support_dir.join("gwnative.lock"), Duration::ZERO) {
                Ok(lock) => lock,
                Err(reason) => {
                    session.state = SessionState::Failed { reason };
                    return;
                }
            };
        let record = SessionRecord::pending(&profile_id, &session.nonce);
        if let Err(error) = write_record(&session.support_dir, &record) {
            drop(reservation);
            session.state = SessionState::Failed { reason: error };
            return;
        }
        let Some(spec) = session.launch_spec.clone() else {
            session.state = SessionState::Failed {
                reason: "queued session omitted launch snapshot".into(),
            };
            return;
        };
        match self.runner.spawn(&spec) {
            Ok(pid) => {
                let Some(identity) = self.runner.process_identity(pid) else {
                    let cleanup = self.runner.terminate_reap(pid);
                    drop(reservation);
                    if let Err(error) = cleanup {
                        // Child remains owned by runner but cannot be safely
                        // discarded. Pending nonce stays available for child
                        // host; PID keeps Play disabled until it exits.
                        session.pid = Some(pid);
                        session.started = Some(Instant::now());
                        session.state = SessionState::AwaitingWindow;
                        eprintln!("[launcher] could not establish spawned game identity: {error}");
                    } else {
                        let _ = clear_record(&session.support_dir, &session.nonce);
                        session.state = SessionState::Failed {
                            reason: "could not establish spawned game identity".into(),
                        };
                    }
                    return;
                };
                if let Err(error) = write_record(
                    &session.support_dir,
                    &SessionRecord {
                        version: VERSION,
                        profile_id: profile_id.clone(),
                        nonce: session.nonce.clone(),
                        pid,
                        process_identity: identity.clone(),
                        handoff_pid: None,
                        handoff_identity: None,
                    },
                ) {
                    let cleanup = self.runner.terminate_reap(pid);
                    drop(reservation);
                    if let Err(cleanup) = cleanup {
                        session.pid = Some(pid);
                        session.process_identity = Some(identity);
                        session.started = Some(Instant::now());
                        session.state = SessionState::AwaitingWindow;
                        eprintln!(
                            "[launcher] could not publish spawned game identity: {error}; {cleanup}"
                        );
                    } else {
                        let _ = clear_record(&session.support_dir, &session.nonce);
                        session.state = SessionState::Failed { reason: error };
                    }
                    return;
                }
                // Record is committed before reservation release. The child
                // cannot acquire profile lock and replace this record earlier.
                drop(reservation);
                session.pid = Some(pid);
                session.process_identity = Some(identity);
                session.started = Some(Instant::now());
                session.state = SessionState::AwaitingWindow;
            }
            Err(error) => {
                let _ = clear_record(&session.support_dir, &session.nonce);
                drop(reservation);
                session.state = SessionState::Failed { reason: error };
            }
        }
    }

    pub fn show(&mut self, profile_id: &str) -> Result<bool, String> {
        self.request_command(profile_id, HostCommand::Show)
    }

    pub fn capture_window_layout(
        &self,
        profile_id: &str,
        request_id: &str,
    ) -> Result<bool, String> {
        if !valid_capture_request_id(request_id) {
            return Err(
                "window-layout capture request ID must be 32 hexadecimal characters".into(),
            );
        }
        self.request_command(
            profile_id,
            HostCommand::CaptureWindowLayout {
                request_id: request_id.into(),
            },
        )
    }

    /// Orderly close only.  Force quit deliberately remains separate UI action.
    pub fn close(&mut self, profile_id: &str) -> Result<bool, String> {
        let Some(session) = self.sessions.get(profile_id) else {
            return Ok(false);
        };
        if !matches!(
            session.state,
            SessionState::AwaitingWindow
                | SessionState::Running { .. }
                | SessionState::Closing { .. }
        ) {
            return Ok(false);
        }
        let result = self
            .probe
            .command(&session.support_dir, &session.nonce, HostCommand::Close);
        let pid = session.pid;
        let identity = session.process_identity.clone();
        // Host may be wedged before it can acknowledge orderly shutdown.  Keep
        // game busy and enter Closing so UI can offer explicit Force Quit later.
        if let Some(pid) = pid
            && let Some(session) = self.sessions.get_mut(profile_id)
        {
            session.state = SessionState::Closing { pid };
            session.close_requested = true;
            if result.is_ok() {
                session.close_target_identity = identity;
            }
        }
        match result {
            Ok(()) => Ok(true),
            Err(_) if pid.is_some() => Ok(true),
            Err(error) => Err(error),
        }
    }

    /// Last-resort action from an explicit confirmation.  It rechecks live
    /// nonce-authenticated identity immediately before SIGKILL; PID from record
    /// alone is never enough to target another process.
    pub fn force_quit_explicit(&mut self, profile_id: &str) -> Result<bool, String> {
        let Some(session) = self.sessions.get(profile_id) else {
            return Ok(false);
        };
        let Some(expected_pid) = session.pid else {
            return Ok(false);
        };
        let Some(identity) = session.process_identity.as_deref() else {
            return Ok(false);
        };
        if !self.probe.profile_locked(&session.support_dir)?
            || process_identity(expected_pid).as_deref() != Some(identity)
        {
            return Ok(false);
        }
        // SAFETY: pid has just been confirmed by game-held, nonce-protected IPC.
        if unsafe { kill(expected_pid as i32, SIGKILL) } != 0 {
            return Err(format!(
                "could not force quit game process {expected_pid}: {}",
                std::io::Error::last_os_error()
            ));
        }
        if let Some(session) = self.sessions.get_mut(profile_id) {
            session.state = SessionState::Closing { pid: expected_pid };
        }
        Ok(true)
    }

    pub fn snapshots(&self) -> Vec<SessionSnapshot> {
        self.sessions
            .iter()
            .map(|(profile_id, session)| SessionSnapshot {
                profile_id: profile_id.clone(),
                state: session.state.clone(),
            })
            .collect()
    }

    fn request_command(&self, profile_id: &str, command: HostCommand) -> Result<bool, String> {
        let Some(session) = self.sessions.get(profile_id) else {
            return Ok(false);
        };
        if !matches!(
            session.state,
            SessionState::AwaitingWindow
                | SessionState::Running { .. }
                | SessionState::Closing { .. }
        ) {
            return Ok(false);
        }
        self.probe
            .command(&session.support_dir, &session.nonce, command)?;
        Ok(true)
    }

    fn reconcile(&mut self) {
        for (profile_id, session) in &mut self.sessions {
            let pending = matches!(
                session.state,
                SessionState::AwaitingWindow
                    | SessionState::Running { .. }
                    | SessionState::Closing { .. }
            );
            if !pending {
                continue;
            }
            match self.probe.inspect(&session.support_dir, &session.nonce) {
                Ok(Some(live)) if live.profile_id == *profile_id => {
                    // Same nonce and authenticated socket permit a relaunch
                    // successor with a new PID to replace its predecessor.
                    session.pid = Some(live.pid);
                    session.process_identity = Some(live.process_identity);
                    if (live.ready && matches!(session.state, SessionState::AwaitingWindow))
                        || matches!(session.state, SessionState::Running { .. })
                    {
                        session.state = SessionState::Running { pid: live.pid };
                    } else if matches!(session.state, SessionState::Closing { .. }) {
                        session.state = SessionState::Closing { pid: live.pid };
                    }
                    if matches!(session.state, SessionState::Closing { .. })
                        && session.close_requested
                        && session.close_target_identity.as_deref()
                            != session.process_identity.as_deref()
                        && self
                            .probe
                            .command(&session.support_dir, &session.nonce, HostCommand::Close)
                            .is_ok()
                    {
                        session.close_target_identity = session.process_identity.clone();
                    }
                }
                Ok(None) => {
                    // A launch can be between exec and lock acquisition.  Do not
                    // overwrite its nonce record or launch another same-profile
                    // game.  Parent may report failure after child-exit evidence.
                    // Cold game preparation may take longer than a fixed UI
                    // timeout.  Keep profile busy until child-exit plus lock
                    // evidence says it ended, rather than risking a duplicate.
                }
                Ok(Some(_)) => {
                    session.state = SessionState::Failed {
                        reason: "game session identity changed".into(),
                    }
                }
                Err(_) => {
                    // Transient socket errors must not erase a live session's
                    // control path or permit a duplicate profile launch.
                }
            }
        }
        let released = self
            .sessions
            .iter()
            .filter_map(|(profile, session)| {
                let pid = session.pid?;
                let no_host = self
                    .probe
                    .inspect(&session.support_dir, &session.nonce)
                    .ok()
                    .flatten()
                    .is_none();
                let exited = self.runner.exited(pid).ok().flatten() == Some(true)
                    || session.process_identity.as_deref().is_some_and(|identity| {
                        self.runner.process_identity(pid).as_deref() != Some(identity)
                    });
                let unlocked = self.probe.profile_locked(&session.support_dir).ok() == Some(false);
                let handoff = read_record(&session.support_dir)
                    .ok()
                    .flatten()
                    .is_some_and(|record| {
                        record.nonce == session.nonce
                            && record.handoff_pid.zip(record.handoff_identity).is_some_and(
                                |(pid, identity)| {
                                    process_identity(pid).as_deref() == Some(identity.as_str())
                                },
                            )
                    });
                (no_host && exited && unlocked && !handoff).then(|| profile.clone())
            })
            .collect::<Vec<_>>();
        for profile in released {
            if let Some(session) = self.sessions.remove(&profile) {
                let _ = clear_record(&session.support_dir, &session.nonce);
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionRecord {
    version: u32,
    profile_id: String,
    nonce: String,
    pid: u32,
    #[serde(default)]
    process_identity: String,
    #[serde(default)]
    handoff_pid: Option<u32>,
    #[serde(default)]
    handoff_identity: Option<String>,
}

impl SessionRecord {
    fn pending(profile_id: &str, nonce: &str) -> Self {
        Self {
            version: VERSION,
            profile_id: profile_id.into(),
            nonce: nonce.into(),
            pid: 0,
            process_identity: String::new(),
            handoff_pid: None,
            handoff_identity: None,
        }
    }
}

/// Game-process end of session protocol.  `lock` remains owned until process
/// exit, so socket state cannot outlive profile exclusion.
pub struct GameHost {
    listener: UnixListener,
    support_dir: PathBuf,
    profile_id: String,
    nonce: String,
    pid: u32,
    process_identity: String,
    ready: bool,
    _lock: instance::Instance,
}
static RELAUNCH_HOST: OnceLock<Arc<Mutex<GameHost>>> = OnceLock::new();
pub fn install_relaunch_host(host: Arc<Mutex<GameHost>>) -> Result<(), String> {
    RELAUNCH_HOST
        .set(host)
        .map_err(|_| "relaunch host already installed".into())
}
pub fn commit_relaunch(pid: u32) -> Result<(), String> {
    let Some(host) = RELAUNCH_HOST.get() else {
        return Ok(());
    };
    host.lock()
        .map_err(|_| "relaunch host unavailable".to_owned())?
        .commit_relaunch(pid)
}

impl GameHost {
    /// Call after game-specific profile lock is acquired.  Existing default
    /// profile works in-place because caller supplies historical support dir.
    pub fn start(
        support_dir: PathBuf,
        profile_id: &str,
        lock: instance::Instance,
    ) -> Result<Self, String> {
        let record = read_record(&support_dir)?.unwrap_or_else(|| {
            // Explicit `--game` remains useful for adopted legacy/default
            // profiles.  It publishes same authenticated contract for a later
            // launcher reconnect, but does not imply launcher-managed launch.
            SessionRecord::pending(profile_id, &nonce())
        });
        if record.version != VERSION
            || record.profile_id != profile_id
            || !valid_nonce(&record.nonce)
        {
            return Err("launcher session record is invalid".into());
        }
        let path = socket_path(&record.nonce);
        let _ = fs::remove_file(&path);
        let listener = UnixListener::bind(&path)
            .map_err(|error| format!("could not open game session socket: {error}"))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("could not protect game session socket: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("could not configure game session socket: {error}"))?;
        let pid = std::process::id();
        let process_identity = process_identity(pid)
            .ok_or_else(|| "could not establish game process identity".to_owned())?;
        write_record(
            &support_dir,
            &SessionRecord {
                pid,
                process_identity: process_identity.clone(),
                handoff_pid: None,
                handoff_identity: None,
                ..record.clone()
            },
        )?;
        Ok(Self {
            listener,
            support_dir,
            profile_id: profile_id.into(),
            nonce: record.nonce,
            pid,
            process_identity,
            ready: false,
            _lock: lock,
        })
    }

    pub fn ready(&mut self) {
        self.ready = true;
    }

    /// Commit an already-spawned successor before predecessor releases lock.
    fn commit_relaunch(&self, pid: u32) -> Result<(), String> {
        let mut record = read_record(&self.support_dir)?
            .ok_or_else(|| "launcher session record is absent".to_owned())?;
        if record.nonce != self.nonce || record.pid != self.pid {
            return Err("launcher session identity changed".into());
        }
        record.handoff_pid = Some(pid);
        record.handoff_identity = process_identity(pid);
        if record.handoff_identity.is_none() {
            return Err("could not establish successor identity".into());
        }
        write_record(&self.support_dir, &record)
    }

    /// Main-thread polling.  Caller decides how Show activates its NSWindow and
    /// how Close calls `app::request_quit`; no control request changes UI here.
    pub fn poll(&self) -> Result<Vec<HostCommand>, String> {
        let mut commands = Vec::new();
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(command) = self.handle(stream)? {
                        commands.push(command);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    return Ok(commands);
                }
                Err(error) => {
                    return Err(format!("could not accept game session command: {error}"));
                }
            }
        }
    }

    fn handle(&self, mut stream: UnixStream) -> Result<Option<HostCommand>, String> {
        stream
            .set_read_timeout(Some(IPC_TIMEOUT))
            .map_err(|error| format!("could not bound game session read: {error}"))?;
        stream
            .set_write_timeout(Some(IPC_TIMEOUT))
            .map_err(|error| format!("could not bound game session write: {error}"))?;
        if !same_user(&stream) {
            return Ok(None);
        }
        let mut request = Vec::new();
        BufReader::new(&stream)
            .take(MAX_REQUEST_BYTES + 1)
            .read_until(b'\n', &mut request)
            .map_err(|error| format!("could not read game session command: {error}"))?;
        if request.len() > MAX_REQUEST_BYTES as usize {
            return Ok(None);
        }
        let request = std::str::from_utf8(&request).ok().unwrap_or_default();
        let mut fields = request.trim_end().split(' ');
        let Some(token) = fields.next() else {
            return Ok(None);
        };
        let Some(verb) = fields.next() else {
            return Ok(None);
        };
        if token != self.nonce {
            return Ok(None);
        }
        let command = match verb {
            "state" if fields.next().is_none() => {
                writeln!(
                    stream,
                    "ok {} {} {} {}",
                    self.profile_id,
                    self.pid,
                    u8::from(self.ready),
                    self.process_identity,
                )
                .map_err(|error| format!("could not reply to game session probe: {error}"))?;
                return Ok(None);
            }
            "show" if fields.next().is_none() => HostCommand::Show,
            "close" if fields.next().is_none() => HostCommand::Close,
            "capture-window-layout" => {
                let Some(request_id) = fields.next().filter(|id| valid_capture_request_id(id))
                else {
                    return Ok(None);
                };
                if fields.next().is_some() {
                    return Ok(None);
                }
                HostCommand::CaptureWindowLayout {
                    request_id: request_id.into(),
                }
            }
            _ => return Ok(None),
        };
        writeln!(stream, "ok")
            .map_err(|error| format!("could not acknowledge game session command: {error}"))?;
        Ok(Some(command))
    }
}

impl Drop for GameHost {
    fn drop(&mut self) {
        let _ = clear_record(&self.support_dir, &self.nonce);
        let _ = fs::remove_file(socket_path(&self.nonce));
    }
}

pub struct IpcProbe;

impl SessionProbe for IpcProbe {
    fn inspect(
        &self,
        support_dir: &Path,
        expected_nonce: &str,
    ) -> Result<Option<LiveSession>, String> {
        let Some(record) = read_record(support_dir)? else {
            return Ok(None);
        };
        if record.nonce != expected_nonce || record.pid == 0 {
            return Ok(None);
        }
        let mut stream = match UnixStream::connect(socket_path(&record.nonce)) {
            Ok(stream) => stream,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(format!("could not reach game session: {error}")),
        };
        stream
            .set_read_timeout(Some(IPC_TIMEOUT))
            .map_err(|error| format!("could not bound game-session read: {error}"))?;
        stream
            .set_write_timeout(Some(IPC_TIMEOUT))
            .map_err(|error| format!("could not bound game-session write: {error}"))?;
        writeln!(stream, "{expected_nonce} state")
            .map_err(|error| format!("could not query game session: {error}"))?;
        let mut response = String::new();
        BufReader::new(stream)
            .take(MAX_REQUEST_BYTES)
            .read_to_string(&mut response)
            .map_err(|error| format!("could not read game session state: {error}"))?;
        let fields = response.trim_end().split(' ').collect::<Vec<_>>();
        if fields.len() != 5
            || fields[0] != "ok"
            || fields[1] != record.profile_id
            || fields[2].parse().ok() != Some(record.pid)
            || record.process_identity != fields[4]
            || process_identity(record.pid).as_deref() != Some(fields[4])
        {
            return Ok(None);
        }
        Ok(Some(LiveSession {
            profile_id: record.profile_id,
            pid: record.pid,
            ready: fields[3] == "1",
            process_identity: fields[4].into(),
        }))
    }

    fn command(
        &self,
        _support_dir: &Path,
        expected_nonce: &str,
        command: HostCommand,
    ) -> Result<(), String> {
        let (verb, argument) = match command {
            HostCommand::Show => ("show", None),
            HostCommand::Close => ("close", None),
            HostCommand::CaptureWindowLayout { request_id } => {
                ("capture-window-layout", Some(request_id))
            }
        };
        let mut stream = UnixStream::connect(socket_path(expected_nonce))
            .map_err(|error| format!("could not reach game session: {error}"))?;
        stream
            .set_read_timeout(Some(IPC_TIMEOUT))
            .map_err(|error| format!("could not bound game-session read: {error}"))?;
        stream
            .set_write_timeout(Some(IPC_TIMEOUT))
            .map_err(|error| format!("could not bound game-session write: {error}"))?;
        if let Some(argument) = argument {
            writeln!(stream, "{expected_nonce} {verb} {argument}")
        } else {
            writeln!(stream, "{expected_nonce} {verb}")
        }
        .map_err(|error| format!("could not send game session command: {error}"))?;
        let mut response = String::new();
        BufReader::new(stream)
            .take(MAX_REQUEST_BYTES)
            .read_to_string(&mut response)
            .map_err(|error| format!("could not read game session acknowledgement: {error}"))?;
        if response.trim_end() != "ok" {
            return Err("game refused session command".into());
        }
        Ok(())
    }

    fn profile_locked(&self, support_dir: &Path) -> Result<bool, String> {
        match instance::acquire_shared(&support_dir.join("gwnative.lock"), Duration::ZERO) {
            Ok(lock) => {
                drop(lock);
                Ok(false)
            }
            Err(_) => Ok(true),
        }
    }
}

fn record_path(support_dir: &Path) -> PathBuf {
    support_dir.join(RECORD)
}
fn socket_path(nonce: &str) -> PathBuf {
    // Unix-domain paths cap around 104 bytes on macOS.  Support paths include
    // Application Support and user-selected profile names, so keep socket in
    // short system temp directory; random nonce prevents cross-profile reuse.
    let prefix = nonce.get(..24).unwrap_or(nonce);
    std::env::temp_dir().join(format!("gwnative-{prefix}.sock"))
}

fn read_record(support_dir: &Path) -> Result<Option<SessionRecord>, String> {
    let path = record_path(support_dir);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("could not read launcher session record: {error}")),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("could not parse launcher session record: {error}"))
}

fn write_record(support_dir: &Path, record: &SessionRecord) -> Result<(), String> {
    fs::create_dir_all(support_dir)
        .map_err(|error| format!("could not create profile support directory: {error}"))?;
    let path = record_path(support_dir);
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec(record)
        .map_err(|error| format!("could not encode launcher session record: {error}"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|error| format!("could not create launcher session record: {error}"))?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("could not write launcher session record: {error}"))?;
    fs::rename(&temporary, &path)
        .map_err(|error| format!("could not publish launcher session record: {error}"))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("could not protect launcher session record: {error}"))
}

fn clear_record(support_dir: &Path, nonce: &str) -> Result<(), String> {
    if read_record(support_dir)?.is_some_and(|record| record.nonce == nonce) {
        match fs::remove_file(record_path(support_dir)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("could not remove launcher session record: {error}")),
        }
    } else {
        Ok(())
    }
}

fn valid_profile_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_capture_request_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_nonce(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Kernel process-start identity.  PID reuse cannot satisfy this value because
/// `proc_pidinfo` reports the actual process start down to microseconds.
fn process_identity(pid: u32) -> Option<String> {
    let mut info = ProcBsdInfo::default();
    let bytes = unsafe {
        proc_pidinfo(
            pid as i32,
            PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut ProcBsdInfo).cast(),
            std::mem::size_of::<ProcBsdInfo>() as i32,
        )
    };
    (bytes == std::mem::size_of::<ProcBsdInfo>() as i32)
        .then(|| format!("{pid}:{}:{}", info.start_seconds, info.start_microseconds))
}

// Prefix/layout of macOS `proc_bsdinfo` from libproc.h.  Only start timestamp
// is consumed, but full preceding layout prevents reading an offset by guess.
#[repr(C)]
struct ProcBsdInfo {
    flags: u32,
    status: u32,
    xstatus: u32,
    pid: u32,
    ppid: u32,
    uid: u32,
    gid: u32,
    ruid: u32,
    rgid: u32,
    svuid: u32,
    svgid: u32,
    rfu_1: u32,
    comm: [i8; 16],
    name: [i8; 32],
    nfiles: u32,
    pgid: u32,
    pjobc: u32,
    e_tdev: u32,
    tdev: u32,
    nice: i32,
    start_seconds: i64,
    start_microseconds: i64,
}

impl Default for ProcBsdInfo {
    fn default() -> Self {
        // SAFETY: all-zero is valid scratch storage for C output structure.
        unsafe { std::mem::zeroed() }
    }
}

fn nonce() -> String {
    let mut bytes = [0u8; 32];
    // macOS arc4random is kernel-seeded.  Session nonce is capability material,
    // unlike PID it must be unguessable by another local process.
    unsafe { arc4random_buf(bytes.as_mut_ptr().cast(), bytes.len()) };
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn same_user(stream: &UnixStream) -> bool {
    let mut uid = 0;
    let mut gid = 0;
    unsafe { getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) == 0 && uid == geteuid() }
}

unsafe extern "C" {
    fn arc4random_buf(buffer: *mut std::ffi::c_void, length: usize);
    fn getpeereid(socket: i32, euid: *mut u32, egid: *mut u32) -> i32;
    fn geteuid() -> u32;
    fn kill(pid: i32, signal: i32) -> i32;
}

#[link(name = "proc")]
unsafe extern "C" {
    fn proc_pidinfo(
        pid: i32,
        flavor: i32,
        arg: u64,
        buffer: *mut std::ffi::c_void,
        buffersize: i32,
    ) -> i32;
}

const SIGKILL: i32 = 9;
const PROC_PIDTBSDINFO: i32 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    struct Runner {
        next: u32,
        launches: Vec<LaunchSpec>,
        fail: bool,
        exited: bool,
        identity: bool,
        terminated: Vec<u32>,
        cleanup_fail: bool,
    }
    impl Default for Runner {
        fn default() -> Self {
            Self {
                next: 0,
                launches: Vec::new(),
                fail: false,
                exited: false,
                identity: true,
                terminated: Vec::new(),
                cleanup_fail: false,
            }
        }
    }
    impl GameRunner for Runner {
        fn spawn(&mut self, spec: &LaunchSpec) -> Result<u32, String> {
            self.launches.push(spec.clone());
            if self.fail {
                Err("spawn failed".into())
            } else {
                self.next += 1;
                Ok(self.next)
            }
        }
        fn exited(&mut self, _pid: u32) -> Result<Option<bool>, String> {
            Ok(Some(self.exited))
        }
        fn process_identity(&self, pid: u32) -> Option<String> {
            self.identity.then(|| format!("runner-{pid}"))
        }
        fn terminate_reap(&mut self, pid: u32) -> Result<(), String> {
            self.terminated.push(pid);
            if self.cleanup_fail {
                Err("reap failed".into())
            } else {
                Ok(())
            }
        }
    }
    #[derive(Default)]
    struct Probe {
        states: std::cell::RefCell<BTreeMap<String, LiveSession>>,
        commands: std::cell::RefCell<Vec<HostCommand>>,
        locked: std::cell::Cell<bool>,
        fail_command: std::cell::Cell<bool>,
    }
    impl SessionProbe for Probe {
        fn inspect(&self, support: &Path, _nonce: &str) -> Result<Option<LiveSession>, String> {
            Ok(self
                .states
                .borrow()
                .get(&support.display().to_string())
                .cloned())
        }
        fn command(
            &self,
            _support: &Path,
            _nonce: &str,
            command: HostCommand,
        ) -> Result<(), String> {
            if self.fail_command.get() {
                return Err("socket timed out".into());
            }
            self.commands.borrow_mut().push(command);
            Ok(())
        }
        fn profile_locked(&self, _support: &Path) -> Result<bool, String> {
            Ok(self.locked.get())
        }
    }
    fn path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "gwnative-launcher-session-{name}-{}",
            std::process::id()
        ))
    }
    fn controller() -> SessionController<Runner, Probe> {
        SessionController::new(
            PathBuf::from("/Applications/GWNative.app/Contents/MacOS/GWNative"),
            Runner::default(),
            Probe::default(),
        )
    }

    #[test]
    fn queue_waits_for_first_window_before_second_spawn() {
        let mut controller = controller();
        let first = path("first");
        let second = path("second");
        assert!(controller.request("first", first.clone()));
        assert!(controller.request("second", second.clone()));
        controller.tick();
        controller.tick();
        assert_eq!(controller.runner.launches.len(), 1);
        controller.probe.states.borrow_mut().insert(
            first.display().to_string(),
            LiveSession {
                profile_id: "first".into(),
                pid: 1,
                ready: true,
                process_identity: "first".into(),
            },
        );
        controller.tick();
        assert_eq!(controller.runner.launches.len(), 2);
        assert_eq!(
            controller.runner.launches[0].args[1..],
            ["--game", "--profile", "first", "--new-instance"]
        );
    }

    #[test]
    fn queued_spec_keeps_its_options_and_environment_snapshot() {
        let mut controller = controller();
        let mut spec = LaunchSpec::game(&controller.executable, "frozen");
        spec.options.muted = true;
        spec.options.preferred_character = Some("Koss".into());
        spec.env
            .insert("GWNATIVE_TEXTURE_MANIFEST".into(), "revision-a".into());
        assert!(controller.request_spec(spec, path("frozen")));
        controller.tick();
        let launched = &controller.runner.launches[0];
        assert!(launched.options.muted);
        assert_eq!(
            launched.options.preferred_character.as_deref(),
            Some("Koss")
        );
        assert_eq!(
            launched
                .env
                .get("GWNATIVE_TEXTURE_MANIFEST")
                .map(String::as_str),
            Some("revision-a")
        );
    }

    #[test]
    fn current_executable_forwards_mute_to_child_argv() {
        use std::os::unix::fs::OpenOptionsExt;

        let stem = format!("gwnative-launcher-argv-{}-{}", std::process::id(), nonce());
        let script = std::env::temp_dir().join(format!("{stem}.sh"));
        let output = std::env::temp_dir().join(format!("{stem}.sh.args"));
        let script_body = "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\n";
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o755)
            .open(&script)
            .expect("create argv fixture");
        std::fs::write(&script, script_body.as_bytes()).expect("write argv fixture");

        let invocation = crate::cli::parse(["-nosound"]).unwrap();
        let mut runner = CurrentExecutable::with_game_options(
            invocation.offline,
            invocation.no_update,
            invocation.legacy.mute,
        );
        let mut spec = LaunchSpec::game(&script, "fixture");
        spec.options.muted = true;
        let pid = runner.spawn(&spec).expect("spawn argv fixture");
        for _ in 0..100 {
            if runner.exited(pid).expect("observe argv fixture") == Some(true) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let args = std::fs::read_to_string(&output).expect("read captured argv");
        assert!(args.lines().any(|arg| arg == "-nosound"));
        assert!(
            !args
                .lines()
                .any(|arg| arg.contains("password") || arg.contains("email"))
        );

        let _ = std::fs::remove_file(script);
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn resolved_launch_arguments_are_ordered_and_never_transport_character() {
        let options = crate::launcher_preferences::ResolvedLaunchOptions {
            muted: true,
            frame_rate_limit: crate::launcher_preferences::FrameRateLimit::Limit(144),
            window_mode: crate::launcher_preferences::WindowMode::Fullscreen,
            window_frame: None,
            preferred_character: Some("Koss".into()),
            texture_pack_ids: vec!["tpf:ui".into()],
        };
        assert_eq!(
            supported_option_args(&options),
            ["-nosound", "-fps", "144", "-windowedfullscreen"]
        );
    }

    #[test]
    fn child_uses_frozen_launch_environment_not_inherited_values() {
        const CHILD: &str = "GWNATIVE_SESSION_ENV_ISOLATION_CHILD";
        if std::env::var(CHILD).as_deref() != Ok("1") {
            let status = Command::new(std::env::current_exe().expect("test executable"))
                .arg("launcher_sessions::tests::child_uses_frozen_launch_environment_not_inherited_values")
                .arg("--exact")
                .arg("--nocapture")
                .env(CHILD, "1")
                .env("GWNATIVE_ACCOUNT_AUTO_LOGIN", "inherited")
                .env("GWNATIVE_TEXTURE_MANIFEST", "inherited")
                .env("GWNATIVE_RESTORE_MAXIMIZED", "inherited")
                .env("GWNATIVE_WINDOW_SNAPSHOT", "inherited")
                .env("GWNATIVE_LAUNCH_OPTIONS", "inherited")
                .status()
                .expect("start isolated test child");
            assert!(status.success(), "isolated environment child failed");
            return;
        }
        use std::os::unix::fs::OpenOptionsExt;

        let stem = format!("gwnative-launcher-env-{}-{}", std::process::id(), nonce());
        let script = std::env::temp_dir().join(format!("{stem}.sh"));
        let output = std::env::temp_dir().join(format!("{stem}.sh.env"));
        let script_body = "#!/bin/sh\nprintf '%s\\n' \"$GWNATIVE_ACCOUNT_AUTO_LOGIN\" \"$GWNATIVE_TEXTURE_MANIFEST\" \"$GWNATIVE_RESTORE_MAXIMIZED\" \"$GWNATIVE_WINDOW_SNAPSHOT\" \"$GWNATIVE_LAUNCH_OPTIONS\" > \"$0.env\"\n";
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o755)
            .open(&script)
            .expect("create environment fixture");
        std::fs::write(&script, script_body.as_bytes()).expect("write environment fixture");

        let mut spec = LaunchSpec::game(&script, "fixture");
        spec.options.muted = true;
        spec.env.extend([
            ("GWNATIVE_ACCOUNT_AUTO_LOGIN".into(), "true".into()),
            ("GWNATIVE_TEXTURE_MANIFEST".into(), "pinned".into()),
            ("GWNATIVE_RESTORE_MAXIMIZED".into(), "1".into()),
            ("GWNATIVE_WINDOW_SNAPSHOT".into(), "frozen-frame".into()),
        ]);
        let mut runner = CurrentExecutable::default();
        let pid = runner.spawn(&spec).expect("spawn environment fixture");
        for _ in 0..100 {
            if runner.exited(pid).expect("observe environment fixture") == Some(true) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let values = std::fs::read_to_string(&output).expect("read captured environment");
        let lines = values.lines().collect::<Vec<_>>();
        assert_eq!(&lines[..4], ["true", "pinned", "1", "frozen-frame"]);
        assert!(lines[4].contains("\"muted\":true"));
        let _ = std::fs::remove_file(script);
        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn cancel_cannot_kill_spawned_game() {
        let mut controller = controller();
        let support = path("cancel");
        assert!(controller.request("cancel", support));
        assert!(controller.cancel_queued("cancel"));
        assert!(controller.request("cancel", path("cancel")));
        controller.tick();
        assert!(!controller.cancel_queued("cancel"));
        assert_eq!(controller.runner.launches.len(), 1);
    }

    #[test]
    fn reconnect_requires_live_nonce_authenticated_reply() {
        let mut controller = controller();
        let support = path("reconnect");
        let _ = fs::remove_dir_all(&support);
        fs::create_dir_all(&support).unwrap();
        write_record(
            &support,
            &SessionRecord {
                version: VERSION,
                profile_id: "iron".into(),
                nonce: "a".repeat(64),
                pid: 42,
                process_identity: "iron".into(),
                handoff_pid: None,
                handoff_identity: None,
            },
        )
        .unwrap();
        assert!(!controller.reconnect("iron", support.clone()).unwrap());
        controller.probe.states.borrow_mut().insert(
            support.display().to_string(),
            LiveSession {
                profile_id: "iron".into(),
                pid: 42,
                ready: true,
                process_identity: "iron".into(),
            },
        );
        assert!(controller.reconnect("iron", support.clone()).unwrap());
        assert_eq!(
            controller.snapshots()[0].state,
            SessionState::Running { pid: 42 }
        );
        let _ = fs::remove_dir_all(support);
    }

    #[test]
    fn close_and_show_are_explicit_host_commands() {
        let mut controller = controller();
        let support = path("commands");
        assert!(controller.request("iron", support.clone()));
        controller.tick();
        controller.probe.states.borrow_mut().insert(
            support.display().to_string(),
            LiveSession {
                profile_id: "iron".into(),
                pid: 1,
                ready: true,
                process_identity: "iron".into(),
            },
        );
        controller.tick();
        assert!(controller.show("iron").unwrap());
        assert!(controller.close("iron").unwrap());
        assert_eq!(
            *controller.probe.commands.borrow(),
            vec![HostCommand::Show, HostCommand::Close]
        );
    }

    #[test]
    fn capture_layout_is_only_sent_to_a_live_game() {
        let mut controller = controller();
        let request_id = "a".repeat(32);
        assert!(
            !controller
                .capture_window_layout("missing", &request_id)
                .unwrap()
        );
        let support = path("capture-layout");
        assert!(controller.request("iron", support.clone()));
        controller.tick();
        assert!(
            controller
                .capture_window_layout("iron", &request_id)
                .unwrap()
        );
        assert_eq!(
            *controller.probe.commands.borrow(),
            vec![HostCommand::CaptureWindowLayout { request_id }]
        );
    }

    #[test]
    fn capture_layout_rejects_an_unaddressable_request_id() {
        let controller = controller();
        assert!(controller.capture_window_layout("iron", "short").is_err());
    }

    #[test]
    fn reconnect_adopts_authenticated_preparing_game() {
        let mut controller = controller();
        let support = path("reconnect-preparing");
        let _ = fs::remove_dir_all(&support);
        fs::create_dir_all(&support).unwrap();
        write_record(
            &support,
            &SessionRecord {
                version: VERSION,
                profile_id: "iron".into(),
                nonce: "b".repeat(64),
                pid: 42,
                process_identity: "iron".into(),
                handoff_pid: None,
                handoff_identity: None,
            },
        )
        .unwrap();
        controller.probe.states.borrow_mut().insert(
            support.display().to_string(),
            LiveSession {
                profile_id: "iron".into(),
                pid: 42,
                ready: false,
                process_identity: "iron".into(),
            },
        );
        assert!(controller.reconnect("iron", support.clone()).unwrap());
        assert_eq!(
            controller.snapshots()[0].state,
            SessionState::AwaitingWindow
        );
        let _ = fs::remove_dir_all(support);
    }

    #[test]
    fn close_is_allowed_while_window_is_preparing() {
        let mut controller = controller();
        let support = path("close-preparing");
        assert!(controller.request("iron", support));
        controller.tick();
        assert!(controller.close("iron").unwrap());
        assert!(matches!(
            controller.snapshots()[0].state,
            SessionState::Closing { .. }
        ));
    }

    #[test]
    fn close_before_handshake_is_delivered_once_when_host_arrives() {
        let mut controller = controller();
        let support = path("late-close");
        assert!(controller.request("iron", support.clone()));
        controller.tick();
        controller.probe.fail_command.set(true);
        assert!(controller.close("iron").unwrap());
        controller.probe.fail_command.set(false);
        controller.probe.states.borrow_mut().insert(
            support.display().to_string(),
            LiveSession {
                profile_id: "iron".into(),
                pid: 1,
                ready: true,
                process_identity: "iron".into(),
            },
        );
        controller.tick();
        controller.tick();
        assert_eq!(
            *controller.probe.commands.borrow(),
            vec![HostCommand::Close]
        );
    }

    #[test]
    fn reconnect_release_uses_identity_and_unlocked_profile_without_child_handle() {
        let mut controller = controller();
        let support = path("reconnect-release");
        let _ = fs::remove_dir_all(&support);
        fs::create_dir_all(&support).unwrap();
        write_record(
            &support,
            &SessionRecord {
                version: VERSION,
                profile_id: "iron".into(),
                nonce: "c".repeat(64),
                pid: 42,
                process_identity: "stale".into(),
                handoff_pid: None,
                handoff_identity: None,
            },
        )
        .unwrap();
        controller.probe.states.borrow_mut().insert(
            support.display().to_string(),
            LiveSession {
                profile_id: "iron".into(),
                pid: 42,
                ready: true,
                process_identity: "stale".into(),
            },
        );
        assert!(controller.reconnect("iron", support.clone()).unwrap());
        controller.probe.states.borrow_mut().clear();
        controller.probe.locked.set(false);
        controller.tick();
        assert!(controller.snapshots().is_empty());
        let _ = fs::remove_dir_all(support);
    }

    #[test]
    fn relaunch_handoff_keeps_profile_busy_across_parent_lock_gap() {
        let mut controller = controller();
        let support = path("handoff-gap");
        let pid = std::process::id();
        let identity = process_identity(pid).unwrap();
        let _ = fs::remove_dir_all(&support);
        fs::create_dir_all(&support).unwrap();
        write_record(
            &support,
            &SessionRecord {
                version: VERSION,
                profile_id: "iron".into(),
                nonce: "e".repeat(64),
                pid,
                process_identity: identity.clone(),
                handoff_pid: Some(pid),
                handoff_identity: Some(identity.clone()),
            },
        )
        .unwrap();
        controller.probe.states.borrow_mut().insert(
            support.display().to_string(),
            LiveSession {
                profile_id: "iron".into(),
                pid,
                ready: true,
                process_identity: identity,
            },
        );
        assert!(controller.reconnect("iron", support.clone()).unwrap());
        controller.probe.states.borrow_mut().clear();
        controller.probe.locked.set(false);
        controller.tick();
        assert!(!controller.snapshots().is_empty());
        let _ = fs::remove_dir_all(support);
    }

    #[test]
    fn explicit_force_quit_targets_birth_checked_hung_game() {
        let mut child = Command::new("/bin/sleep").arg("10").spawn().unwrap();
        let pid = child.id();
        let identity = process_identity(pid).unwrap();
        let mut controller = controller();
        let support = path("force");
        let _ = fs::remove_dir_all(&support);
        fs::create_dir_all(&support).unwrap();
        write_record(
            &support,
            &SessionRecord {
                version: VERSION,
                profile_id: "iron".into(),
                nonce: "d".repeat(64),
                pid,
                process_identity: identity.clone(),
                handoff_pid: None,
                handoff_identity: None,
            },
        )
        .unwrap();
        controller.probe.states.borrow_mut().insert(
            support.display().to_string(),
            LiveSession {
                profile_id: "iron".into(),
                pid,
                ready: true,
                process_identity: identity,
            },
        );
        controller.probe.locked.set(true);
        assert!(controller.reconnect("iron", support.clone()).unwrap());
        controller.probe.states.borrow_mut().clear(); // simulated hung socket
        controller.probe.fail_command.set(true);
        controller.tick();
        assert!(controller.close("iron").unwrap());
        assert!(matches!(
            controller.snapshots()[0].state,
            SessionState::Closing { .. }
        ));
        assert!(controller.force_quit_explicit("iron").unwrap());
        assert!(!child.wait().unwrap().success());
        let _ = fs::remove_dir_all(support);
    }

    #[test]
    fn spawn_failure_releases_profile_for_explicit_retry() {
        let mut controller = controller();
        controller.runner.fail = true;
        let support = path("retry");
        assert!(controller.request("iron", support.clone()));
        controller.tick();
        assert!(matches!(
            controller.snapshots()[0].state,
            SessionState::Failed { .. }
        ));
        controller.runner.fail = false;
        assert!(controller.request("iron", support));
        controller.tick();
        assert_eq!(controller.runner.launches.len(), 2);
    }

    #[test]
    fn failed_member_does_not_strand_later_queued_member() {
        let mut controller = controller();
        controller.runner.fail = true;
        let first = path("failed-group-member");
        let second = path("later-group-member");
        assert!(controller.request("first", first));
        assert!(controller.request("second", second));
        controller.tick();
        assert!(matches!(
            controller
                .snapshots()
                .iter()
                .find(|snapshot| snapshot.profile_id == "first")
                .map(|snapshot| &snapshot.state),
            Some(SessionState::Failed { .. })
        ));
        controller.runner.fail = false;
        controller.tick();
        assert_eq!(controller.runner.launches.len(), 2);
        assert_eq!(controller.runner.launches[1].profile_id, "second");
    }

    #[test]
    fn external_profile_lock_prevents_record_overwrite_or_spawn() {
        let mut controller = controller();
        let support = path("external-busy");
        let _ = fs::remove_dir_all(&support);
        fs::create_dir_all(&support).unwrap();
        let original = SessionRecord::pending("iron", &"f".repeat(64));
        write_record(&support, &original).unwrap();
        let held = instance::acquire(&support.join("gwnative.lock"), Duration::ZERO).unwrap();

        assert!(controller.request("iron", support.clone()));
        controller.tick();

        assert!(matches!(
            controller.snapshots()[0].state,
            SessionState::Failed { .. }
        ));
        assert!(controller.runner.launches.is_empty());
        assert_eq!(
            read_record(&support).unwrap().unwrap().nonce,
            original.nonce
        );
        drop(held);
        let _ = fs::remove_dir_all(support);
    }

    #[test]
    fn identity_failure_reaps_owned_child_and_clears_pending_record() {
        let mut controller = controller();
        controller.runner.identity = false;
        let support = path("identity-failure");
        let _ = fs::remove_dir_all(&support);

        assert!(controller.request("iron", support.clone()));
        controller.tick();

        assert_eq!(controller.runner.terminated, vec![1]);
        assert!(read_record(&support).unwrap().is_none());
        assert!(matches!(
            controller.snapshots()[0].state,
            SessionState::Failed { .. }
        ));
        let _ = fs::remove_dir_all(support);
    }

    #[test]
    fn unreaped_child_stays_reserved_and_blocks_retry() {
        let mut controller = controller();
        controller.runner.identity = false;
        controller.runner.cleanup_fail = true;
        let support = path("unreaped-child");
        let _ = fs::remove_dir_all(&support);

        assert!(controller.request("iron", support.clone()));
        controller.tick();

        assert_eq!(controller.runner.terminated, vec![1]);
        assert!(read_record(&support).unwrap().is_some());
        assert_eq!(
            controller.snapshots()[0].state,
            SessionState::AwaitingWindow
        );
        assert!(!controller.request("iron", support.clone()));
        let _ = fs::remove_dir_all(support);
    }

    #[test]
    fn reconnect_reserves_initial_pre_host_pid_identity() {
        let mut controller = controller();
        let support = path("initial-pre-host");
        let pid = std::process::id();
        let identity = process_identity(pid).unwrap();
        let _ = fs::remove_dir_all(&support);
        fs::create_dir_all(&support).unwrap();
        write_record(
            &support,
            &SessionRecord {
                version: VERSION,
                profile_id: "iron".into(),
                nonce: "9".repeat(64),
                pid,
                process_identity: identity,
                handoff_pid: None,
                handoff_identity: None,
            },
        )
        .unwrap();

        assert!(controller.reconnect("iron", support.clone()).unwrap());
        assert_eq!(
            controller.snapshots()[0].state,
            SessionState::AwaitingWindow
        );
        let _ = fs::remove_dir_all(support);
    }

    #[test]
    fn reconnect_reserves_live_successor_when_predecessor_is_dead() {
        let mut controller = controller();
        let support = path("successor-pre-host");
        let successor = std::process::id();
        let successor_identity = process_identity(successor).unwrap();
        let _ = fs::remove_dir_all(&support);
        fs::create_dir_all(&support).unwrap();
        write_record(
            &support,
            &SessionRecord {
                version: VERSION,
                profile_id: "iron".into(),
                nonce: "8".repeat(64),
                pid: 1,
                process_identity: "dead-predecessor".into(),
                handoff_pid: Some(successor),
                handoff_identity: Some(successor_identity),
            },
        )
        .unwrap();

        assert!(controller.reconnect("iron", support.clone()).unwrap());
        assert_eq!(
            controller.snapshots()[0].state,
            SessionState::AwaitingWindow
        );
        let _ = fs::remove_dir_all(support);
    }

    #[test]
    fn reaped_child_and_unlocked_profile_release_play_once() {
        let mut controller = controller();
        let support = path("ended");
        assert!(controller.request("iron", support));
        controller.tick();
        controller.runner.exited = true;
        controller.probe.locked.set(false);
        controller.tick();
        assert!(controller.snapshots().is_empty());
    }

    #[test]
    fn unproven_startup_stays_busy_during_repeated_tick_race() {
        let mut controller = controller();
        let support = path("race");
        assert!(controller.request("iron", support));
        controller.tick();
        for _ in 0..8 {
            controller.tick();
        }
        assert_eq!(controller.runner.launches.len(), 1);
        assert!(!controller.request("iron", path("race")));
    }
}
