use super::*;

use crate::scratch::TempDir;
use std::time::{Duration, SystemTime};

fn png(pixel: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&pixel)
        .unwrap();
    bytes
}

fn encrypted(bytes: &[u8], check: u8) -> Vec<u8> {
    let mut crypto = ZipCrypto::new();
    let mut plain = vec![0; 12];
    plain[11] = check;
    plain.extend_from_slice(bytes);
    plain
        .into_iter()
        .map(|value| {
            let key = (((crypto.c | 2).wrapping_mul((crypto.c | 2) ^ 1)) >> 8) as u8;
            crypto.update(value);
            value ^ key
        })
        .collect()
}

/// Minimal stored, encrypted Zip archive then legacy TPF XOR wrapper.
fn tpf(entries: &[(&str, Vec<u8>)], corrupt_crc: bool) -> Vec<u8> {
    struct Central {
        name: Vec<u8>,
        crc: u32,
        size: usize,
        offset: usize,
    }
    let mut archive = Vec::new();
    let mut central = Vec::new();
    for (name, value) in entries {
        let name = name.as_bytes().to_vec();
        let crc = crc32(value);
        let offset = archive.len();
        let encrypted = encrypted(value, (crc >> 24) as u8);
        archive.extend_from_slice(b"PK\x03\x04");
        archive.extend_from_slice(&20u16.to_le_bytes());
        archive.extend_from_slice(&1u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&crc.to_le_bytes());
        archive.extend_from_slice(&(encrypted.len() as u32).to_le_bytes());
        archive.extend_from_slice(&(value.len() as u32).to_le_bytes());
        archive.extend_from_slice(&(name.len() as u16).to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&name);
        archive.extend_from_slice(&encrypted);
        central.push(Central {
            name,
            crc,
            size: value.len(),
            offset,
        });
    }
    let directory = archive.len();
    for (index, entry) in central.iter().enumerate() {
        archive.extend_from_slice(b"PK\x01\x02");
        archive.extend_from_slice(&20u16.to_le_bytes());
        archive.extend_from_slice(&20u16.to_le_bytes());
        archive.extend_from_slice(&1u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        let crc = if corrupt_crc && index == 1 {
            entry.crc ^ 1
        } else {
            entry.crc
        };
        archive.extend_from_slice(&crc.to_le_bytes());
        archive.extend_from_slice(&((entry.size + 12) as u32).to_le_bytes());
        archive.extend_from_slice(&(entry.size as u32).to_le_bytes());
        archive.extend_from_slice(&(entry.name.len() as u16).to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u32.to_le_bytes());
        archive.extend_from_slice(&(entry.offset as u32).to_le_bytes());
        archive.extend_from_slice(&entry.name);
    }
    let directory_bytes = archive.len() - directory;
    archive.extend_from_slice(b"PK\x05\x06");
    archive.extend_from_slice(&0u16.to_le_bytes());
    archive.extend_from_slice(&0u16.to_le_bytes());
    archive.extend_from_slice(&(central.len() as u16).to_le_bytes());
    archive.extend_from_slice(&(central.len() as u16).to_le_bytes());
    archive.extend_from_slice(&(directory_bytes as u32).to_le_bytes());
    archive.extend_from_slice(&(directory as u32).to_le_bytes());
    archive.extend_from_slice(&0u16.to_le_bytes());
    archive
        .into_iter()
        .enumerate()
        .map(|(i, value)| value ^ XOR[i & 3])
        .collect()
}

fn pack(definitions: &str, pixel: [u8; 4]) -> Vec<u8> {
    tpf(
        &[
            ("texmod.def", definitions.as_bytes().to_vec()),
            ("ui.png", png(pixel)),
        ],
        false,
    )
}

fn xor_tpf(mut archive: Vec<u8>) -> Vec<u8> {
    for (index, value) in archive.iter_mut().enumerate() {
        *value ^= XOR[index & 3];
    }
    archive
}

fn split_disk_tpf(encoded: &[u8]) -> Vec<u8> {
    let mut archive = xor_tpf(encoded.to_vec());
    let end = archive
        .windows(4)
        .rposition(|value| value == b"PK\x05\x06")
        .unwrap();
    archive[end + 4..end + 6].copy_from_slice(&1u16.to_le_bytes());
    xor_tpf(archive)
}

fn scan_ready(library: &mut TextureLibrary, root: &std::path::Path, at: SystemTime) {
    library.scan_at(root, at).unwrap();
    library
        .scan_at(root, at + Duration::from_millis(500))
        .unwrap();
}

fn malicious_session(entries: Vec<TextureEntry>) -> (TempDir, std::path::PathBuf) {
    malicious_session_packs(vec![entries])
}

fn malicious_session_packs(entry_sets: Vec<Vec<TextureEntry>>) -> (TempDir, std::path::PathBuf) {
    let temp = TempDir::new("texture-malicious-session");
    let base = temp.0.join("library");
    TextureLibrary::new(&base).unwrap();
    let manifest = base.join("sessions/malicious.json");
    let record = SessionRecord {
        format: 1,
        packs: entry_sets
            .into_iter()
            .enumerate()
            .map(|(index, entries)| {
                let revision = digest(format!("malicious-session-revision-{index}").as_bytes());
                let asset = revision_path(&base, "ignored", &revision);
                std::fs::create_dir_all(asset.parent().unwrap()).unwrap();
                std::fs::write(&asset, serde_json::to_vec(&entries).unwrap()).unwrap();
                SessionPackRef {
                    id: format!("tpf:malicious-{index}"),
                    revision,
                    asset_path: asset,
                }
            })
            .collect(),
    };
    std::fs::write(&manifest, serde_json::to_vec(&record).unwrap()).unwrap();
    (temp, manifest)
}

#[test]
fn session_manifest_rejects_malicious_dimensions_entry_counts_and_encoded_pixels() {
    let bad_dimensions = vec![TextureEntry {
        target: 1,
        width: 0,
        height: 1,
        rgba_base64: String::new(),
        compressed: None,
    }];
    let (_temp, manifest) = malicious_session(bad_dimensions);
    assert!(
        TextureLibrary::load_session_manifest(&manifest)
            .err()
            .is_some_and(|error| error.contains("dimensions"))
    );

    let oversized_aggregate = vec![
        TextureEntry {
            target: 1,
            width: MAX_DIMENSION,
            height: MAX_DIMENSION,
            rgba_base64: String::new(),
            compressed: None,
        },
        TextureEntry {
            target: 2,
            width: MAX_DIMENSION,
            height: MAX_DIMENSION,
            rgba_base64: String::new(),
            compressed: None,
        },
    ];
    let (_temp, manifest) = malicious_session(oversized_aggregate);
    assert!(
        TextureLibrary::load_session_manifest(&manifest)
            .err()
            .is_some_and(|error| error.contains("aggregate"))
    );

    let too_many = (0..(MAX_ENTRIES / 2 + 1))
        .map(|target| TextureEntry {
            target: target as u32,
            width: 1,
            height: 1,
            rgba_base64: "AQIDBA==".into(),
            compressed: None,
        })
        .collect::<Vec<_>>();
    let (_temp, manifest) = malicious_session_packs(vec![too_many.clone(), too_many]);
    assert!(
        TextureLibrary::load_session_manifest(&manifest)
            .err()
            .is_some_and(|error| error.contains("too many entries"))
    );

    let invalid_encoded = vec![TextureEntry {
        target: 1,
        width: 1,
        height: 1,
        rgba_base64: "!!!!!!!!".into(),
        compressed: None,
    }];
    let (_temp, manifest) = malicious_session(invalid_encoded);
    let error = TextureLibrary::load_session_manifest(&manifest).unwrap_err();
    assert!(error.contains("malformed"), "{error}");

    let malformed_compressed = vec![TextureEntry {
        target: 1,
        width: 4,
        height: 4,
        rgba_base64: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0u8; 64]),
        compressed: Some(TextureCompressed {
            mode: "DXT9".into(),
            levels: vec!["AAAAAAAAAAA=".into()],
        }),
    }];
    let (_temp, manifest) = malicious_session(malformed_compressed);
    assert!(
        TextureLibrary::load_session_manifest(&manifest)
            .unwrap_err()
            .contains("compressed mode")
    );
}

#[test]
fn encrypted_tpf_png_decodes_rgba_and_rejects_bad_archive_data() {
    let valid = pack("0x1234|ui.png\n", [1, 2, 3, 4]);
    let decoded = parse_tpf(&valid).unwrap();
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].target, 0x1234);
    assert_eq!(decoded[0].rgba, [1, 2, 3, 4]);

    let bad_crc = tpf(
        &[
            ("texmod.def", b"0x1234|ui.png\n".to_vec()),
            ("ui.png", png([1, 2, 3, 4])),
        ],
        true,
    );
    assert!(
        parse_tpf(&bad_crc)
            .err()
            .is_some_and(|error| error.contains("checksum"))
    );
    assert!(parse_tpf(&valid[..valid.len() - 5]).is_err());
}

#[test]
fn definitions_reject_unsafe_or_wide_hashes_and_deduplicate_same_mapping() {
    assert!(parse_tpf(&pack("0x1|../ui.png\n", [1, 1, 1, 1])).is_err());
    assert!(parse_tpf(&pack("0x100000000|ui.png\n", [1, 1, 1, 1])).is_err());
    let same = parse_tpf(&pack("0x1|ui.png\n0x1|UI.PNG\n", [1, 1, 1, 1])).unwrap();
    assert_eq!(same.len(), 1);
    let conflicting = tpf(
        &[
            ("texmod.def", b"0x1|a.png\n0x1|b.png\n".to_vec()),
            ("a.png", png([1, 1, 1, 1])),
            ("b.png", png([2, 2, 2, 2])),
        ],
        false,
    );
    assert!(
        parse_tpf(&conflicting)
            .err()
            .is_some_and(|error| error.contains("conflicting"))
    );
}

#[test]
fn parser_rejects_split_archives_and_ambiguous_central_names() {
    let valid = pack("0x1|ui.png\n", [1, 2, 3, 4]);
    assert!(
        parse_tpf(&split_disk_tpf(&valid))
            .err()
            .is_some_and(|error| error.contains("split"))
    );
    let duplicate_image = tpf(
        &[
            ("texmod.def", b"0x1|ui.png\n".to_vec()),
            ("ui.png", png([1, 2, 3, 4])),
            ("UI.PNG", png([5, 6, 7, 8])),
        ],
        false,
    );
    assert!(
        parse_tpf(&duplicate_image)
            .err()
            .is_some_and(|error| error.contains("duplicate entry"))
    );
    let duplicate_definitions = tpf(
        &[
            ("texmod.def", b"0x1|ui.png\n".to_vec()),
            ("TEXMOD.DEF", b"0x1|ui.png\n".to_vec()),
            ("ui.png", png([1, 2, 3, 4])),
        ],
        false,
    );
    assert!(parse_tpf(&duplicate_definitions).is_err());
}

#[test]
fn bounded_regular_reads_valid_file_and_refuses_symlink_or_oversize() {
    let temp = TempDir::new("texture-bounded-read");
    let valid = temp.0.join("valid");
    std::fs::write(&valid, b"safe").unwrap();
    assert_eq!(read_bounded_regular(&valid, 4).unwrap(), b"safe");
    let oversize = temp.0.join("oversize");
    std::fs::write(&oversize, b"oversized").unwrap();
    assert!(read_bounded_regular(&oversize, 4).is_err());
    let link = temp.0.join("link");
    std::os::unix::fs::symlink(&valid, &link).unwrap();
    assert!(read_bounded_regular(&link, 4).is_err());
}

#[test]
fn scanner_ignores_non_tpf_and_nested_sources() {
    let temp = TempDir::new("texture-scanner-scope");
    std::fs::write(temp.0.join("notes.txt"), b"not a texture pack").unwrap();
    let nested = temp.0.join("nested");
    std::fs::create_dir(&nested).unwrap();
    std::fs::write(nested.join("ui.tpf"), pack("0x1|ui.png\n", [1, 2, 3, 4])).unwrap();
    let mut library = TextureLibrary::new(temp.0.join("library")).unwrap();
    scan_ready(
        &mut library,
        &temp.0,
        SystemTime::UNIX_EPOCH + Duration::from_secs(1),
    );
    assert!(library.snapshot().is_empty());
    assert_eq!(library.pending_count(), 0);
}

#[test]
fn invalid_atomic_replacement_retains_last_good_across_restart() {
    let temp = TempDir::new("texture-invalid-last-good");
    let source = temp.0.join("ui.tpf");
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
    std::fs::write(&source, pack("0x1|ui.png\n", [1, 2, 3, 4])).unwrap();
    let mut library = TextureLibrary::new(temp.0.join("library")).unwrap();
    scan_ready(&mut library, &temp.0, now);
    let before = library.snapshot().pop().unwrap();
    let revision = before.revision.clone().unwrap();
    let stage = temp.0.join(".ui.tpf.stage");
    std::fs::write(&stage, pack("0x10|ui.png\n", [9, 8, 7, 6])).unwrap();
    std::fs::rename(&stage, &source).unwrap();
    scan_ready(&mut library, &temp.0, now + Duration::from_secs(2));
    let replaced = library.snapshot().pop().unwrap();
    assert_ne!(replaced.revision.as_deref(), Some(revision.as_str()));
    let revision = replaced.revision.clone().unwrap();
    std::fs::write(&stage, b"not a TPF").unwrap();
    std::fs::rename(&stage, &source).unwrap();
    scan_ready(&mut library, &temp.0, now + Duration::from_secs(4));
    let after = library.snapshot().pop().unwrap();
    assert_eq!(after.status, TexturePackStatus::Ready);
    assert_eq!(after.revision.as_deref(), Some(revision.as_str()));
    assert!(after.error.is_some());
    let restarted = TextureLibrary::new(temp.0.join("library")).unwrap();
    assert_eq!(
        restarted.snapshot()[0].revision.as_deref(),
        Some(revision.as_str())
    );
    assert_eq!(
        restarted
            .pin(&[PackSelection {
                id: after.id,
                revision: None
            }])
            .packs
            .len(),
        1
    );
}

#[test]
fn prune_keeps_current_and_pinned_revision_then_removes_released_history_and_orphan() {
    let temp = TempDir::new("texture-prune-pins");
    let root = &temp.0;
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
    let a = root.join("a.tpf");
    let b = root.join("b.tpf");
    std::fs::write(&a, pack("0x1|ui.png\n", [1, 1, 1, 1])).unwrap();
    std::fs::write(&b, pack("0x2|ui.png\n", [2, 2, 2, 2])).unwrap();
    let mut library = TextureLibrary::new(root.join("library")).unwrap();
    scan_ready(&mut library, root, now);
    let initial = library.snapshot();
    let a_summary = initial.iter().find(|value| value.source == a).unwrap();
    let old = a_summary.revision.clone().unwrap();
    let a_id = a_summary.id.clone();
    let b_summary = initial.iter().find(|value| value.source == b).unwrap();
    let b_current = b_summary.revision.clone().unwrap();
    std::fs::write(&a, pack("0x10|ui.png\n", [3, 3, 3, 3])).unwrap();
    scan_ready(&mut library, root, now + Duration::from_secs(2));
    let a_current = library
        .snapshot()
        .iter()
        .find(|value| value.id == a_id)
        .unwrap()
        .revision
        .clone()
        .unwrap();
    let manifest = library
        .pin_to_file(
            "old-a",
            &[PackSelection {
                id: a_id.clone(),
                revision: Some(old.clone()),
            }],
        )
        .unwrap();
    let orphan = digest(b"synthetic orphan revision");
    write_revision(
        &library.base,
        "ignored",
        &orphan,
        &[DecodedEntry {
            target: 9,
            width: 1,
            height: 1,
            rgba: vec![9, 9, 9, 9],
            compressed: None,
        }],
    )
    .unwrap();
    library.prune_unused_revisions().unwrap();
    assert!(revision_path(&library.base, &a_id, &old).is_file());
    assert!(revision_path(&library.base, &a_id, &a_current).is_file());
    assert!(revision_path(&library.base, &b_summary.id, &b_current).is_file());
    assert!(!revision_path(&library.base, "ignored", &orphan).exists());
    let lease = TextureLibrary::acquire_session_lease(&manifest).unwrap();
    drop(lease);
    assert!(TextureLibrary::release_session(&manifest).unwrap());
    library.prune_unused_revisions().unwrap();
    assert!(!revision_path(&library.base, &a_id, &old).exists());
    assert!(revision_path(&library.base, &a_id, &a_current).is_file());
}
