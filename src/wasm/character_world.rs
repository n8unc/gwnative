//! Closed JSPI entered-world observation for a frozen 128-bit character UUID.
//!
//! This module certifies only current credential-prefill JSPI output. It emits
//! one four-word UUID comparator; no heap address, roster index, name, or
//! generic read primitive crosses its export boundary.

use wasmparser::{BinaryReader, ImportSectionReader, Operator, TypeRef, Validator};

#[cfg(test)]
use super::codec::{Section, WASM_HEADER, encode_section};
use super::codec::{parse_code, section_by_id, sleb, split_sections, uleb};
use super::{Outcome, digest};

pub(super) const WORLD_ENTERED_EXPORT: &str = "GwnativeCharacterWorldEntered";

const JSPI_PREFILL_SHA256: &str =
    "e2ad01cc8f3ccb82263399b72ec596f47e9a5a7439e640f7089463cb5abe07db";
const ROSTER_READERS: &[(u32, &str)] = &[
    (
        10_128,
        "87b6b897fd8df7688f46fcaf517b78b1af5334d00e85f4c20d9d9978a4f799e0",
    ),
    (
        10_129,
        "49eb229605004bc69ce17787f6e38d1b453892e740a1b0f3e071d86e4685d6aa",
    ),
];
const WORLD_READERS: &[(u32, &str)] = &[
    (
        228,
        "2c71a9a0ced310eca89eb420643fcb3e22474a276b5bc9703f653462fdbc55c8",
    ),
    (
        9_517,
        "c6e2e54332d133eb208974890b255570fb2fcc9a802ea52957b4a64116f7e72c",
    ),
    (
        9_524,
        "3656f5655d9704c608247be52aaf1654141bce2db8f085c6cef4c967c7068acf",
    ),
    (
        9_557,
        "3ff1eea7cc5052ba8f985b865847a676564b1b776eff897800efac1a3d8d3413",
    ),
    (
        9_560,
        "2580db8af0702dd9338068470bfcb08eb5c9e17d320c66b90d2838bda48e57f2",
    ),
    (
        8_939,
        "4f5dba8233bdb016c47be222cd3c710cbfd7c32f982c935a3e8568312179b5bb",
    ),
    (
        8_940,
        "f63235d97028ffe7bfab880ac8a0f85c88a3b3323fe2e9893ba06d0ea1a3b891",
    ),
];

const RECORD_BYTES: u32 = 132;
// Current f9560 copies its input's 16 bytes at +0/+8 into CharContext +100/+108.
// Its selected-record call path supplies `record + 8`, so this is current-client
// provenance for the exported roster UUID rather than a reference-project claim.
const ROSTER_UUID_OFFSET: u32 = 8;
const CONTEXT_ROOT: u32 = 2_680_560;
const CHARACTER_CONTEXT_ACCESSOR: u32 = 228;
const CHARACTER_CONTEXT_SLOT: u32 = 17;
const CURRENT_MAP: u32 = 564;
const PLAYER_NUMBER: u32 = 684;
const CHARACTER_UUID: u32 = 100;
const CHARACTER_REQUIRED: u32 = PLAYER_NUMBER + 4;
const MAP_CONTEXT_SLOT: u32 = 11;
const MAP_PLAYER_TABLE: u32 = 2_060;
const MAP_PLAYER_COUNT: u32 = 2_068;
const MAP_PLAYER_STRIDE: u32 = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WorldLayout {
    pub(crate) record_bytes: u32,
    pub(crate) roster_uuid_offset: u32,
    pub(crate) context_root: u32,
    pub(crate) character_context_slot: u32,
    pub(crate) current_map: u32,
    pub(crate) player_number: u32,
    pub(crate) character_uuid: u32,
    pub(crate) map_context_slot: u32,
    pub(crate) map_player_table: u32,
    pub(crate) map_player_count: u32,
    pub(crate) map_player_stride: u32,
}

fn imports(section: &[u8]) -> Outcome<u32> {
    let reader = ImportSectionReader::new(BinaryReader::new(section, 0))
        .map_err(|error| format!("character-world: imports: {error}"))?;
    let mut functions = 0u32;
    for item in reader.into_imports() {
        if matches!(
            item.map_err(|error| format!("character-world: import: {error}"))?
                .ty,
            TypeRef::Func(_) | TypeRef::FuncExact(_)
        ) {
            functions = functions
                .checked_add(1)
                .ok_or("character-world: too many imports")?;
        }
    }
    Ok(functions)
}

fn function_body<'a>(bodies: &'a [Vec<u8>], imported: u32, index: u32) -> Outcome<&'a [u8]> {
    let local = index
        .checked_sub(imported)
        .ok_or("character-world: imported reader")? as usize;
    bodies
        .get(local)
        .map(Vec::as_slice)
        .ok_or("character-world: missing reader".into())
}

fn operators(body: &[u8]) -> Outcome<Vec<Operator<'_>>> {
    let mut reader = wasmparser::FunctionBody::new(BinaryReader::new(body, 0))
        .get_operators_reader()
        .map_err(|error| format!("character-world: operators: {error}"))?;
    let mut result = Vec::new();
    while !reader.eof() {
        result.push(
            reader
                .read()
                .map_err(|error| format!("character-world: operator: {error}"))?,
        );
    }
    Ok(result)
}

fn i32_constants(body: &[u8]) -> Outcome<Vec<i32>> {
    Ok(operators(body)?
        .into_iter()
        .filter_map(|operator| match operator {
            Operator::I32Const { value } => Some(value),
            _ => None,
        })
        .collect())
}

fn calls(body: &[u8]) -> Outcome<Vec<u32>> {
    Ok(operators(body)?
        .into_iter()
        .filter_map(|operator| match operator {
            Operator::Call { function_index } => Some(function_index),
            _ => None,
        })
        .collect())
}

fn load_offsets(body: &[u8]) -> Outcome<Vec<u64>> {
    Ok(operators(body)?
        .into_iter()
        .filter_map(|operator| match operator {
            Operator::I32Load { memarg } => Some(memarg.offset),
            _ => None,
        })
        .collect())
}

fn i64_offsets(body: &[u8], stores: bool) -> Outcome<Vec<u64>> {
    Ok(operators(body)?
        .into_iter()
        .filter_map(|operator| match (stores, operator) {
            (false, Operator::I64Load { memarg }) | (true, Operator::I64Store { memarg }) => {
                Some(memarg.offset)
            }
            _ => None,
        })
        .collect())
}

fn has_memory_copy(body: &[u8]) -> Outcome<bool> {
    Ok(operators(body)?
        .into_iter()
        .any(|operator| matches!(operator, Operator::MemoryCopy { .. })))
}

/// Certifies exact prefixed JSPI output plus current reader/copy semantics.
/// Operator checks avoid treating byte positions in a variable-length Wasm
/// encoding as a layout proof.
pub(super) fn certify(input: &[u8]) -> Outcome<WorldLayout> {
    Validator::new()
        .validate_all(input)
        .map_err(|error| format!("character-world: invalid input: {error}"))?;
    if digest(input) != JSPI_PREFILL_SHA256 {
        return Err("character-world: unsupported input".into());
    }
    let sections = split_sections(input)?;
    let imported = imports(section_by_id(&sections, 2)?)?;
    let bodies = parse_code(section_by_id(&sections, 10)?)?;
    for &(index, expected) in ROSTER_READERS.iter().chain(WORLD_READERS.iter()) {
        let actual = digest(function_body(&bodies, imported, index)?);
        if actual != expected {
            return Err(format!("character-world: certified reader {index} changed"));
        }
    }
    let roster = function_body(&bodies, imported, ROSTER_READERS[0].0)?;
    let selected = function_body(&bodies, imported, ROSTER_READERS[1].0)?;
    // f10128 reads count/table, selects `index * 132`, then memory.copies an
    // entire record. f10129 reads that same table and compares its name +24.
    // Exact bodies bind this to current list code; semantic checks bind values.
    if !has_memory_copy(roster)?
        || !has_memory_copy(selected)?
        || !i32_constants(roster)?.contains(&(RECORD_BYTES as i32))
        || !i32_constants(selected)?.contains(&(RECORD_BYTES as i32))
        || !i32_constants(selected)?.contains(&24)
        || !load_offsets(roster)?.contains(&5_928_432)
        || !load_offsets(roster)?.contains(&5_928_424)
    {
        return Err("character-world: record consumer semantics changed".into());
    }
    let accessor = function_body(&bodies, imported, CHARACTER_CONTEXT_ACCESSOR)?;
    let current_map = function_body(&bodies, imported, 9_517)?;
    let player_number = function_body(&bodies, imported, 9_524)?;
    let character_uuid = function_body(&bodies, imported, 9_557)?;
    let record_uuid_copy = function_body(&bodies, imported, 9_560)?;
    let player_lookup = function_body(&bodies, imported, 8_939)?;
    let current_player_lookup = function_body(&bodies, imported, 8_940)?;
    // These exact client readers bind f228's root/slot and every field used
    // below. The direct root read avoids f228's transition-time assertion.
    if !i32_constants(accessor)?.contains(&2)
        || !load_offsets(accessor)?.contains(&(CONTEXT_ROOT as u64))
        || !load_offsets(accessor)?.contains(&0)
        || !calls(current_map)?.contains(&CHARACTER_CONTEXT_ACCESSOR)
        || !i32_constants(current_map)?.contains(&(CHARACTER_CONTEXT_SLOT as i32))
        || !load_offsets(current_map)?.contains(&(CURRENT_MAP as u64))
        || !calls(player_number)?.contains(&CHARACTER_CONTEXT_ACCESSOR)
        || !i32_constants(player_number)?.contains(&(CHARACTER_CONTEXT_SLOT as i32))
        || !load_offsets(player_number)?.contains(&(PLAYER_NUMBER as u64))
        || !calls(character_uuid)?.contains(&CHARACTER_CONTEXT_ACCESSOR)
        || !i32_constants(character_uuid)?.contains(&(CHARACTER_CONTEXT_SLOT as i32))
        || !i32_constants(character_uuid)?.contains(&(CHARACTER_UUID as i32))
        // f9560 receives roster +8 and writes its two 64-bit halves into the
        // current CharContext UUID at +100/+108.
        || !calls(record_uuid_copy)?.contains(&CHARACTER_CONTEXT_ACCESSOR)
        || !i32_constants(record_uuid_copy)?.contains(&(CHARACTER_CONTEXT_SLOT as i32))
        || i64_offsets(record_uuid_copy, false)? != [8, 0]
        || i64_offsets(record_uuid_copy, true)? != [108, 100]
        // f8940 feeds f9524's current player number into f8939. f8939 reads
        // map context slot 11's table/count, then returns table[player * 80].
        || calls(current_player_lookup)? != [9_524, 8_939]
        || !calls(player_lookup)?.contains(&CHARACTER_CONTEXT_ACCESSOR)
        || !i32_constants(player_lookup)?.contains(&(MAP_CONTEXT_SLOT as i32))
        || !i32_constants(player_lookup)?.contains(&(MAP_PLAYER_TABLE as i32))
        || !i32_constants(player_lookup)?.contains(&(MAP_PLAYER_STRIDE as i32))
        || !load_offsets(player_lookup)?.contains(&(MAP_PLAYER_TABLE as u64))
        || !load_offsets(player_lookup)?.contains(&(MAP_PLAYER_COUNT as u64))
    {
        return Err("character-world: current context accessors changed".into());
    }
    Ok(WorldLayout {
        record_bytes: RECORD_BYTES,
        roster_uuid_offset: ROSTER_UUID_OFFSET,
        context_root: CONTEXT_ROOT,
        character_context_slot: CHARACTER_CONTEXT_SLOT,
        current_map: CURRENT_MAP,
        player_number: PLAYER_NUMBER,
        character_uuid: CHARACTER_UUID,
        map_context_slot: MAP_CONTEXT_SLOT,
        map_player_table: MAP_PLAYER_TABLE,
        map_player_count: MAP_PLAYER_COUNT,
        map_player_stride: MAP_PLAYER_STRIDE,
    })
}

fn i32(value: i64) -> Vec<u8> {
    let mut out = vec![0x41];
    out.extend(sleb(value));
    out
}
fn get(index: u32) -> Vec<u8> {
    let mut out = vec![0x20];
    out.extend(uleb(index as u64));
    out
}
fn set(index: u32) -> Vec<u8> {
    let mut out = vec![0x21];
    out.extend(uleb(index as u64));
    out
}
fn load(offset: u32) -> Vec<u8> {
    let mut out = vec![0x28, 0x02];
    out.extend(uleb(offset as u64));
    out
}
fn ret(value: i64) -> Vec<u8> {
    let mut out = i32(value);
    out.push(0x0f);
    out
}

fn require_memory(out: &mut Vec<u8>, pointer_local: u32, memory_local: u32, bytes: u32) {
    out.extend(get(memory_local));
    out.extend(i32(bytes as i64));
    out.push(0x49); // lt_u
    out.extend([0x04, 0x40]);
    out.extend(ret(-1));
    out.push(0x0b);
    out.extend(get(pointer_local));
    out.push(0x45); // eqz
    out.extend([0x04, 0x40]);
    out.extend(ret(-1));
    out.push(0x0b);
    out.extend(get(pointer_local));
    out.extend(get(memory_local));
    out.extend(i32(bytes as i64));
    out.push(0x6b);
    out.push(0x4b); // pointer > memory - bytes
    out.extend([0x04, 0x40]);
    out.extend(ret(-1));
    out.push(0x0b);
}

/// Context roots disappear during ordinary character-select and map
/// transitions. A null root is therefore waiting; only a non-null pointer
/// outside current linear memory is malformed.
fn require_optional_memory(out: &mut Vec<u8>, pointer_local: u32, memory_local: u32, bytes: u32) {
    out.extend(get(pointer_local));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    out.extend(ret(0));
    out.push(0x0b);
    require_memory(out, pointer_local, memory_local, bytes);
}

/// Emits `(uuid0, uuid1, uuid2, uuid3) -> i32`.
///
/// `1`: ready world belongs to frozen UUID. `0`: valid non-entered/loading or
/// mismatch. `-1`: corrupt/unavailable pointer or impossible state. UUID words
/// are opaque; function neither reads roster records nor exposes game memory.
pub(super) fn world_entered_reader(layout: WorldLayout) -> Vec<u8> {
    // locals: memory byte length, contexts, character context, scratch, map context
    let mut out = vec![0x01, 0x05, 0x7f];
    const MEM: u32 = 4;
    const CONTEXTS: u32 = 5;
    const CHARACTER: u32 = 6;
    const SCRATCH: u32 = 7;
    const MAP_CONTEXT: u32 = 8;
    out.extend([0x3f, 0x00]);
    out.extend(i32(65_536));
    out.push(0x6c);
    out.extend(set(MEM));

    // Frozen target must be a nonzero 128-bit UUID. Empty target is not entered.
    out.extend(get(0));
    out.extend(get(1));
    out.push(0x72);
    out.extend(get(2));
    out.push(0x72);
    out.extend(get(3));
    out.push(0x72);
    out.push(0x45);
    out.extend([0x04, 0x40]);
    out.extend(ret(0));
    out.push(0x0b);

    out.extend(i32(layout.context_root as i64));
    out.extend(set(SCRATCH));
    require_memory(&mut out, SCRATCH, MEM, 4);
    out.extend(get(SCRATCH));
    out.extend(load(0));
    out.extend(set(CONTEXTS));
    require_optional_memory(
        &mut out,
        CONTEXTS,
        MEM,
        layout.character_context_slot * 4 + 4,
    );
    out.extend(get(CONTEXTS));
    out.extend(load(layout.character_context_slot * 4));
    out.extend(set(CHARACTER));
    require_optional_memory(&mut out, CHARACTER, MEM, CHARACTER_REQUIRED);

    // Map and player readers are direct f228(17) consumers. Both must be
    // populated before this treats context as stable; no generic +572 helper
    // is used. Their certified bodies prove field provenance, not a broader
    // client "entered" flag, so absent values conservatively remain waiting.
    out.extend(get(CHARACTER));
    out.extend(load(layout.current_map));
    out.extend(set(SCRATCH));
    out.extend(get(SCRATCH));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    out.extend(ret(0));
    out.push(0x0b);
    out.extend(get(CHARACTER));
    out.extend(load(layout.player_number));
    out.extend(set(SCRATCH));
    out.extend(get(SCRATCH));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    out.extend(ret(0));
    out.push(0x0b);

    // f8940's read-only meaning: current player ID resolves to a present row
    // in map context slot 11. Reproduce its table lookup without calling f8939,
    // which can allocate missing rows.
    out.extend(get(CONTEXTS));
    out.extend(load(layout.map_context_slot * 4));
    out.extend(set(MAP_CONTEXT));
    require_optional_memory(&mut out, MAP_CONTEXT, MEM, layout.map_player_count + 4);
    out.extend(get(MAP_CONTEXT));
    out.extend(load(layout.map_player_count));
    out.extend(set(SCRATCH));
    out.extend(get(SCRATCH));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    out.extend(ret(0));
    out.push(0x0b);
    out.extend(get(CHARACTER));
    out.extend(load(layout.player_number));
    out.extend(get(SCRATCH));
    out.push(0x4b); // player > count (unsigned); equal is also out of range
    out.extend(get(CHARACTER));
    out.extend(load(layout.player_number));
    out.extend(get(SCRATCH));
    out.push(0x46);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    out.extend(ret(0));
    out.push(0x0b);
    out.extend(get(MAP_CONTEXT));
    out.extend(load(layout.map_player_table));
    out.extend(set(SCRATCH));
    require_optional_memory(&mut out, SCRATCH, MEM, 4);
    out.extend(get(SCRATCH));
    out.extend(i32(3));
    out.push(0x71);
    out.push(0x45);
    out.push(0x45);
    out.extend([0x04, 0x40]);
    out.extend(ret(-1));
    out.push(0x0b);
    // Prove `count * stride` cannot wrap and that its complete table lies in
    // current memory before deriving the selected row address.
    out.extend(get(MAP_CONTEXT));
    // MAP_CONTEXT currently holds no value needed below; reuse it for count.
    out.extend(load(layout.map_player_count));
    out.extend(set(MAP_CONTEXT));
    out.extend(get(MAP_CONTEXT));
    out.extend(get(MEM));
    out.extend(i32(layout.map_player_stride as i64));
    out.push(0x6e); // div_u
    out.push(0x4b); // count > memory / stride
    out.extend([0x04, 0x40]);
    out.extend(ret(-1));
    out.push(0x0b);
    out.extend(get(SCRATCH));
    out.extend(get(MEM));
    out.extend(get(MAP_CONTEXT));
    out.extend(i32(layout.map_player_stride as i64));
    out.push(0x6c);
    out.push(0x6b);
    out.push(0x4b); // table > memory - count * stride
    out.extend([0x04, 0x40]);
    out.extend(ret(-1));
    out.push(0x0b);
    out.extend(get(SCRATCH));
    out.extend(get(CHARACTER));
    out.extend(load(layout.player_number));
    out.extend(i32(layout.map_player_stride as i64));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(set(MAP_CONTEXT));
    // Wrapped table + player*stride is malformed, not loading.
    out.extend(get(MAP_CONTEXT));
    out.extend(get(SCRATCH));
    out.push(0x49);
    out.extend([0x04, 0x40]);
    out.extend(ret(-1));
    out.push(0x0b);
    require_memory(&mut out, MAP_CONTEXT, MEM, 4);
    out.extend(get(MAP_CONTEXT));
    out.extend(load(0));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    out.extend(ret(0));
    out.push(0x0b);

    // Current UUID must be present and all four words must equal frozen target.
    out.extend(get(CHARACTER));
    out.extend(load(layout.character_uuid));
    out.extend(get(CHARACTER));
    out.extend(load(layout.character_uuid + 4));
    out.push(0x72);
    out.extend(get(CHARACTER));
    out.extend(load(layout.character_uuid + 8));
    out.push(0x72);
    out.extend(get(CHARACTER));
    out.extend(load(layout.character_uuid + 12));
    out.push(0x72);
    out.push(0x45);
    out.extend([0x04, 0x40]);
    out.extend(ret(0));
    out.push(0x0b);
    for (word, parameter) in [0, 1, 2, 3].into_iter().enumerate() {
        out.extend(get(CHARACTER));
        out.extend(load(layout.character_uuid + (word as u32 * 4)));
        out.extend(get(parameter));
        out.push(0x47);
        out.extend([0x04, 0x40]);
        out.extend(ret(0));
        out.push(0x0b);
    }
    out.extend(ret(1));
    out.push(0x0b);
    out
}

#[cfg(test)]
mod tests {
    use super::super::codec::{encode_code, encode_index_vector};
    use super::*;

    fn fixture(body: Vec<u8>) -> Vec<u8> {
        let mut module = WASM_HEADER.to_vec();
        module.extend(encode_section(&Section {
            id: 1,
            body: vec![1, 0x60, 4, 0x7f, 0x7f, 0x7f, 0x7f, 1, 0x7f],
        }));
        module.extend(encode_section(&Section {
            id: 3,
            body: encode_index_vector(&[0]),
        }));
        module.extend(encode_section(&Section {
            id: 5,
            body: vec![1, 0, 92],
        }));
        module.extend(encode_section(&Section {
            id: 7,
            body: vec![
                2, 5, b'w', b'o', b'r', b'l', b'd', 0, 0, 6, b'm', b'e', b'm', b'o', b'r', b'y', 2,
                0,
            ],
        }));
        module.extend(encode_section(&Section {
            id: 10,
            body: encode_code(&[body]),
        }));
        module
    }

    #[test]
    fn exact_prefill_certifies_and_other_bytes_refuse() {
        let raw = include_bytes!("../../web/Gw.jspi.wasm");
        let glue = include_bytes!("../../web/Gw.jspi.js");
        let input = super::super::launcher_prefill::rewrite(super::super::Runtime::Jspi, raw, glue)
            .unwrap()
            .unwrap();
        let layout = certify(&input).unwrap();
        assert_eq!(layout.context_root, 2_680_560);
        assert_eq!(layout.character_context_slot, 17);
        assert_eq!(layout.current_map, 564);
        assert_eq!(layout.player_number, 684);
        assert_eq!(layout.character_uuid, 100);
        let mut changed = input;
        changed[8] ^= 1;
        assert!(certify(&changed).is_err());
    }

    #[test]
    fn emitted_reader_is_a_valid_closed_wasm_function() {
        let layout = WorldLayout {
            record_bytes: RECORD_BYTES,
            roster_uuid_offset: ROSTER_UUID_OFFSET,
            context_root: CONTEXT_ROOT,
            character_context_slot: CHARACTER_CONTEXT_SLOT,
            current_map: 564,
            player_number: 684,
            character_uuid: 100,
            map_context_slot: MAP_CONTEXT_SLOT,
            map_player_table: MAP_PLAYER_TABLE,
            map_player_count: MAP_PLAYER_COUNT,
            map_player_stride: MAP_PLAYER_STRIDE,
        };
        let module = fixture(world_entered_reader(layout));
        Validator::new().validate_all(&module).unwrap();
    }
    #[test]
    fn emitted_reader_observes_only_valid_frozen_world_identity() {
        use std::process::Command;

        let layout = WorldLayout {
            record_bytes: RECORD_BYTES,
            roster_uuid_offset: ROSTER_UUID_OFFSET,
            context_root: CONTEXT_ROOT,
            character_context_slot: CHARACTER_CONTEXT_SLOT,
            current_map: CURRENT_MAP,
            player_number: PLAYER_NUMBER,
            character_uuid: CHARACTER_UUID,
            map_context_slot: MAP_CONTEXT_SLOT,
            map_player_table: MAP_PLAYER_TABLE,
            map_player_count: MAP_PLAYER_COUNT,
            map_player_stride: MAP_PLAYER_STRIDE,
        };
        let scratch = crate::scratch::TempDir::new("character-world-reader");
        let wasm = scratch.0.join("world.wasm");
        std::fs::write(&wasm, fixture(world_entered_reader(layout))).unwrap();
        let script = r#"
const fs = require('fs');
const { world, memory } = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(process.argv[1]))).exports;
const words = new Uint32Array(memory.buffer);
const contexts = 0x5a1000, character = 0x5a1200, map = 0x5a1400, table = 0x5a1800;
words[2680560 >>> 2] = contexts; words[(contexts >>> 2) + 17] = character; words[(contexts >>> 2) + 11] = map;
words[(map + 2060) >>> 2] = table; words[(map + 2068) >>> 2] = 8;
words[(character + 564) >>> 2] = 42; words[(character + 684) >>> 2] = 7;
words[(character + 100) >>> 2] = 1; words[(character + 104) >>> 2] = 2; words[(character + 108) >>> 2] = 3; words[(character + 112) >>> 2] = 4;
if (world(1,2,3,4) !== 0) process.exit(1); words[(table + 7 * 80) >>> 2] = 0x5a1c00;
if (world(1,2,3,4) !== 1 || world(1,2,3,5) !== 0 || world(0,0,0,0) !== 0) process.exit(2);
words[(character + 564) >>> 2] = 0; if (world(1,2,3,4) !== 0) process.exit(3);
words[(character + 564) >>> 2] = 42; words[(character + 684) >>> 2] = 0; if (world(1,2,3,4) !== 0) process.exit(4);
words[(character + 684) >>> 2] = 7; words[(contexts >>> 2) + 11] = 0; if (world(1,2,3,4) !== 0) process.exit(5);
words[(contexts >>> 2) + 11] = map; words[(map + 2060) >>> 2] = 0; if (world(1,2,3,4) !== 0) process.exit(6);
words[(map + 2060) >>> 2] = table; words[(character + 684) >>> 2] = 8; if (world(1,2,3,4) !== 0) process.exit(7);
words[(character + 684) >>> 2] = 0x10000000; words[(map + 2068) >>> 2] = 0xffffffff; if (world(1,2,3,4) !== -1) process.exit(8);
words[(character + 684) >>> 2] = 7; words[(map + 2068) >>> 2] = 8; words[2680560 >>> 2] = 0; if (world(1,2,3,4) !== 0) process.exit(9);
words[2680560 >>> 2] = contexts; words[(contexts >>> 2) + 17] = character; if (world(1,2,3,4) !== 1) process.exit(10);
words[(contexts >>> 2) + 17] = 0xffffff00; if (world(1,2,3,4) !== -1) process.exit(11);
"#;
        let status = Command::new("node")
            .arg("-e")
            .arg(script)
            .arg(&wasm)
            .status()
            .unwrap();
        assert!(status.success());
    }
}
