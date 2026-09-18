//! Background preparation with immutable published library views. UI never waits
//! for source IO or decoding; launches pin the last completely prepared view.
use crate::texture_packs::{PackSelection, TextureLibrary};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

struct Published {
    library: Option<Arc<TextureLibrary>>,
    folder: PathBuf,
    message: String,
}

enum Command {
    Refresh,
    Folder(PathBuf),
}

pub struct LibraryWorker {
    base: PathBuf,
    published: Arc<Mutex<Published>>,
    commands: mpsc::Sender<Command>,
    leases: std::collections::HashMap<String, (PathBuf, crate::instance::Instance)>,
}

impl LibraryWorker {
    pub fn start(base: &Path) -> Self {
        let choice = std::fs::read(base.join("launcher/texture-folder.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<PathBuf>(&bytes).ok());
        let folder = crate::paths::texture_source_dir(base, choice.as_deref());
        let published = Arc::new(Mutex::new(Published {
            library: None,
            folder: folder.clone(),
            message: "Checking texture folder…".into(),
        }));
        let (commands, receiver) = mpsc::channel();
        let shared = published.clone();
        let managed = base.join("texture-library");
        std::thread::spawn(move || {
            let mut folder = folder;
            let mut library = match TextureLibrary::new(managed) {
                Ok(library) => library,
                Err(error) => {
                    if let Ok(mut state) = shared.lock() {
                        state.message = error;
                    }
                    return;
                }
            };
            loop {
                let result = std::fs::create_dir_all(&folder)
                    .map_err(|e| e.to_string())
                    .and_then(|_| library.scan(&folder));
                if let Err(error) = library.prune_unused_revisions() {
                    note!("[textures] cache cleanup deferred: {error}");
                }
                if let Ok(mut state) = shared.lock() {
                    state.folder = folder.clone();
                    state.message = result.err().unwrap_or_else(|| if library.is_preparing() { format!("Waiting for {} texture pack copies to finish…", library.pending_count()) } else { "Choose packs in each Account’s launch settings. Changes apply next launch; unmatched textures keep their original appearance.".into() });
                    state.library = Some(Arc::new(library.clone()));
                }
                match receiver.recv_timeout(Duration::from_secs(1)) {
                    Ok(Command::Folder(next)) => folder = next,
                    Ok(Command::Refresh) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                // Combine bursts; final folder choice wins and each loop rescans,
                // so dropped OS events cannot leave stale discovery indefinitely.
                while let Ok(command) = receiver.try_recv() {
                    if let Command::Folder(next) = command {
                        folder = next;
                    }
                }
            }
        });
        Self {
            base: base.to_owned(),
            published,
            commands,
            leases: Default::default(),
        }
    }

    pub fn ready(&self) -> bool {
        self.published
            .lock()
            .map(|state| {
                state
                    .library
                    .as_ref()
                    .is_some_and(|library| !library.is_preparing())
                    || (state.library.is_none() && state.message != "Checking texture folder…")
            })
            .unwrap_or(true)
    }

    pub fn snapshot(&self) -> Value {
        let Ok(state) = self.published.lock() else {
            return json!({"message":"Texture library unavailable","packs":[]});
        };
        let packs: Vec<Value> = state
            .library
            .as_ref()
            .map(|library| {
                library
                    .snapshot()
                    .into_iter()
                    .map(|pack| {
                        let name = pack
                            .source
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        let mut value = serde_json::to_value(pack).unwrap_or_default();
                        value["name"] = json!(name);
                        value
                    })
                    .collect()
            })
            .unwrap_or_default();
        json!({"folder":state.folder,"message":state.message,"packs":packs})
    }

    pub fn warnings(&self, ids: &[String]) -> Vec<String> {
        let library = self
            .published
            .lock()
            .ok()
            .and_then(|state| state.library.clone());
        let packs = library
            .map(|library| library.snapshot())
            .unwrap_or_default();
        ids.iter()
            .filter_map(|id| match packs.iter().find(|pack| &pack.id == id) {
                None => {
                    Some("Selected texture pack unavailable; original textures retained".into())
                }
                Some(pack) => pack.error.as_ref().map(|error| {
                    format!(
                        "{}: {error}{}",
                        pack.source
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy(),
                        if pack.revision.is_some() {
                            "; last prepared revision retained"
                        } else {
                            "; original textures retained"
                        }
                    )
                }),
            })
            .collect()
    }

    pub fn conflicts(&self, ids: &[String]) -> Vec<usize> {
        let library = self
            .published
            .lock()
            .ok()
            .and_then(|state| state.library.clone());
        library
            .map(|library| {
                library.conflicts(
                    &ids.iter()
                        .map(|id| PackSelection {
                            id: id.clone(),
                            revision: None,
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .unwrap_or_else(|| vec![0; ids.len()])
    }

    pub fn folder(&self) -> Result<PathBuf, String> {
        self.published
            .lock()
            .map(|s| s.folder.clone())
            .map_err(|_| "Texture library unavailable".into())
    }

    pub fn refresh(&self) -> Result<(), String> {
        self.commands
            .send(Command::Refresh)
            .map_err(|_| "Texture scanner is unavailable".into())
    }

    pub fn change_folder(&self, folder: PathBuf) -> Result<(), String> {
        if !folder.is_absolute() || !folder.is_dir() {
            return Err("Choose an existing texture folder".into());
        }
        let path = self.base.join("launcher/texture-folder.json");
        let stage = path.with_extension("json.tmp");
        std::fs::write(
            &stage,
            serde_json::to_vec(&folder).map_err(|e| e.to_string())?,
        )
        .and_then(|_| std::fs::rename(stage, path))
        .map_err(|e| e.to_string())?;
        self.commands
            .send(Command::Folder(folder))
            .map_err(|_| "Texture scanner is unavailable".into())
    }

    pub fn release_finished(&mut self, snapshots: &[crate::launcher_sessions::SessionSnapshot]) {
        let finished: Vec<String> = self
            .leases
            .keys()
            .filter(|id| {
                !snapshots.iter().any(|s| {
                    &s.profile_id == *id
                        && !matches!(
                            s.state,
                            crate::launcher_sessions::SessionState::Failed { .. }
                        )
                })
            })
            .cloned()
            .collect();
        for id in finished {
            let support = if id == "default" {
                self.base.clone()
            } else {
                self.base.join("profiles").join(&id)
            };
            let Ok(_profile_guard) =
                crate::instance::acquire(&support.join("gwnative.lock"), Duration::ZERO)
            else {
                continue;
            };
            if let Some((path, lease)) = self.leases.remove(&id) {
                drop(lease);
                let _ = TextureLibrary::release_session(&path);
                let _ = self.commands.send(Command::Refresh);
            }
        }
    }

    pub fn pin(&mut self, profile_id: &str, ids: &[String]) -> Result<Option<PathBuf>, String> {
        if ids.is_empty() {
            return Ok(None);
        }
        let library = self
            .published
            .lock()
            .map_err(|_| "Texture library unavailable")?
            .library
            .clone()
            .ok_or("Texture packs still preparing")?;
        let session_id = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        );
        let path = library.pin_to_file(
            &session_id,
            &ids.iter()
                .map(|id| PackSelection {
                    id: id.clone(),
                    revision: None,
                })
                .collect::<Vec<_>>(),
        )?;
        let lease = TextureLibrary::acquire_session_lease(&path)?;
        self.replace_lease(profile_id, path.clone(), lease);
        Ok(Some(path))
    }

    /// A retry supersedes launcher's old manifest, but never a live child's
    /// shared lease. Releasing after dropping launcher lease removes only a
    /// manifest no process still owns.
    fn replace_lease(&mut self, profile_id: &str, path: PathBuf, lease: crate::instance::Instance) {
        if let Some((old_path, old_lease)) = self.leases.remove(profile_id) {
            drop(old_lease);
            if let Err(error) = TextureLibrary::release_session(&old_path) {
                note!("[textures] replaced session cleanup deferred: {error}");
            }
        }
        self.leases.insert(profile_id.to_owned(), (path, lease));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker(base: PathBuf) -> LibraryWorker {
        let (commands, _receiver) = mpsc::channel();
        LibraryWorker {
            published: Arc::new(Mutex::new(Published {
                library: None,
                folder: base.clone(),
                message: String::new(),
            })),
            base,
            commands,
            leases: Default::default(),
        }
    }

    fn manifest(library: &TextureLibrary, id: &str) -> PathBuf {
        library.pin_to_file(id, &[]).unwrap()
    }

    #[test]
    fn retry_releases_replaced_launcher_only_manifest() {
        let temp = crate::scratch::TempDir::new("texture-retry-releases-old");
        let library = TextureLibrary::new(temp.0.join("texture-library")).unwrap();
        let old = manifest(&library, "old");
        let new = manifest(&library, "new");
        let mut worker = worker(temp.0.clone());
        worker.replace_lease(
            "profile",
            old.clone(),
            TextureLibrary::acquire_session_lease(&old).unwrap(),
        );
        worker.replace_lease(
            "profile",
            new.clone(),
            TextureLibrary::acquire_session_lease(&new).unwrap(),
        );
        assert!(
            !old.exists(),
            "retry must release old launcher-only manifest"
        );
        assert!(new.exists());
    }

    #[test]
    fn retry_retains_replaced_manifest_while_child_holds_shared_lease() {
        let temp = crate::scratch::TempDir::new("texture-retry-retains-child");
        let library = TextureLibrary::new(temp.0.join("texture-library")).unwrap();
        let old = manifest(&library, "old");
        let new = manifest(&library, "new");
        let child = TextureLibrary::acquire_session_lease(&old).unwrap();
        let mut worker = worker(temp.0.clone());
        worker.replace_lease(
            "profile",
            old.clone(),
            TextureLibrary::acquire_session_lease(&old).unwrap(),
        );
        worker.replace_lease(
            "profile",
            new.clone(),
            TextureLibrary::acquire_session_lease(&new).unwrap(),
        );
        assert!(
            old.exists(),
            "child lease prevents retry from deleting old manifest"
        );
        drop(child);
        assert!(TextureLibrary::release_session(&old).unwrap());
    }
}
