//! Narrow host-to-page contract for preferred-character startup.
//!
//! Registration belongs to launcher integration. This module deliberately
//! describes no offsets or UI input fallback: the host's exact-artifact
//! transform proof must grant every listed operation before page code can act.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

const OBSERVATIONS_FILE: &str = "character-observations.json";
const OBSERVATIONS_FORMAT: u32 = 1;
const MAX_OBSERVED_NAMES: usize = 64;
const MAX_OBSERVED_NAME_UTF16: usize = 40;
const MAX_OBSERVATION_BYTES: usize = 16 * 1024;

/// Serialize profile-local replacement so simultaneous page retries cannot
/// publish a partial JSON document. The complete roster replaces its prior
/// observation; it is autocomplete history, never a character identity.
static OBSERVATION_WRITE: OnceLock<Mutex<()>> = OnceLock::new();
static OBSERVATION_TEMPORARY: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
pub const OPERATIONS: [&str; 6] = [
    "observeReady",
    "readRoster",
    "selectCharacter",
    "readSelected",
    "enterCharacter",
    "observeEntered",
];

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Capability {
    pub supported: bool,
    pub runtime: String,
    pub build: String,
    pub operations: Vec<String>,
}

impl Capability {
    /// Unsupported is explicit so launcher UI can distinguish absent proof from
    /// an unset preference. It intentionally advertises no callable operation.
    pub fn unsupported(runtime: impl Into<String>, build: impl Into<String>) -> Self {
        Self {
            supported: false,
            runtime: runtime.into(),
            build: build.into(),
            operations: Vec::new(),
        }
    }

    /// Test descriptor for a runtime whose transform proof grants all operations.
    #[cfg(test)]
    pub fn certified(runtime: impl Into<String>, build: impl Into<String>) -> Self {
        Self {
            supported: true,
            runtime: runtime.into(),
            build: build.into(),
            operations: OPERATIONS.iter().map(ToString::to_string).collect(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservationClaim {
    pub session_id: String,
    pub names: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct ObservationFile {
    format: u32,
    names: Vec<String>,
}

pub fn validated_names(names: Vec<String>) -> Result<Vec<String>, ()> {
    if names.len() > MAX_OBSERVED_NAMES {
        return Err(());
    }
    let mut distinct = BTreeSet::new();
    let mut accepted = Vec::new();
    for name in names {
        if name.is_empty()
            || name.encode_utf16().count() > MAX_OBSERVED_NAME_UTF16
            || name.chars().any(char::is_control)
        {
            return Err(());
        }
        if distinct.insert(name.clone()) {
            accepted.push(name);
        }
    }
    Ok(accepted)
}

pub fn save_observations(profile_support: &Path, names: Vec<String>) -> Result<(), ()> {
    let names = validated_names(names)?;
    let _guard = OBSERVATION_WRITE
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| ())?;
    fs::create_dir_all(profile_support).map_err(|_| ())?;
    let path = profile_support.join(OBSERVATIONS_FILE);
    let temporary = path.with_extension(format!(
        "json.{}.{}.tmp",
        std::process::id(),
        OBSERVATION_TEMPORARY.fetch_add(1, Ordering::Relaxed)
    ));
    let bytes = serde_json::to_vec(&ObservationFile {
        format: OBSERVATIONS_FORMAT,
        names,
    })
    .map_err(|_| ())?;
    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, &path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|_| ())
}

/// Corrupt or future observation data merely loses autocomplete suggestions;
/// it must never make Accounts or launcher snapshot unavailable.
pub fn load_observations(profile_support: &Path) -> Vec<String> {
    let path = profile_support.join(OBSERVATIONS_FILE);
    let Ok(file) = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    else {
        return Vec::new();
    };
    let Ok(metadata) = file.metadata() else {
        return Vec::new();
    };
    if !metadata.file_type().is_file() || metadata.len() > MAX_OBSERVATION_BYTES as u64 {
        return Vec::new();
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    if file
        .take((MAX_OBSERVATION_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() > MAX_OBSERVATION_BYTES
    {
        return Vec::new();
    }
    let Ok(file) = serde_json::from_slice::<ObservationFile>(&bytes) else {
        return Vec::new();
    };
    if file.format != OBSERVATIONS_FORMAT {
        return Vec::new();
    }
    validated_names(file.names).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_capability_cannot_accidentally_advertise_action() {
        assert_eq!(
            Capability::unsupported("jspi", "exact").operations,
            Vec::<String>::new()
        );
    }

    #[test]
    fn certified_capability_requires_closed_operation_set() {
        assert_eq!(
            Capability::certified("asyncify", "exact").operations.len(),
            OPERATIONS.len()
        );
    }

    #[test]
    fn observed_names_are_bounded_utf16_and_deduplicated() {
        assert_eq!(
            validated_names(vec!["Cynn".into(), "Cynn".into(), "Mhenlo".into()]).unwrap(),
            ["Cynn", "Mhenlo"]
        );
        assert!(validated_names(vec!["x".repeat(41)]).is_err());
        assert!(validated_names(vec!["bad\nname".into()]).is_err());
    }

    #[test]
    fn observation_file_is_private_atomic_and_malformed_is_empty() {
        let temp = crate::scratch::TempDir::new("character-observations");
        save_observations(&temp.0, vec!["Devona".into()]).unwrap();
        assert_eq!(load_observations(&temp.0), ["Devona"]);
        let metadata = std::fs::metadata(temp.0.join(OBSERVATIONS_FILE)).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        std::fs::write(temp.0.join(OBSERVATIONS_FILE), b"not json").unwrap();
        assert!(load_observations(&temp.0).is_empty());
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn observation_reader_refuses_links_and_oversized_files() {
        let temp = crate::scratch::TempDir::new("character-observation-bounds");
        let outside = temp.0.join("outside");
        std::fs::write(&outside, b"{\"format\":1,\"names\":[\"Devona\"]}").unwrap();
        std::os::unix::fs::symlink(&outside, temp.0.join(OBSERVATIONS_FILE)).unwrap();
        assert!(load_observations(&temp.0).is_empty());
        std::fs::remove_file(temp.0.join(OBSERVATIONS_FILE)).unwrap();
        std::fs::write(
            temp.0.join(OBSERVATIONS_FILE),
            vec![b'x'; MAX_OBSERVATION_BYTES + 1],
        )
        .unwrap();
        assert!(load_observations(&temp.0).is_empty());
    }
}
