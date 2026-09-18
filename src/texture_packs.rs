//! Shared, immutable library for player-owned legacy TexMod packs.
//!
//! This deliberately owns preparation only.  Launcher owns source-folder choice,
//! account selections and child manifest transport; renderer owns WebGL upload.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use flate2::read::DeflateDecoder;
use serde::Deserialize;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::instance;

const MAX_SOURCE_BYTES: usize = 256 * 1024 * 1024;
const MAX_ENTRIES: usize = 1024;
const MAX_ENTRY_BYTES: usize = 64 * 1024 * 1024;
const MAX_EXPANDED_BYTES: usize = 256 * 1024 * 1024;
const MAX_DIMENSION: u32 = 4096;
const MAX_RGBA_BYTES: usize = 64 * 1024 * 1024;
const CONVERSION_VERSION: &str = "tpf-rgba-dxt-v3";
const PREVIOUS_CONVERSION_VERSIONS: [&str; 2] = ["tpf-rgba-dxt-v2", "tpf-rgba-v1"];
const XOR: [u8; 4] = [0xa4, 0x3f, 0xa4, 0x3f];
const PASSWORD: [u8; 42] = [
    0x73, 0x2a, 0x63, 0x7d, 0x5f, 0x0a, 0xa6, 0xbd, 0x7d, 0x65, 0x7e, 0x67, 0x61, 0x2a, 0x7f, 0x7f,
    0x74, 0x61, 0x67, 0x5b, 0x60, 0x70, 0x45, 0x74, 0x5c, 0x22, 0x74, 0x5d, 0x6e, 0x6a, 0x73, 0x41,
    0x77, 0x6e, 0x46, 0x47, 0x77, 0x49, 0x0c, 0x4b, 0x46, 0x6f,
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TexturePackSummary {
    pub id: String,
    pub source: PathBuf,
    pub revision: Option<String>,
    pub status: TexturePackStatus,
    pub error: Option<String>,
    pub mappings: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TexturePackStatus {
    Pending,
    Ready,
    Missing,
    Invalid,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackSelection {
    pub id: String,
    pub revision: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextureSessionManifest {
    pub format: u32,
    pub packs: Vec<TextureSessionPack>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextureSessionPack {
    pub id: String,
    pub revision: String,
    pub entries: Vec<TextureEntry>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionRecord {
    format: u32,
    packs: Vec<SessionPackRef>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionPackRef {
    id: String,
    revision: String,
    asset_path: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextureEntry {
    pub target: u32,
    pub width: u32,
    pub height: u32,
    pub rgba_base64: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compressed: Option<TextureCompressed>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextureCompressed {
    pub mode: String,
    pub levels: Vec<String>,
}

#[derive(Clone)]
struct Revision {
    entries: Arc<Vec<DecodedEntry>>,
    needs_upgrade: bool,
    version: String,
}
#[derive(Clone)]
struct DecodedEntry {
    target: u32,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    compressed: Option<DecodedCompressed>,
}
#[derive(Clone)]
struct DecodedCompressed {
    mode: String,
    levels: Vec<Vec<u8>>,
}
struct DecodedImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    compressed: Option<DecodedCompressed>,
}
#[derive(Clone)]
struct Source {
    path: PathBuf,
    revisions: BTreeMap<String, Revision>,
    current: Option<String>,
    error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryEntry {
    id: String,
    path: PathBuf,
    dev: u64,
    ino: u64,
    current: Option<String>,
    error: Option<String>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Registry {
    format: u32,
    sources: Vec<RegistryEntry>,
}
#[derive(Clone)]
struct Observation {
    len: u64,
    modified: SystemTime,
    ino: u64,
    stable_since: SystemTime,
    prepared: bool,
}
#[derive(Clone)]
pub struct TextureLibrary {
    base: PathBuf,
    sources: BTreeMap<String, Source>,
    registry: Registry,
    observed: BTreeMap<PathBuf, Observation>,
}

impl TextureLibrary {
    /// `base` belongs to GWNative. Source packs remain outside it, untouched.
    pub fn new(base: impl Into<PathBuf>) -> Result<Self, String> {
        let base = base.into();
        fs::create_dir_all(base.join("revisions"))
            .map_err(|e| format!("could not create texture library: {e}"))?;
        fs::create_dir_all(base.join("sessions"))
            .map_err(|e| format!("could not create texture session library: {e}"))?;
        let registry_path = base.join("registry.json");
        let registry: Registry = read_bounded_regular(&registry_path, 4 * 1024 * 1024)
            .ok()
            .and_then(|b| serde_json::from_slice::<Registry>(&b).ok())
            .filter(|r| r.format == 1)
            .unwrap_or(Registry {
                format: 1,
                sources: Vec::new(),
            });
        let mut sources = BTreeMap::new();
        for saved in &registry.sources {
            let mut revisions = BTreeMap::new();
            if let Some(revision) = saved.current.as_ref().filter(|value| valid_revision(value)) {
                let restored = std::iter::once(CONVERSION_VERSION)
                    .chain(PREVIOUS_CONVERSION_VERSIONS)
                    .find_map(|version| {
                        read_revision_version(&base, &saved.id, revision, version)
                            .ok()
                            .map(|entries| (entries, version))
                    });
                if let Some((entries, version)) = restored {
                    revisions.insert(
                        revision.to_owned(),
                        Revision {
                            entries: Arc::new(entries),
                            needs_upgrade: version != CONVERSION_VERSION,
                            version: version.into(),
                        },
                    );
                }
            }
            let current = saved.current.clone().filter(|r| revisions.contains_key(r));
            sources.insert(
                saved.id.clone(),
                Source {
                    path: saved.path.clone(),
                    revisions,
                    current,
                    error: saved.error.clone(),
                },
            );
        }
        Ok(Self {
            base,
            sources,
            registry,
            observed: BTreeMap::new(),
        })
    }

    /// Scan only immediate regular `.tpf` children.  Invalid replacement leaves
    /// current revision usable; removal makes future manifests bypass source.
    pub fn scan(&mut self, root: &Path) -> Result<(), String> {
        self.scan_at(root, SystemTime::now())
    }
    /// Deterministic scanner entry for tests. A source must be observed unchanged
    /// for 500ms before bytes are opened; unchanged prepared sources are skipped.
    pub fn scan_at(&mut self, root: &Path, now: SystemTime) -> Result<(), String> {
        let library_base = self.base.clone();
        let mut present = BTreeSet::new();
        let mut packs = Vec::new();
        for item in fs::read_dir(root).map_err(|e| format!("could not scan texture packs: {e}"))? {
            let item = item.map_err(|e| format!("could not read texture pack directory: {e}"))?;
            let path = item.path();
            if !item.file_type().map_err(|e| e.to_string())?.is_file() || !is_tpf(&path) {
                continue;
            }
            packs.push(path);
        }
        if packs.len() > MAX_ENTRIES {
            return Err(format!(
                "texture pack folder contains more than {MAX_ENTRIES} supported files"
            ));
        }
        packs.sort();
        for path in packs {
            let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
            let id = self.registry_id(&path, metadata.dev(), metadata.ino());
            present.insert(id.clone());
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let unchanged = self.observed.get(&path).is_some_and(|old| {
                old.len == metadata.len() && old.ino == metadata.ino() && old.modified == modified
            });
            if !unchanged {
                self.observed.insert(
                    path.clone(),
                    Observation {
                        len: metadata.len(),
                        modified,
                        ino: metadata.ino(),
                        stable_since: now,
                        prepared: false,
                    },
                );
                self.note_error(id, path, "waiting for stable source copy".into());
                continue;
            }
            let observed = self.observed.get_mut(&path).expect("observation exists");
            if now
                .duration_since(observed.stable_since)
                .unwrap_or_default()
                < Duration::from_millis(500)
            {
                self.note_error(id, path, "waiting for stable source copy".into());
                continue;
            }
            if observed.prepared {
                continue;
            }
            if metadata.len() as usize > MAX_SOURCE_BYTES {
                observed.prepared = true;
                self.note_error(id, path, "TPF size is outside safety limit".into());
                continue;
            }
            let bytes = match read_bounded_regular(&path, MAX_SOURCE_BYTES) {
                Ok(v) => v,
                Err(e) => {
                    observed.prepared = true;
                    self.note_error(id, path, e.to_string());
                    continue;
                }
            };
            if fs::metadata(&path)
                .map(|after| {
                    after.len() != metadata.len()
                        || after.ino() != metadata.ino()
                        || after.modified().unwrap_or(SystemTime::UNIX_EPOCH) != modified
                })
                .unwrap_or(true)
            {
                self.note_error(
                    id,
                    path,
                    "source changed during read; waiting for stable copy".into(),
                );
                continue;
            }
            let revision = digest(&bytes);
            let source = self.sources.entry(id.clone()).or_insert_with(|| Source {
                path: path.clone(),
                revisions: BTreeMap::new(),
                current: None,
                error: None,
            });
            source.path = path;
            if source.revisions.contains_key(&revision) {
                if source.revisions[&revision].needs_upgrade {
                    if let Ok(raw) = read_bounded_regular(
                        &library_base.join("sources").join(format!("{revision}.tpf")),
                        MAX_SOURCE_BYTES,
                    ) {
                        if let Ok(entries) = parse_tpf(&raw) {
                            let maintenance = instance::acquire(
                                &library_base.join("maintenance.lock"),
                                Duration::ZERO,
                            )?;
                            write_revision(&library_base, &id, &revision, &entries)?;
                            source.revisions.insert(
                                revision.clone(),
                                Revision {
                                    entries: Arc::new(entries),
                                    needs_upgrade: false,
                                    version: CONVERSION_VERSION.into(),
                                },
                            );
                            drop(maintenance);
                        }
                    }
                }
                source.current = Some(revision);
                source.error = None;
                if let Some(observed) = self.observed.get_mut(&source.path) {
                    observed.prepared = true;
                }
                continue;
            }
            match parse_tpf(&bytes) {
                Ok(entries) => {
                    let maintenance =
                        instance::acquire(&library_base.join("maintenance.lock"), Duration::ZERO)?;
                    write_revision(&library_base, &id, &revision, &entries)?;
                    write_source(&library_base, &revision, &bytes)?;
                    source.revisions.insert(
                        revision.clone(),
                        Revision {
                            entries: Arc::new(entries),
                            needs_upgrade: false,
                            version: CONVERSION_VERSION.into(),
                        },
                    );
                    source.current = Some(revision);
                    source.error = None;
                    if let Some(observed) = self.observed.get_mut(&source.path) {
                        observed.prepared = true;
                    }
                    drop(maintenance);
                }
                Err(error) => {
                    source.error = Some(error);
                    if let Some(observed) = self.observed.get_mut(&source.path) {
                        observed.prepared = true;
                    }
                }
            }
        }
        for (id, source) in &mut self.sources {
            if !present.contains(id) {
                source.current = None;
                source.error = Some("source file is missing".into());
            }
        }
        self.observed.retain(|path, _| path.exists());
        self.save_registry()?;
        Ok(())
    }
    pub fn snapshot(&self) -> Vec<TexturePackSummary> {
        self.sources
            .iter()
            .map(|(id, s)| TexturePackSummary {
                id: id.clone(),
                source: s.path.clone(),
                revision: s.current.clone(),
                status: if s.error.as_deref() == Some("waiting for stable source copy")
                    && s.current.is_none()
                {
                    TexturePackStatus::Pending
                } else if s.current.is_some() {
                    TexturePackStatus::Ready
                } else if s.error.as_deref() == Some("source file is missing") {
                    TexturePackStatus::Missing
                } else {
                    TexturePackStatus::Invalid
                },
                error: s.error.clone(),
                mappings: s
                    .current
                    .as_ref()
                    .and_then(|r| s.revisions.get(r))
                    .map_or(0, |r| r.entries.len()),
            })
            .collect()
    }
    pub fn is_preparing(&self) -> bool {
        self.observed.values().any(|value| !value.prepared)
    }
    pub fn pending_count(&self) -> usize {
        self.observed
            .values()
            .filter(|value| !value.prepared)
            .count()
    }
    /// Number of mappings each ordered selection loses to earlier packs.
    pub fn conflicts(&self, selections: &[PackSelection]) -> Vec<usize> {
        let mut claimed = BTreeSet::new();
        selections
            .iter()
            .map(|wanted| {
                let Some(source) = self.sources.get(&wanted.id) else {
                    return 0;
                };
                let Some(revision) = wanted
                    .revision
                    .as_ref()
                    .filter(|value| source.revisions.contains_key(*value))
                    .or(source.current.as_ref())
                else {
                    return 0;
                };
                source.revisions.get(revision).map_or(0, |revision| {
                    revision
                        .entries
                        .iter()
                        .filter(|entry| !claimed.insert(entry.target))
                        .count()
                })
            })
            .collect()
    }
    /// Pin exact revisions for one session. Duplicate targets retain earliest
    /// selected pack: later conflicting mappings never reach WebGL.
    #[cfg(test)]
    pub fn pin(&self, selections: &[PackSelection]) -> TextureSessionManifest {
        let mut claimed = BTreeSet::new();
        let mut packs = Vec::new();
        for wanted in selections {
            let Some(source) = self.sources.get(&wanted.id) else {
                continue;
            };
            let Some(current) = source.current.as_ref() else {
                continue;
            };
            let revision = wanted
                .revision
                .as_ref()
                .filter(|r| source.revisions.contains_key(*r))
                .unwrap_or(current);
            let Some(rev) = source.revisions.get(revision) else {
                continue;
            };
            let entries = rev
                .entries
                .iter()
                .filter(|e| claimed.insert(e.target))
                .map(|e| TextureEntry {
                    target: e.target,
                    width: e.width,
                    height: e.height,
                    rgba_base64: base64::Engine::encode(
                        &base64::engine::general_purpose::STANDARD,
                        &e.rgba,
                    ),
                    compressed: e.compressed.as_ref().map(|compressed| TextureCompressed {
                        mode: compressed.mode.clone(),
                        levels: compressed
                            .levels
                            .iter()
                            .map(|level| {
                                base64::Engine::encode(
                                    &base64::engine::general_purpose::STANDARD,
                                    level,
                                )
                            })
                            .collect(),
                    }),
                })
                .collect();
            packs.push(TextureSessionPack {
                id: wanted.id.clone(),
                revision: revision.clone(),
                entries,
            });
        }
        TextureSessionManifest { format: 1, packs }
    }
    /// Write an immutable per-child manifest. Coordinator passes only this
    /// path/token through launch plumbing, never large pixels in arguments.
    pub fn pin_to_file(
        &self,
        session_id: &str,
        selections: &[PackSelection],
    ) -> Result<PathBuf, String> {
        let _maintenance = instance::acquire(&self.base.join("maintenance.lock"), Duration::ZERO)?;
        if !session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            || session_id.is_empty()
        {
            return Err("texture session id is invalid".into());
        }
        let mut packs = Vec::new();
        for wanted in selections {
            let Some(source) = self.sources.get(&wanted.id) else {
                continue;
            };
            let Some(current) = source.current.as_ref() else {
                continue;
            };
            let revision = wanted
                .revision
                .as_ref()
                .filter(|r| source.revisions.contains_key(*r))
                .unwrap_or(current);
            let prepared = source
                .revisions
                .get(revision)
                .expect("current revision is prepared");
            let asset_path =
                revision_path_version(&self.base, &wanted.id, revision, &prepared.version);
            if !asset_path.is_file() {
                if prepared.needs_upgrade {
                    continue;
                }
                write_revision(&self.base, &wanted.id, revision, &prepared.entries)?;
            }
            packs.push(SessionPackRef {
                id: wanted.id.clone(),
                revision: revision.clone(),
                asset_path,
            });
        }
        let bytes =
            serde_json::to_vec(&SessionRecord { format: 1, packs }).map_err(|e| e.to_string())?;
        let stage = self
            .base
            .join("sessions")
            .join(format!(".{session_id}.tmp"));
        let output = self
            .base
            .join("sessions")
            .join(format!("{session_id}.json"));
        fs::write(&stage, bytes).map_err(|e| format!("could not prepare texture session: {e}"))?;
        fs::rename(stage, &output)
            .map_err(|e| format!("could not publish texture session: {e}"))?;
        Ok(output)
    }
    /// Child-only expansion after launch. Reads only immutable derived revision
    /// files named by pinned record; never reopens user-owned TPF source.
    pub fn load_session_manifest(path: &Path) -> Result<TextureSessionManifest, String> {
        let record: SessionRecord = serde_json::from_slice(
            &read_bounded_regular(path, 1024 * 1024)
                .map_err(|e| format!("could not read texture session: {e}"))?,
        )
        .map_err(|_| "texture session is malformed")?;
        if record.format != 1 || record.packs.len() > MAX_ENTRIES {
            return Err("texture session is invalid".into());
        }
        let base = path
            .parent()
            .and_then(Path::parent)
            .ok_or("texture session path is invalid")?;
        let revisions = fs::canonicalize(base.join("revisions"))
            .map_err(|_| "texture revision directory is unavailable")?;
        let mut total = 0usize;
        let mut entries = 0usize;
        let mut packs = Vec::new();
        for reference in record.packs {
            let meta = fs::symlink_metadata(&reference.asset_path)
                .map_err(|_| "pinned texture revision is unavailable")?;
            if !meta.file_type().is_file()
                || meta.file_type().is_symlink()
                || meta.len() as usize > MAX_EXPANDED_BYTES
            {
                return Err("pinned texture revision is unsafe".into());
            }
            let asset = fs::canonicalize(&reference.asset_path)
                .map_err(|_| "pinned texture revision is unavailable")?;
            if !asset.starts_with(&revisions) {
                return Err("pinned texture revision escapes library".into());
            }
            let values: Vec<TextureEntry> = serde_json::from_slice(
                &read_bounded_regular(&asset, MAX_EXPANDED_BYTES)
                    .map_err(|_| "pinned texture revision is unavailable")?,
            )
            .map_err(|_| "pinned texture revision is malformed")?;
            if values.len() > MAX_ENTRIES {
                return Err("pinned texture revision is too large".into());
            }
            for value in &values {
                entries = entries
                    .checked_add(1)
                    .ok_or("pinned texture entry count overflows")?;
                if entries > MAX_ENTRIES {
                    return Err("pinned texture session has too many entries".into());
                }
                if value.width == 0
                    || value.height == 0
                    || value.width > MAX_DIMENSION
                    || value.height > MAX_DIMENSION
                {
                    return Err("pinned texture dimensions are invalid".into());
                }
                let pixels = (value.width as usize)
                    .checked_mul(value.height as usize)
                    .and_then(|value| value.checked_mul(4))
                    .ok_or("pinned texture dimensions overflow")?;
                total = total
                    .checked_add(pixels)
                    .ok_or("pinned texture aggregate overflows")?;
                if pixels > MAX_RGBA_BYTES || total > MAX_RGBA_BYTES {
                    return Err("pinned texture aggregate exceeds safety limit".into());
                }
            }
            for value in &values {
                let expected = value.width as usize * value.height as usize * 4;
                if value.rgba_base64.len() != expected.div_ceil(3) * 4 {
                    return Err("pinned texture encoded pixels have invalid length".into());
                }
                let decoded = base64::Engine::decode(
                    &base64::engine::general_purpose::STANDARD,
                    &value.rgba_base64,
                )
                .map_err(|_| "pinned texture pixels are malformed")?;
                if decoded.len() != expected {
                    return Err("pinned texture dimensions are invalid".into());
                }
                if let Some(compressed) =
                    decode_compressed(value.compressed.clone(), value.width, value.height)?
                {
                    let compressed_bytes = compressed.levels.iter().map(Vec::len).sum::<usize>();
                    total = total
                        .checked_add(compressed_bytes)
                        .ok_or("pinned compressed aggregate overflows")?;
                    if total > MAX_RGBA_BYTES {
                        return Err("pinned compressed aggregate exceeds safety limit".into());
                    }
                }
            }
            packs.push(TextureSessionPack {
                id: reference.id,
                revision: reference.revision,
                entries: values,
            });
        }
        Ok(TextureSessionManifest { format: 1, packs })
    }
    /// Hold this for child lifetime. Maintenance cannot remove session manifest
    /// while any launcher or child process retains shared lease.
    pub fn acquire_session_lease(path: &Path) -> Result<instance::Instance, String> {
        instance::acquire_shared(&session_lock(path), Duration::ZERO)
    }
    /// Caller drops all of its shared leases first. Returns false when another
    /// process still pins session; never removes source or revision assets.
    pub fn release_session(path: &Path) -> Result<bool, String> {
        let lock = session_lock(path);
        let exclusive = match instance::acquire(&lock, Duration::ZERO) {
            Ok(lock) => lock,
            Err(_) => return Ok(false),
        };
        if path.is_file() {
            fs::remove_file(path)
                .map_err(|e| format!("could not remove expired texture session: {e}"))?;
        }
        drop(exclusive);
        Ok(true)
    }
    /// Remove only derived revision files no current source or existing pinned
    /// session names. Any malformed session makes this a no-op for safety.
    pub fn prune_unused_revisions(&mut self) -> Result<usize, String> {
        let maintenance = instance::acquire(&self.base.join("maintenance.lock"), Duration::ZERO)?;
        let mut keep = BTreeSet::new();
        for (id, source) in &self.sources {
            if let Some(revision) = &source.current {
                // A restored current revision may still be v1/v2 while its
                // managed raw source is unavailable for migration. Keep that
                // exact immutable asset; using current conversion version
                // would prune the only selectable last-known-good revision.
                if let Some(prepared) = source.revisions.get(revision) {
                    keep.insert(revision_path_version(
                        &self.base,
                        id,
                        revision,
                        &prepared.version,
                    ));
                }
            }
        }
        let sessions = self.base.join("sessions");
        for entry in fs::read_dir(&sessions).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let record: SessionRecord = serde_json::from_slice(
                &read_bounded_regular(&path, 1024 * 1024).map_err(|e| e.to_string())?,
            )
            .map_err(|_| "cannot prune: session metadata is malformed")?;
            if record.format != 1 || record.packs.len() > MAX_ENTRIES {
                return Err("cannot prune: session metadata is invalid".into());
            }
            for reference in record.packs {
                keep.insert(reference.asset_path);
            }
        }
        let mut removed = 0;
        for directory in fs::read_dir(self.base.join("revisions")).map_err(|e| e.to_string())? {
            let directory = directory.map_err(|e| e.to_string())?;
            if !directory.file_type().map_err(|e| e.to_string())?.is_dir() {
                continue;
            }
            for asset in fs::read_dir(directory.path()).map_err(|e| e.to_string())? {
                let asset = asset.map_err(|e| e.to_string())?;
                let path = asset.path();
                if path.extension().is_some_and(|ext| ext == "json") && !keep.contains(&path) {
                    fs::remove_file(path).map_err(|e| e.to_string())?;
                    removed += 1;
                }
            }
        }
        for source in self.sources.values_mut() {
            source
                .revisions
                .retain(|revision, _| source.current.as_ref() == Some(revision));
        }
        drop(maintenance);
        Ok(removed)
    }
    fn note_error(&mut self, id: String, path: PathBuf, error: String) {
        let source = self.sources.entry(id).or_insert_with(|| Source {
            path: path.clone(),
            revisions: BTreeMap::new(),
            current: None,
            error: None,
        });
        source.path = path;
        source.error = Some(error);
    }
    fn registry_id(&mut self, path: &Path, dev: u64, ino: u64) -> String {
        let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        if let Some(saved) = self
            .registry
            .sources
            .iter_mut()
            .find(|entry| entry.path == path)
        {
            saved.dev = dev;
            saved.ino = ino;
            return saved.id.clone();
        }
        if let Some(saved) = self
            .registry
            .sources
            .iter_mut()
            .find(|entry| entry.dev == dev && entry.ino == ino)
        {
            saved.path = path;
            return saved.id.clone();
        }
        let id = format!("tpf:{}", digest(path.to_string_lossy().as_bytes()));
        self.registry.sources.push(RegistryEntry {
            id: id.clone(),
            path,
            dev,
            ino,
            current: None,
            error: None,
        });
        id
    }
    fn save_registry(&mut self) -> Result<(), String> {
        for entry in &mut self.registry.sources {
            if let Some(source) = self.sources.get(&entry.id) {
                entry.path = source.path.clone();
                entry.current = source.current.clone();
                entry.error = source.error.clone();
            }
        }
        let bytes = serde_json::to_vec(&self.registry).map_err(|e| e.to_string())?;
        let stage = self.base.join(".registry.tmp");
        fs::write(&stage, bytes).map_err(|e| e.to_string())?;
        fs::rename(stage, self.base.join("registry.json")).map_err(|e| e.to_string())
    }
}

fn is_tpf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|x| x.eq_ignore_ascii_case("tpf"))
}
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn write_revision(
    base: &Path,
    _id: &str,
    revision: &str,
    entries: &[DecodedEntry],
) -> Result<(), String> {
    let pack = base.join("revisions").join(CONVERSION_VERSION);
    fs::create_dir_all(&pack).map_err(|e| e.to_string())?;
    let output = pack.join(format!("{revision}.json"));
    if output.exists() {
        return Ok(());
    }
    let values: Vec<TextureEntry> = entries
        .iter()
        .map(|e| TextureEntry {
            target: e.target,
            width: e.width,
            height: e.height,
            rgba_base64: base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                &e.rgba,
            ),
            compressed: e.compressed.as_ref().map(|compressed| TextureCompressed {
                mode: compressed.mode.clone(),
                levels: compressed
                    .levels
                    .iter()
                    .map(|level| {
                        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, level)
                    })
                    .collect(),
            }),
        })
        .collect();
    let bytes = serde_json::to_vec(&values).map_err(|e| e.to_string())?;
    let stage = pack.join(format!(".{revision}.tmp"));
    fs::write(&stage, bytes).map_err(|e| e.to_string())?;
    fs::rename(stage, output).map_err(|e| e.to_string())
}
#[cfg(test)]
fn revision_path(base: &Path, id: &str, revision: &str) -> PathBuf {
    revision_path_version(base, id, revision, CONVERSION_VERSION)
}
fn revision_path_version(base: &Path, id: &str, revision: &str, version: &str) -> PathBuf {
    let _ = id;
    base.join("revisions")
        .join(version)
        .join(format!("{revision}.json"))
}
fn write_source(base: &Path, revision: &str, bytes: &[u8]) -> Result<(), String> {
    let sources = base.join("sources");
    fs::create_dir_all(&sources).map_err(|e| e.to_string())?;
    let output = sources.join(format!("{revision}.tpf"));
    if output.exists() {
        return Ok(());
    }
    let stage = sources.join(format!(".{revision}.tmp"));
    fs::write(&stage, bytes).map_err(|e| e.to_string())?;
    fs::rename(stage, output).map_err(|e| e.to_string())
}
fn session_lock(path: &Path) -> PathBuf {
    path.with_extension("json.lock")
}
fn read_revision_version(
    base: &Path,
    id: &str,
    revision: &str,
    version: &str,
) -> Result<Vec<DecodedEntry>, String> {
    if !valid_revision(revision) {
        return Err("revision id is invalid".into());
    }
    let values: Vec<TextureEntry> = serde_json::from_slice(
        &read_bounded_regular(
            &revision_path_version(base, id, revision, version),
            MAX_EXPANDED_BYTES,
        )
        .map_err(|e| e.to_string())?,
    )
    .map_err(|_| "revision is malformed")?;
    let mut total = 0usize;
    values
        .into_iter()
        .map(|entry| {
            if entry.width == 0
                || entry.height == 0
                || entry.width > MAX_DIMENSION
                || entry.height > MAX_DIMENSION
            {
                return Err("revision dimensions are invalid".into());
            }
            let expected = (entry.width as usize)
                .checked_mul(entry.height as usize)
                .and_then(|v| v.checked_mul(4))
                .ok_or("revision dimensions overflow")?;
            total = total
                .checked_add(expected)
                .ok_or("revision aggregate overflows")?;
            if total > MAX_RGBA_BYTES {
                return Err("revision aggregate exceeds safety limit".into());
            }
            if entry.rgba_base64.len() != expected.div_ceil(3) * 4 {
                return Err("revision encoded pixels have invalid length".into());
            }
            let rgba = base64::Engine::decode(
                &base64::engine::general_purpose::STANDARD,
                entry.rgba_base64,
            )
            .map_err(|_| "revision pixels are malformed")?;
            if rgba.len() != expected {
                return Err("revision dimensions are invalid".into());
            }
            let compressed = decode_compressed(entry.compressed, entry.width, entry.height)?;
            Ok(DecodedEntry {
                target: entry.target,
                width: entry.width,
                height: entry.height,
                rgba,
                compressed,
            })
        })
        .collect()
}
fn decode_compressed(
    value: Option<TextureCompressed>,
    width: u32,
    height: u32,
) -> Result<Option<DecodedCompressed>, String> {
    let Some(value) = value else { return Ok(None) };
    let block = match value.mode.as_str() {
        "DXT1" => 8usize,
        "DXT3" | "DXT5" => 16usize,
        _ => return Err("compressed mode is invalid".into()),
    };
    let maximum_levels = usize::try_from(u32::BITS - width.max(height).leading_zeros())
        .map_err(|_| "compressed levels are invalid")?;
    if value.levels.is_empty() || value.levels.len() > maximum_levels {
        return Err("compressed levels are invalid".into());
    }
    let mut levels = Vec::with_capacity(value.levels.len());
    let mut level_width = width;
    let mut level_height = height;
    let mut total = 0usize;
    for encoded in value.levels {
        let bytes = ((level_width as usize).div_ceil(4))
            .checked_mul((level_height as usize).div_ceil(4))
            .and_then(|n| n.checked_mul(block))
            .ok_or("compressed dimensions overflow")?;
        if encoded.len() != bytes.div_ceil(3) * 4 {
            return Err("compressed encoded bytes are invalid".into());
        }
        total = total
            .checked_add(bytes)
            .ok_or("compressed aggregate overflows")?;
        if total > MAX_RGBA_BYTES {
            return Err("compressed aggregate exceeds safety limit".into());
        }
        let level = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded)
            .map_err(|_| "compressed bytes are malformed")?;
        if level.len() != bytes {
            return Err("compressed bytes are invalid".into());
        }
        levels.push(level);
        level_width = (level_width >> 1).max(1);
        level_height = (level_height >> 1).max(1);
    }
    Ok(Some(DecodedCompressed {
        mode: value.mode,
        levels,
    }))
}
fn read_bounded_regular(path: &Path, maximum: usize) -> Result<Vec<u8>, String> {
    let mut options = fs::OpenOptions::new();
    options.read(true).custom_flags(libc::O_NOFOLLOW);
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > maximum as u64 {
        return Err("file is not a bounded regular file".into());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.by_ref()
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > maximum {
        return Err("file exceeds safety limit".into());
    }
    Ok(bytes)
}
fn valid_revision(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn le16(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2)
        .and_then(|v| v.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or_else(|| "TPF is truncated".into())
}
fn le32(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .and_then(|v| v.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| "TPF is truncated".into())
}
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('\0')
        && !name.starts_with(['/', '\\'])
        && name
            .replace('\\', "/")
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}

#[derive(Clone)]
struct ZipEntry {
    name: String,
    flags: u16,
    method: u16,
    time: u16,
    crc: u32,
    compressed: usize,
    expanded: usize,
    offset: usize,
}
fn parse_tpf(input: &[u8]) -> Result<Vec<DecodedEntry>, String> {
    if input.len() < 22 || input.len() > MAX_SOURCE_BYTES {
        return Err("TPF size is outside safety limit".into());
    }
    let archive: Vec<u8> = input
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ XOR[i & 3])
        .collect();
    if archive.get(..4) != Some(b"PK\x03\x04") {
        return Err("not legacy TexMod TPF".into());
    }
    let end = archive
        .windows(4)
        .rposition(|x| x == b"PK\x05\x06")
        .ok_or("TPF end record is missing")?;
    let count = le16(&archive, end + 10)? as usize;
    if le16(&archive, end + 4)? != 0
        || le16(&archive, end + 6)? != 0
        || le16(&archive, end + 8)? as usize != count
        || end < 22
    {
        return Err("split TPF archives are not supported".into());
    }
    let directory = le32(&archive, end + 16)? as usize;
    let directory_bytes = le32(&archive, end + 12)? as usize;
    if count == 0
        || count > MAX_ENTRIES
        || directory
            .checked_add(directory_bytes)
            .filter(|x| *x <= end)
            .is_none()
    {
        return Err("TPF directory is outside safety limit".into());
    }
    let mut entries = Vec::new();
    let mut names = BTreeSet::new();
    let mut at = directory;
    let mut total = 0usize;
    for _ in 0..count {
        if archive.get(at..at + 4) != Some(b"PK\x01\x02") {
            return Err("TPF directory entry is invalid".into());
        }
        let flags = le16(&archive, at + 8)?;
        let method = le16(&archive, at + 10)?;
        let time = le16(&archive, at + 12)?;
        let crc = le32(&archive, at + 16)?;
        let compressed = le32(&archive, at + 20)? as usize;
        let expanded = le32(&archive, at + 24)? as usize;
        let nl = le16(&archive, at + 28)? as usize;
        let xl = le16(&archive, at + 30)? as usize;
        let cl = le16(&archive, at + 32)? as usize;
        let offset = le32(&archive, at + 42)? as usize;
        let next = at
            .checked_add(46 + nl + xl + cl)
            .ok_or("TPF directory overflows")?;
        if next > archive.len()
            || compressed > MAX_ENTRY_BYTES
            || expanded > MAX_ENTRY_BYTES
            || flags & 1 == 0
            || flags & 0x40 != 0
            || !matches!(method, 0 | 8)
        {
            return Err("unsupported or oversized TPF entry".into());
        }
        total = total
            .checked_add(expanded)
            .ok_or("TPF expansion overflows")?;
        if total > MAX_EXPANDED_BYTES {
            return Err("TPF expansion exceeds safety limit".into());
        }
        let name = std::str::from_utf8(&archive[at + 46..at + 46 + nl])
            .map_err(|_| "TPF name is not UTF-8")?
            .to_owned();
        if !safe_name(&name) {
            return Err("TPF contains unsafe path".into());
        }
        if !names.insert(name.to_ascii_lowercase()) {
            return Err("TPF contains duplicate entry names".into());
        }
        entries.push(ZipEntry {
            name,
            flags,
            method,
            time,
            crc,
            compressed,
            expanded,
            offset,
        });
        at = next;
    }
    let definitions: Vec<_> = entries
        .iter()
        .filter(|e| e.name.eq_ignore_ascii_case("texmod.def"))
        .collect();
    if definitions.len() != 1 {
        return Err("TPF must contain exactly one texmod.def".into());
    }
    let def = definitions[0];
    let definitions = String::from_utf8(read_zip(&archive, def)?)
        .map_err(|_| "texmod.def is not valid UTF-8")?
        .trim_end_matches('\0')
        .to_owned();
    let mut images = BTreeMap::new();
    for e in &entries {
        images.entry(e.name.to_ascii_lowercase()).or_insert(e);
    }
    let mut targets = BTreeMap::new();
    for line in definitions.lines().map(str::trim).filter(|x| !x.is_empty()) {
        let (hash, name) = line.split_once('|').ok_or("invalid texmod.def mapping")?;
        let hex = hash
            .strip_prefix("0x")
            .or_else(|| hash.strip_prefix("0X"))
            .ok_or("invalid TexMod hash")?;
        if hex.len() > 8 {
            return Err("unsupported TexMod hash width".into());
        }
        let target = u32::from_str_radix(hex, 16).map_err(|_| "invalid TexMod hash")?;
        let name = name.trim();
        if !safe_name(name) {
            return Err("unsafe texmod.def image path".into());
        }
        let image = images
            .get(&name.to_ascii_lowercase())
            .ok_or("mapped image missing")?;
        let previous = targets.insert(target, *image);
        if let Some(old) = previous {
            if !old.name.eq_ignore_ascii_case(&image.name) {
                return Err("conflicting target mapping".into());
            }
        }
    }
    if targets.is_empty() {
        return Err("texmod.def has no mappings".into());
    }
    let mut decoded = Vec::new();
    let mut total_rgba = 0usize;
    for (target, entry) in targets {
        let image = decode_image(&entry.name, &read_zip(&archive, entry)?)?;
        total_rgba = total_rgba
            .checked_add(image.rgba.len())
            .ok_or("decoded texture size overflows")?;
        if total_rgba > MAX_EXPANDED_BYTES {
            return Err("decoded texture pack exceeds safety limit".into());
        }
        decoded.push(DecodedEntry {
            target,
            width: image.width,
            height: image.height,
            rgba: image.rgba,
            compressed: image.compressed,
        });
    }
    Ok(decoded)
}

fn decode_image(name: &str, input: &[u8]) -> Result<DecodedImage, String> {
    if name.to_ascii_lowercase().ends_with(".dds") {
        return decode_dds_image(input);
    }
    if name.to_ascii_lowercase().ends_with(".png") {
        let (width, height, rgba) = decode_png(input)?;
        return Ok(DecodedImage {
            width,
            height,
            rgba,
            compressed: None,
        });
    }
    Err("only DDS and PNG images are supported".into())
}

fn decode_png(input: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    if input.is_empty() || input.len() > MAX_ENTRY_BYTES {
        return Err("PNG size is outside safety limit".into());
    }
    let mut decoder = png::Decoder::new(std::io::Cursor::new(input));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| "PNG header is invalid")?;
    let info = reader.info();
    let width = info.width;
    let height = info.height;
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .and_then(|v| v.checked_mul(4))
        .ok_or("PNG dimensions overflow")?;
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || pixels > MAX_RGBA_BYTES
    {
        return Err("PNG dimensions outside supported range".into());
    }
    let mut raw = vec![0; reader.output_buffer_size()];
    if raw.len() > MAX_RGBA_BYTES {
        return Err("PNG decoded bytes exceed safety limit".into());
    }
    let output = reader
        .next_frame(&mut raw)
        .map_err(|_| "PNG pixels are invalid")?;
    let raw = &raw[..output.buffer_size()];
    let mut rgba = Vec::with_capacity(pixels);
    match output.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(raw),
        png::ColorType::Rgb => {
            for pixel in raw.chunks_exact(3) {
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for pixel in raw.chunks_exact(2) {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
        }
        png::ColorType::Grayscale => {
            for value in raw {
                rgba.extend_from_slice(&[*value, *value, *value, 255]);
            }
        }
        png::ColorType::Indexed => return Err("PNG indexed colour did not expand".into()),
    }
    if rgba.len() != pixels {
        return Err("PNG output dimensions are invalid".into());
    }
    Ok((width, height, rgba))
}
fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in bytes {
        c ^= b as u32;
        for _ in 0..8 {
            c = (c >> 1) ^ if c & 1 != 0 { 0xedb8_8320 } else { 0 };
        }
    }
    !c
}
struct ZipCrypto {
    a: u32,
    b: u32,
    c: u32,
}
impl ZipCrypto {
    fn new() -> Self {
        let mut x = Self {
            a: 0x1234_5678,
            b: 0x2345_6789,
            c: 0x3456_7890,
        };
        for b in PASSWORD {
            x.update(b)
        }
        x
    }
    fn update(&mut self, b: u8) {
        self.a = crc32_update(self.a, b);
        self.b = (self.b.wrapping_add(self.a & 255).wrapping_mul(134775813)).wrapping_add(1);
        self.c = crc32_update(self.c, (self.b >> 24) as u8);
    }
    fn decrypt(&mut self, b: u8) -> u8 {
        let t = self.c | 2;
        let p = b ^ (((t.wrapping_mul(t ^ 1)) >> 8) as u8);
        self.update(p);
        p
    }
}
fn crc32_update(mut c: u32, b: u8) -> u32 {
    c ^= b as u32;
    for _ in 0..8 {
        c = (c >> 1) ^ if c & 1 != 0 { 0xedb8_8320 } else { 0 };
    }
    c
}
fn read_zip(archive: &[u8], entry: &ZipEntry) -> Result<Vec<u8>, String> {
    let at = entry.offset;
    if archive.get(at..at + 4) != Some(b"PK\x03\x04") {
        return Err("TPF local entry invalid".into());
    };
    let nl = le16(archive, at + 26)? as usize;
    let xl = le16(archive, at + 28)? as usize;
    let begin = at
        .checked_add(30 + nl + xl)
        .ok_or("TPF local entry overflows")?;
    let end = begin
        .checked_add(entry.compressed)
        .filter(|x| *x <= archive.len())
        .ok_or("TPF entry truncated")?;
    if entry.compressed < 12 {
        return Err("TPF encrypted entry truncated".into());
    };
    let mut crypto = ZipCrypto::new();
    let decrypted: Vec<u8> = archive[begin..end]
        .iter()
        .map(|b| crypto.decrypt(*b))
        .collect();
    let check = if entry.flags & 8 != 0 {
        (entry.time >> 8) as u8
    } else {
        (entry.crc >> 24) as u8
    };
    if decrypted[11] != check {
        return Err("TPF password check failed".into());
    };
    let mut out = Vec::with_capacity(entry.expanded);
    if entry.method == 0 {
        out.extend_from_slice(&decrypted[12..])
    } else {
        let mut d = DeflateDecoder::new(&decrypted[12..]);
        d.by_ref()
            .take((entry.expanded + 1) as u64)
            .read_to_end(&mut out)
            .map_err(|_| "TPF entry cannot decompress")?;
    };
    if out.len() != entry.expanded || crc32(&out) != entry.crc {
        return Err("TPF entry checksum failed".into());
    };
    Ok(out)
}
#[cfg(test)]
fn decode_dds(input: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let image = decode_dds_image(input)?;
    Ok((image.width, image.height, image.rgba))
}
fn decode_dds_image(input: &[u8]) -> Result<DecodedImage, String> {
    if input.len() < 128 || input.get(..4) != Some(b"DDS ") {
        return Err("only DDS images are currently supported".into());
    };
    let h = le32(input, 12)?;
    let w = le32(input, 16)?;
    let bytes = (w as usize)
        .checked_mul(h as usize)
        .and_then(|x| x.checked_mul(4))
        .ok_or("DDS dimensions overflow")?;
    if w == 0 || h == 0 || w > MAX_DIMENSION || h > MAX_DIMENSION || bytes > MAX_RGBA_BYTES {
        return Err("DDS dimensions outside supported range".into());
    };
    let flags = le32(input, 80)?;
    if flags & 4 != 0 {
        let cc = &input[84..88];
        let (mode, name) = match cc {
            b"DXT1" => (1, "DXT1"),
            b"DXT3" => (3, "DXT3"),
            b"DXT5" => (5, "DXT5"),
            _ => return Err("unsupported DDS compression".into()),
        };
        let length = ((w as usize + 3) / 4)
            .checked_mul((h as usize + 3) / 4)
            .and_then(|value| value.checked_mul(if mode == 1 { 8 } else { 16 }))
            .ok_or("DDS compressed dimensions overflow")?;
        let blocks = input
            .get(128..128 + length)
            .ok_or("DDS top mip is truncated")?;
        let (_, _, rgba) = decode_dxt(blocks, w, h, mode)?;
        return Ok(DecodedImage {
            width: w,
            height: h,
            rgba,
            compressed: Some(DecodedCompressed {
                mode: name.into(),
                levels: vec![blocks.to_vec()],
            }),
        });
    };
    let bits = le32(input, 88)?;
    if !matches!(bits, 8 | 16 | 32) {
        return Err("unsupported DDS pixel format".into());
    };
    let bpp = (bits / 8) as usize;
    let pitch = le32(input, 20)? as usize;
    let tight = w as usize * bpp;
    let pitch = if le32(input, 8)? & 8 != 0 && pitch >= tight {
        pitch
    } else {
        tight
    };
    if input.len() < 128 + pitch * h as usize {
        return Err("DDS data truncated".into());
    };
    let masks = [
        le32(input, 92)?,
        le32(input, 96)?,
        le32(input, 100)?,
        le32(input, 104)?,
    ];
    let allowed = if bits == 32 {
        u32::MAX
    } else {
        (1u32 << bits) - 1
    };
    let mut used = 0u32;
    for (index, mask) in masks.iter().enumerate() {
        if index < 3 && *mask == 0 {
            return Err("DDS RGB masks are missing".into());
        }
        if *mask & !allowed != 0 || *mask & used != 0 || (*mask != 0 && !contiguous_mask(*mask)) {
            return Err("DDS channel masks are unsupported".into());
        }
        used |= *mask;
    }
    let mut rgba = vec![0; bytes];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let at = 128 + y * pitch + x * bpp;
            let v = match bpp {
                1 => input[at] as u32,
                2 => le16(input, at)? as u32,
                4 => le32(input, at)?,
                _ => unreachable!(),
            };
            let o = (y * w as usize + x) * 4;
            for c in 0..4 {
                rgba[o + c] = if masks[c] == 0 {
                    255
                } else {
                    channel(v, masks[c])
                };
            }
        }
    }
    Ok(DecodedImage {
        width: w,
        height: h,
        rgba,
        compressed: None,
    })
}
fn rgb565(v: u16) -> [u8; 3] {
    let r = (v >> 11) & 31;
    let g = (v >> 5) & 63;
    let b = v & 31;
    [
        ((r << 3) | (r >> 2)) as u8,
        ((g << 2) | (g >> 4)) as u8,
        ((b << 3) | (b >> 2)) as u8,
    ]
}
fn decode_dxt(src: &[u8], w: u32, h: u32, mode: u8) -> Result<(u32, u32, Vec<u8>), String> {
    let block = if mode == 1 { 8 } else { 16 };
    let bw = (w as usize + 3) / 4;
    let bh = (h as usize + 3) / 4;
    if src.len() < bw * bh * block {
        return Err("DDS top mip is truncated".into());
    };
    let mut out = vec![0; w as usize * h as usize * 4];
    for by in 0..bh {
        for bx in 0..bw {
            let at = (by * bw + bx) * block;
            let co = at + if mode == 1 { 0 } else { 8 };
            let a = le16(src, co)?;
            let b = le16(src, co + 2)?;
            let c0 = rgb565(a);
            let c1 = rgb565(b);
            let mut c = [[0; 4]; 4];
            c[0] = [c0[0], c0[1], c0[2], 255];
            c[1] = [c1[0], c1[1], c1[2], 255];
            if mode == 1 && a <= b {
                c[2] = [
                    ((c0[0] as u16 + c1[0] as u16) / 2) as u8,
                    ((c0[1] as u16 + c1[1] as u16) / 2) as u8,
                    ((c0[2] as u16 + c1[2] as u16) / 2) as u8,
                    255,
                ];
                c[3] = [0, 0, 0, 0]
            } else {
                for i in 0..3 {
                    c[2][i] = ((2 * c0[i] as u16 + c1[i] as u16) / 3) as u8;
                    c[3][i] = ((c0[i] as u16 + 2 * c1[i] as u16) / 3) as u8;
                }
                c[2][3] = 255;
                c[3][3] = 255;
            }
            let bits = le32(src, co + 4)?;
            for p in 0..16 {
                let x = bx * 4 + p % 4;
                let y = by * 4 + p / 4;
                if x >= w as usize || y >= h as usize {
                    continue;
                };
                let mut px = c[((bits >> (2 * p)) & 3) as usize];
                if mode == 3 {
                    px[3] = ((src[at + p / 2] >> ((p % 2) * 4)) & 15) * 17
                } else if mode == 5 {
                    let a0 = src[at];
                    let a1 = src[at + 1];
                    let mut table = [0u8; 8];
                    table[0] = a0;
                    table[1] = a1;
                    if a0 > a1 {
                        for i in 1..7 {
                            table[i + 1] =
                                (((7 - i) as u16 * a0 as u16 + i as u16 * a1 as u16) / 7) as u8
                        }
                    } else {
                        for i in 1..5 {
                            table[i + 1] =
                                (((5 - i) as u16 * a0 as u16 + i as u16 * a1 as u16) / 5) as u8
                        }
                        table[6] = 0;
                        table[7] = 255
                    }
                    let mut ab = 0u64;
                    for i in 0..6 {
                        ab |= (src[at + 2 + i] as u64) << (8 * i)
                    }
                    px[3] = table[((ab >> (3 * p)) & 7) as usize]
                }
                let o = (y * w as usize + x) * 4;
                out[o..o + 4].copy_from_slice(&px)
            }
        }
    }
    Ok((w, h, out))
}
fn channel(value: u32, mask: u32) -> u8 {
    let shift = mask.trailing_zeros();
    let max = mask >> shift;
    let scaled = (((value & mask) >> shift) as u64) * 255 / max as u64;
    scaled as u8
}
fn contiguous_mask(mask: u32) -> bool {
    let shifted = mask >> mask.trailing_zeros();
    shifted & shifted.wrapping_add(1) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    fn temp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "gwnative-tpf-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir(&p).unwrap();
        p
    }
    #[test]
    fn discovery_is_case_insensitive_and_missing_bypasses() {
        let d = temp();
        let p = d.join("UI.TpF");
        fs::write(&p, b"bad").unwrap();
        let mut l = TextureLibrary::new(d.join("library")).unwrap();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        l.scan_at(&d, now).unwrap();
        assert_eq!(l.snapshot()[0].status, TexturePackStatus::Pending);
        l.scan_at(&d, now + Duration::from_millis(500)).unwrap();
        let id = l.snapshot()[0].id.clone();
        assert_eq!(l.snapshot()[0].status, TexturePackStatus::Invalid);
        fs::remove_file(&p).unwrap();
        l.scan(&d).unwrap();
        assert_eq!(l.snapshot()[0].status, TexturePackStatus::Missing);
        assert!(
            l.pin(&[PackSelection { id, revision: None }])
                .packs
                .is_empty()
        );
        fs::remove_dir_all(d).unwrap()
    }
    #[test]
    fn dds_refuses_truncation_and_accepts_bgra() {
        let mut b = vec![0; 132];
        b[..4].copy_from_slice(b"DDS ");
        b[4..8].copy_from_slice(&124u32.to_le_bytes());
        b[12..16].copy_from_slice(&1u32.to_le_bytes());
        b[16..20].copy_from_slice(&1u32.to_le_bytes());
        b[80..84].copy_from_slice(&0x40u32.to_le_bytes());
        b[88..92].copy_from_slice(&32u32.to_le_bytes());
        b[92..96].copy_from_slice(&0x00ff0000u32.to_le_bytes());
        b[96..100].copy_from_slice(&0x0000ff00u32.to_le_bytes());
        b[100..104].copy_from_slice(&0x000000ffu32.to_le_bytes());
        b[104..108].copy_from_slice(&0xff000000u32.to_le_bytes());
        b[128..132].copy_from_slice(&[1, 2, 3, 4]);
        assert_eq!(decode_dds(&b).unwrap().2, &[3, 2, 1, 4]);
        assert!(decode_dds(&b[..130]).is_err())
    }
    #[test]
    fn old_published_clone_republishes_pruned_revision_before_pinning() {
        let d = temp();
        let mut library = TextureLibrary::new(d.join("library")).unwrap();
        let id = "tpf:test".to_owned();
        let revision = "old".to_owned();
        let entry = DecodedEntry {
            target: 7,
            width: 1,
            height: 1,
            rgba: vec![1, 2, 3, 4],
            compressed: None,
        };
        write_revision(&library.base, &id, &revision, std::slice::from_ref(&entry)).unwrap();
        library.sources.insert(
            id.clone(),
            Source {
                path: d.join("source.tpf"),
                revisions: BTreeMap::from([(
                    revision.clone(),
                    Revision {
                        entries: Arc::new(vec![entry]),
                        needs_upgrade: false,
                        version: CONVERSION_VERSION.into(),
                    },
                )]),
                current: Some(revision.clone()),
                error: None,
            },
        );
        let published = library.clone();
        fs::remove_file(revision_path(&published.base, &id, &revision)).unwrap();
        let manifest = published
            .pin_to_file(
                "session",
                &[PackSelection {
                    id: id.clone(),
                    revision: Some(revision.clone()),
                }],
            )
            .unwrap();
        assert!(revision_path(&published.base, &id, &revision).is_file());
        let lease = TextureLibrary::acquire_session_lease(&manifest).unwrap();
        assert!(!TextureLibrary::release_session(&manifest).unwrap());
        drop(lease);
        assert!(TextureLibrary::release_session(&manifest).unwrap());
        fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn legacy_revision_stays_on_its_immutable_asset_until_raw_recompile() {
        let d = temp();
        let base = d.join("library");
        let revision = digest(b"v1 revision");
        let id = "tpf:legacy".to_owned();
        let v1 = revision_path_version(&base, &id, &revision, "tpf-rgba-v1");
        fs::create_dir_all(v1.parent().unwrap()).unwrap();
        fs::write(
            &v1,
            serde_json::to_vec(&vec![TextureEntry {
                target: 7,
                width: 1,
                height: 1,
                rgba_base64: "AQIDBA==".into(),
                compressed: None,
            }])
            .unwrap(),
        )
        .unwrap();
        fs::create_dir_all(base.join("sessions")).unwrap();
        fs::write(
            base.join("sessions/old.json"),
            serde_json::to_vec(&SessionRecord {
                format: 1,
                packs: vec![SessionPackRef {
                    id: id.clone(),
                    revision: revision.clone(),
                    asset_path: v1.clone(),
                }],
            })
            .unwrap(),
        )
        .unwrap();
        fs::write(
            base.join("registry.json"),
            serde_json::to_vec(&Registry {
                format: 1,
                sources: vec![RegistryEntry {
                    id: id.clone(),
                    path: d.join("missing.tpf"),
                    dev: 0,
                    ino: 0,
                    current: Some(revision.clone()),
                    error: None,
                }],
            })
            .unwrap(),
        )
        .unwrap();
        let mut library = TextureLibrary::new(&base).unwrap();
        assert!(library.sources[&id].revisions[&revision].needs_upgrade);
        // No session lease remains: current legacy revision itself must retain
        // its v1 asset through maintenance, or last-good recovery cannot pin.
        fs::remove_file(base.join("sessions/old.json")).unwrap();
        assert_eq!(library.prune_unused_revisions().unwrap(), 0);
        assert!(v1.is_file());
        let fresh = library
            .pin_to_file(
                "new",
                &[PackSelection {
                    id: id.clone(),
                    revision: None,
                }],
            )
            .unwrap();
        assert!(!revision_path(&base, &id, &revision).exists());
        assert!(v1.is_file());
        assert!(TextureLibrary::load_session_manifest(&fresh).is_ok());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    #[ignore = "ignored player-owned sample is not a CI fixture"]
    fn supplied_minimalus_parses_when_present() {
        let sample =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("texturepacks/Minimalus.UI.v3.2.tpf");
        let entries = parse_tpf(&fs::read(sample).unwrap()).unwrap();
        eprintln!(
            "Minimalus.UI.v3.2.tpf: {} mappings; largest {}x{}",
            entries.len(),
            entries.iter().map(|entry| entry.width).max().unwrap(),
            entries.iter().map(|entry| entry.height).max().unwrap()
        );
        assert!(entries.len() > 100, "expected substantial supplied UI pack");
        assert!(entries.iter().all(|entry| entry.width > 0
            && entry.height > 0
            && entry.rgba.len() == entry.width as usize * entry.height as usize * 4));
    }
}

#[cfg(test)]
#[path = "texture_packs_tests.rs"]
mod regression_tests;
