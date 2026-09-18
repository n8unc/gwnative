//! Release-reviewed cursor layouts, independent of template rewriting.
//!
//! These records are compiled into the application, like the bundled template
//! certificate. No downloaded offsets or caller-supplied manifest are accepted.
//! Unknown Wasm OR glue bytes leave the cursor disabled. See
//! `docs/cursor-compatibility.md` for the disassembly and runtime checks.

use std::path::Path;

use super::certificate::{LayoutCertificate, Runtime};
use super::{digest, rewrite};

const FAMILY: &str = "85594456b6c33aa5a6001bbe65d66b32e918a42dfa3b76224081f245996461af";
const JSPI_WASM: &str = "1eb07332632e2fca8aabf5baa14fa1a1e6a2a59ec7134dfb8f6231d924c9fd7b";
const JSPI_GLUE: &str = "4ee7c8af5aa5f5c2e9a1334642fdc52075464da4d203fc676c7becb31669a2a8";
const ASYNCIFY_WASM: &str = "373c65abcfe6c26161ffd59cb8ea298eb3d1814f4778aeaf24d8978c1fdfa8ef";
const ASYNCIFY_GLUE: &str = "e6d031a22d9047f30a1e2d1d2e25f6daca552df4a5dc1e3fad83b57bf404463c";

fn matches(runtime: Runtime, wasm_hash: &str, glue_hash: &str) -> bool {
    let pair = match runtime {
        Runtime::Jspi => (JSPI_WASM, JSPI_GLUE),
        Runtime::Asyncify => (ASYNCIFY_WASM, ASYNCIFY_GLUE),
    };
    (wasm_hash, glue_hash) == pair
}

fn layout() -> LayoutCertificate {
    // No target/game-state offsets have been reviewed for this record. Keep
    // them zero and authorize only FEATURE_NATIVE_CURSOR in the page manifest.
    let mut words = vec![0; 17];
    words.extend_from_slice(&[
        5_910_960, 5_910_964, 5_910_968, 2_730_272, 0, 12, 8, 0, 8, 12, 20, 24,
    ]);
    LayoutCertificate {
        snapshot_abi: 1,
        snapshot_bytes: 64,
        cursor_snapshot_abi: 1,
        cursor_snapshot_bytes: 4160,
        layout_words: words,
        data_sha256: "82ac696353b647245101804aa43ce0f713d58297967c2ca54e7935cff32b45dc".into(),
        element_sha256: "469ef6f8f6da821169de1ee65a912097e3f82281a4cdbd24d52b3f911cf46f7f".into(),
        shared_global_prefix_sha256:
            "2eb48324dc89101a8eefdca8c6f8b7bee31868c237d4b879db2d373238628032".into(),
        shared_global_count: 10,
    }
}

pub(super) fn manifest(root: &Path, runtime: Runtime) -> Option<serde_json::Value> {
    let wasm = std::fs::read(root.join(runtime.wasm_name())).ok()?;
    let glue = std::fs::read(root.join(runtime.glue_name())).ok()?;
    if !matches(runtime, &digest(&wasm), &digest(&glue)) {
        return None;
    }
    let layout = layout();
    rewrite::verify_layout(&wasm, &layout).ok()?;
    let mut manifest = layout.page_manifest(FAMILY);
    manifest["featureMask"] = serde_json::json!(1);
    Some(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_review_never_authorizes_another_pair_or_runtime() {
        assert!(matches(Runtime::Jspi, JSPI_WASM, JSPI_GLUE));
        assert!(matches(Runtime::Asyncify, ASYNCIFY_WASM, ASYNCIFY_GLUE));
        assert!(!matches(Runtime::Jspi, ASYNCIFY_WASM, ASYNCIFY_GLUE));
        assert!(!matches(Runtime::Asyncify, JSPI_WASM, JSPI_GLUE));
        assert!(!matches(Runtime::Jspi, JSPI_WASM, ASYNCIFY_GLUE));
        assert!(!matches(Runtime::Jspi, &"0".repeat(64), JSPI_GLUE));
        assert!(!matches(Runtime::Jspi, JSPI_WASM, &"0".repeat(64)));
    }

    #[test]
    fn cursor_layout_contains_no_unreviewed_game_state_offsets() {
        assert_eq!(layout().layout_words.len(), 29);
        assert!(layout().layout_words[..17].iter().all(|word| *word == 0));
    }
}
