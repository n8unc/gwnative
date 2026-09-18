//! JSPI-only, exact-build preferred-character startup adapter.
//!
//! Adapted from GWoNmac pre-game proof/transform (GPL-3.0-only, commit
//! `004194ff318320f854240fd227462b19889bef24`). Only bounded roster observations
//! and a closed Select/Play queue are exported; game callbacks drain actions.
//! It composes only after reviewed credential-prefill output and refuses every
//! other artifact. No generic UI dispatcher or input path is exported.

use wasmparser::{BinaryReader, ImportSectionReader, TypeRef, Validator};

use super::codec::{
    Section, WASM_HEADER, encode_code, encode_index_vector, encode_section, parse_code,
    parse_index_vector, read_uleb, section_by_id, sleb, split_sections, uleb,
};
use super::{Outcome, digest};

/// Reviewed JSPI input after `launcher_prefill::rewrite` for client build
/// 38,888. The credential transform does not change either list reader.
pub(super) const JSPI_PREFILL_SHA256: &str =
    "e2ad01cc8f3ccb82263399b72ec596f47e9a5a7439e640f7089463cb5abe07db";

pub(super) const ROSTER_COUNT_EXPORT: &str = "GwnativeCharacterRosterCount";
pub(super) const READINESS_EXPORT: &str = "GwnativeCharacterReadiness";
pub(super) const TICK_COUNT_EXPORT: &str = "GwnativeCharacterTickCount";
pub(super) const NAME_AT_EXPORT: &str = "GwnativeCharacterNameAt";
pub(super) const SELECTED_NAME_EXPORT: &str = "GwnativeSelectedCharacterName";
pub(super) const UUID_AT_EXPORT: &str = "GwnativeCharacterUuidAt";

const EXPORTS: [&str; 3] = [ROSTER_COUNT_EXPORT, READINESS_EXPORT, TICK_COUNT_EXPORT];

// These are exact pre-game character-list readers. Their body proof derives
// all three static values below; no frame dispatcher/action proof is reused
// as authority for a control path.
const CHARACTER_LIST_READERS: &[(u32, &str)] = &[
    (
        10_128,
        "87b6b897fd8df7688f46fcaf517b78b1af5334d00e85f4c20d9d9978a4f799e0",
    ),
    (
        10_129,
        "49eb229605004bc69ce17787f6e38d1b453892e740a1b0f3e071d86e4685d6aa",
    ),
];
const CHARACTER_RECORD_BYTES: u32 = 0x84;
const MAX_CHARACTERS: u32 = 64;
const CHARACTER_NAME_OFFSET: u32 = 0x18;
const CHARACTER_NAME_BYTES: u32 = 40;
const TICK_FUNCTION: u32 = 6_661;
const TICK_SLOT: u32 = 1_721;
const TICK_BODY: &str = "4168ff3e2a37bb36a94d1028f8abf0d0cc199115974646ae23aa253655650eea";
const TICK_TYPE: u32 = 2;

#[derive(Clone, Copy)]
struct CharacterListLayout {
    pointer: u32,
    count: u32,
    selected_name: u32,
}

struct Imports {
    functions: u32,
    globals: u32,
}

fn imports(section: &[u8]) -> Outcome<Imports> {
    let reader = ImportSectionReader::new(BinaryReader::new(section, 0))
        .map_err(|error| format!("character-read: imports: {error}"))?;
    let mut result = Imports {
        functions: 0,
        globals: 0,
    };
    for import in reader.into_imports() {
        match import
            .map_err(|error| format!("character-read: import: {error}"))?
            .ty
        {
            TypeRef::Func(_) | TypeRef::FuncExact(_) => {
                result.functions = result
                    .functions
                    .checked_add(1)
                    .ok_or("character-read: too many imports")?;
            }
            TypeRef::Global(_) => {
                result.globals = result
                    .globals
                    .checked_add(1)
                    .ok_or("character-read: too many imports")?;
            }
            _ => {}
        }
    }
    Ok(result)
}

fn function_body<'a>(bodies: &'a [Vec<u8>], imports: u32, index: u32) -> Outcome<&'a [u8]> {
    let local = index
        .checked_sub(imports)
        .ok_or("character-read: function is imported")? as usize;
    bodies
        .get(local)
        .map(Vec::as_slice)
        .ok_or("character-read: function is missing".into())
}

fn operand(body: &[u8], offset: usize) -> Outcome<u32> {
    let mut cursor = offset;
    read_uleb(body, &mut cursor).map_err(|error| format!("character-read: operand: {error}"))
}

fn certify(input: &[u8], bodies: &[Vec<u8>], imports: u32) -> Outcome<CharacterListLayout> {
    Validator::new()
        .validate_all(input)
        .map_err(|error| format!("character-read: invalid input: {error}"))?;
    if digest(input) != JSPI_PREFILL_SHA256 {
        return Err(format!(
            "character-read: unsupported input {}",
            digest(input)
        ));
    }
    for &(index, expected) in CHARACTER_LIST_READERS {
        let actual = digest(function_body(bodies, imports, index)?);
        if actual != expected {
            return Err(format!(
                "character-read: certified function {index} changed (expected {expected}, got {actual})"
            ));
        }
    }
    let character_array = function_body(bodies, imports, CHARACTER_LIST_READERS[0].0)?;
    let selected = function_body(bodies, imports, CHARACTER_LIST_READERS[1].0)?;
    let layout = CharacterListLayout {
        // These byte positions are the source proof's bounded ULEB operands:
        // count and pointer share a verified eight-byte relation; selected is
        // later static and therefore cannot alias either list scalar.
        count: operand(character_array, 9)?,
        pointer: operand(character_array, 23)?,
        selected_name: operand(selected, 33)?,
    };
    if layout.count
        != layout
            .pointer
            .checked_add(8)
            .ok_or("character-read: layout overflow")?
        || layout.selected_name <= layout.count
    {
        return Err("character-read: character-list layout relationship changed".into());
    }
    Ok(layout)
}

fn type_count(section: &[u8]) -> Outcome<u32> {
    let mut cursor = 0;
    let count = read_uleb(section, &mut cursor)?;
    for _ in 0..count {
        if section.get(cursor) != Some(&0x60) {
            return Err("character-read: non-function type".into());
        }
        cursor += 1;
        for _ in 0..2 {
            let values = read_uleb(section, &mut cursor)? as usize;
            cursor = cursor
                .checked_add(values)
                .filter(|end| *end <= section.len())
                .ok_or("character-read: truncated type")?;
        }
    }
    if cursor != section.len() {
        return Err("character-read: malformed type section".into());
    }
    Ok(count)
}

fn has_exports(section: &[u8]) -> Outcome<bool> {
    let mut cursor = 0;
    let count = read_uleb(section, &mut cursor)?;
    for _ in 0..count {
        let length = read_uleb(section, &mut cursor)? as usize;
        let end = cursor
            .checked_add(length)
            .filter(|end| *end <= section.len())
            .ok_or("character-read: truncated export")?;
        if EXPORTS
            .iter()
            .any(|name| section[cursor..end] == *name.as_bytes())
        {
            return Ok(true);
        }
        cursor = end
            .checked_add(1)
            .ok_or("character-read: truncated export")?;
        let _ = read_uleb(section, &mut cursor)?;
    }
    if cursor != section.len() {
        return Err("character-read: malformed export section".into());
    }
    Ok(false)
}

fn read_sleb(bytes: &[u8], cursor: &mut usize) -> Outcome<i32> {
    let mut result = 0i32;
    let mut shift = 0;
    for _ in 0..5 {
        let byte = *bytes
            .get(*cursor)
            .ok_or("character-read: truncated signed LEB")?;
        *cursor += 1;
        result |= i32::from(byte & 0x7f) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            if shift < 32 && byte & 0x40 != 0 {
                result |= (!0i32) << shift;
            }
            return Ok(result);
        }
    }
    Err("character-read: oversized signed LEB".into())
}

/// Current client uses one active funcref segment. Parse it rather than using
/// a guessed table offset; duplicate or passive segments refuse this wrapper.
fn certified_tick_slot(section: &[u8]) -> Outcome<()> {
    let mut cursor = 0;
    let segments = read_uleb(section, &mut cursor)?;
    let mut found = None;
    for _ in 0..segments {
        if read_uleb(section, &mut cursor)? != 0 || section.get(cursor) != Some(&0x41) {
            return Err("character-read: unsupported callback element segment".into());
        }
        cursor += 1;
        let base = read_sleb(section, &mut cursor)?;
        if base < 0 || section.get(cursor) != Some(&0x0b) {
            return Err("character-read: malformed callback element offset".into());
        }
        cursor += 1;
        let entries = read_uleb(section, &mut cursor)?;
        for offset in 0..entries {
            let function = read_uleb(section, &mut cursor)?;
            if base.checked_add(offset as i32) == Some(TICK_SLOT as i32) {
                if found.replace(function).is_some() {
                    return Err("character-read: duplicate callback table slot".into());
                }
            }
        }
    }
    if cursor != section.len() {
        return Err("character-read: malformed callback element section".into());
    }
    if found == Some(TICK_FUNCTION) {
        Ok(())
    } else {
        Err("character-read: callback table slot changed".into())
    }
}

fn append_private_globals(section: &[u8], additional: u32) -> Outcome<(Vec<u8>, u32)> {
    let mut cursor = 0;
    let count = read_uleb(section, &mut cursor)?;
    let next = count
        .checked_add(additional)
        .ok_or("character-read: global count overflow")?;
    let mut output = uleb(u64::from(next));
    output.extend_from_slice(&section[cursor..]);
    for _ in 0..additional {
        // i32, mutable, i32.const 0, end
        output.extend_from_slice(&[0x7f, 0x01, 0x41, 0x00, 0x0b]);
    }
    Ok((output, count))
}

fn i32(value: u32) -> Vec<u8> {
    let mut out = vec![0x41];
    out.extend(sleb(value as i64));
    out
}
fn load(offset: u32) -> Vec<u8> {
    let mut out = vec![0x28, 0x02];
    out.extend(uleb(u64::from(offset)));
    out
}
fn local_get(index: u32) -> Vec<u8> {
    vec![0x20, index as u8]
}
fn local_set(index: u32) -> Vec<u8> {
    vec![0x21, index as u8]
}
fn scalar_reader(address: u32) -> Vec<u8> {
    let mut body = vec![0x00];
    body.extend(i32(address));
    body.extend(load(0));
    body.push(0x0b);
    body
}

/// Returns private callback tick count as a closed diagnostic.
fn tick_reader(global: u32) -> Vec<u8> {
    let mut body = vec![0x00, 0x23];
    body.extend(uleb(u64::from(global)));
    body.push(0x0b);
    body
}

/// Replacement for certified event-2 callback. It increments private
/// diagnostic state then invokes an appended byte-for-byte original once with
/// the exact two parameters; no action dispatcher is reachable here.
fn tick_wrapper(global: u32, original: u32) -> Vec<u8> {
    let mut body = vec![0x00, 0x23];
    body.extend(uleb(u64::from(global)));
    body.extend([0x41, 0x01, 0x6a, 0x24]);
    body.extend(uleb(u64::from(global)));
    body.extend([0x20, 0x00, 0x20, 0x01, 0x10]);
    body.extend(uleb(u64::from(original)));
    body.push(0x0b);
    body
}

fn tick_action_wrapper(global: u32, original: u32, drain: u32) -> Vec<u8> {
    let mut body = tick_wrapper(global, original);
    body.pop();
    body.push(0x10);
    body.extend(uleb(u64::from(drain)));
    body.push(0x0b);
    body
}

/// Returns direct UTF-16 storage for one bounded roster row, or zero. Caller
/// has no record/heap primitive and must read at most 20 units from result.
/// Returns direct storage for a bounded row's 16-byte UUID, or zero. The
/// row and complete UUID must fit current memory; callers receive no record
/// base or arbitrary offset.
fn uuid_at_reader(layout: CharacterListLayout) -> Vec<u8> {
    const UUID_OFFSET: u32 = 8;
    const UUID_BYTES: u32 = 16;
    let mut body = vec![0x01, 0x03, 0x7f]; // roster pointer, count, memory bytes
    body.extend(i32(layout.count));
    body.extend(load(0));
    body.extend(local_set(2));
    body.extend(local_get(0));
    body.extend(local_get(2));
    body.push(0x4f);
    body.extend([0x04, 0x40, 0x41, 0x00, 0x0f, 0x0b]);
    body.extend(i32(layout.pointer));
    body.extend(load(0));
    body.extend(local_set(1));
    body.extend(local_get(1));
    body.push(0x45);
    body.extend([0x04, 0x40, 0x41, 0x00, 0x0f, 0x0b]);
    body.extend([0x3f, 0x00]);
    body.extend(i32(65_536));
    body.push(0x6c);
    body.extend(local_set(3));
    body.extend(local_get(1));
    body.extend(local_get(3));
    body.extend(local_get(2));
    body.extend(i32(CHARACTER_RECORD_BYTES));
    body.extend([0x6c, 0x6b, 0x4b, 0x04, 0x40, 0x41, 0x00, 0x0f, 0x0b]);
    body.extend(local_get(1));
    body.extend(local_get(0));
    body.extend(i32(CHARACTER_RECORD_BYTES));
    body.extend([0x6c, 0x6a]);
    body.extend(i32(UUID_OFFSET));
    body.push(0x6a);
    body.extend(local_set(1));
    body.extend(local_get(1));
    body.extend(local_get(3));
    body.extend(i32(UUID_BYTES));
    body.extend([0x6b, 0x4b, 0x04, 0x40, 0x41, 0x00, 0x0f, 0x0b]);
    body.extend(local_get(1));
    body.push(0x0b);
    body
}

fn name_at_reader(layout: CharacterListLayout) -> Vec<u8> {
    let mut body = vec![0x01, 0x03, 0x7f]; // roster pointer, count, memory bytes
    body.extend(i32(layout.count));
    body.extend(load(0));
    body.extend(local_set(2));
    body.extend(local_get(0));
    body.extend(local_get(2));
    body.push(0x4f);
    body.extend([0x04, 0x40, 0x41, 0x00, 0x0f, 0x0b]);
    body.extend(i32(layout.pointer));
    body.extend(load(0));
    body.extend(local_set(1));
    body.extend(local_get(1));
    body.push(0x45);
    body.extend([0x04, 0x40, 0x41, 0x00, 0x0f, 0x0b]);
    body.extend([0x3f, 0x00]);
    body.extend(i32(65_536));
    body.push(0x6c);
    body.extend(local_set(3));
    body.extend(local_get(1));
    body.extend(local_get(3));
    body.extend(local_get(2));
    body.extend(i32(CHARACTER_RECORD_BYTES));
    body.extend([0x6c, 0x6b, 0x4b, 0x04, 0x40, 0x41, 0x00, 0x0f, 0x0b]);
    body.extend(local_get(1));
    body.extend(local_get(0));
    body.extend(i32(CHARACTER_RECORD_BYTES));
    body.extend([0x6c, 0x6a]);
    body.extend(i32(CHARACTER_NAME_OFFSET));
    body.push(0x6a);
    body.extend(local_set(1));
    body.extend(local_get(1));
    body.extend(local_get(3));
    body.extend(i32(CHARACTER_NAME_BYTES));
    body.extend([0x6b, 0x4b, 0x04, 0x40, 0x41, 0x00, 0x0f, 0x0b]);
    body.extend(local_get(1));
    body.push(0x0b);
    body
}

/// Return a closed diagnostic: bit 0 is a bounded live roster, bit 1 is a
/// non-empty selected-name buffer. It never returns roster records or names.
fn readiness_reader(layout: CharacterListLayout) -> Vec<u8> {
    let mut body = vec![0x01, 0x02, 0x7f]; // pointer, count
    // count must be 1..=64. This is readiness only; zero is deliberately not
    // treated as a usable roster while character selection is still loading.
    body.extend(i32(layout.count));
    body.extend(load(0));
    body.extend(local_set(1));
    body.extend(local_get(1));
    body.extend(i32(1));
    body.push(0x49); // lt_u
    body.extend(local_get(1));
    body.extend(i32(MAX_CHARACTERS));
    body.push(0x4b); // gt_u
    body.push(0x72); // or
    body.extend([0x04, 0x40, 0x41, 0x00, 0x0f, 0x0b]);
    // pointer must fit all fixed-size records before any selected status is
    // reported. Memory is queried in Wasm; page receives no heap primitive.
    body.extend(i32(layout.pointer));
    body.extend(load(0));
    body.extend(local_set(0));
    body.extend(local_get(0));
    body.push(0x45);
    body.extend([0x04, 0x40, 0x41, 0x00, 0x0f, 0x0b]);
    body.extend(local_get(0));
    body.extend([0x3f, 0x00]);
    body.extend(i32(65_536));
    body.push(0x6c);
    body.extend(local_get(1));
    body.extend(i32(CHARACTER_RECORD_BYTES));
    body.push(0x6c);
    body.push(0x6b);
    body.push(0x4b); // pointer > memory - required bytes
    body.extend([0x04, 0x40, 0x41, 0x00, 0x0f, 0x0b]);
    // selected-name location is certified by second reader. Reading its first
    // UTF-16 unit is enough for a presence bit and cannot reveal identity.
    body.extend(i32(layout.selected_name));
    body.extend(vec![0x2f, 0x01, 0x00]);
    body.push(0x45);
    // Roster proof already passed, so set bit 0 in both branches and bit 1
    // only for a non-empty selected buffer.
    body.extend([0x45, 0x04, 0x7f, 0x41, 0x03, 0x05, 0x41, 0x01, 0x0b]);
    body.push(0x0b);
    body
}

fn replace(sections: &mut [Section], id: u8, body: Vec<u8>) -> Outcome<()> {
    sections
        .iter_mut()
        .find(|section| section.id == id)
        .map(|section| section.body = body)
        .ok_or_else(|| format!("character-read: missing section {id}"))
}

/// Append two closed read-only exports to exact JSPI prefill output. No caller
/// can select a row, dispatch Play, or obtain roster/name memory.
pub(super) fn rewrite_jspi(input: &[u8]) -> Outcome<Vec<u8>> {
    let mut sections = split_sections(input)?;
    let imports = imports(section_by_id(&sections, 2)?)?;
    let mut functions = parse_index_vector(section_by_id(&sections, 3)?)?;
    let mut bodies = parse_code(section_by_id(&sections, 10)?)?;
    if functions.len() != bodies.len() {
        return Err("character-read: function/code count differs".into());
    }
    let layout = certify(input, &bodies, imports.functions)?;
    let world_layout = super::character_world::certify(input)?;
    certified_tick_slot(section_by_id(&sections, 9)?)?;
    let tick_local = TICK_FUNCTION
        .checked_sub(imports.functions)
        .ok_or("character-read: callback is imported")? as usize;
    if functions.get(tick_local) != Some(&TICK_TYPE)
        || digest(
            bodies
                .get(tick_local)
                .ok_or("character-read: callback missing")?,
        ) != TICK_BODY
    {
        return Err("character-read: callback proof changed".into());
    }
    if has_exports(section_by_id(&sections, 7)?)? {
        return Err("character-read: export collision".into());
    }
    let types = type_count(section_by_id(&sections, 1)?)?;
    let type_section = section_by_id(&sections, 1)?;
    let mut cursor = 0;
    let _ = read_uleb(type_section, &mut cursor)?;
    let mut next_types = uleb(u64::from(types + 5));
    next_types.extend_from_slice(&type_section[cursor..]);
    next_types.extend_from_slice(&[0x60, 0x00, 0x01, 0x7f]); // () -> i32
    next_types.extend_from_slice(&[0x60, 0x01, 0x7f, 0x01, 0x7f]); // (i32) -> i32
    next_types.extend_from_slice(&[0x60, 0x04, 0x7f, 0x7f, 0x7f, 0x7f, 0x01, 0x7f]); // (i32, i32, i32, i32) -> i32
    next_types.extend_from_slice(&[0x60, 0x00, 0x00]); // () -> ()
    next_types.extend_from_slice(&[0x60, 0x02, 0x7f, 0x7f, 0x01, 0x7f]); // (i32, i32) -> i32
    let plan = super::character_actions::action_plan(input)?;
    let (globals, defined_globals) =
        append_private_globals(section_by_id(&sections, 6)?, plan.private_globals + 1)?;
    let tick_global = imports
        .globals
        .checked_add(defined_globals)
        .ok_or("character-read: global index overflow")?;
    let action_globals = super::character_actions::ActionGlobals {
        pending: tick_global + 1,
        argument: tick_global + 2,
        enabled: tick_global + 3,
        result: tick_global + 4,
        expected: tick_global + 5,
        attempts: tick_global + 6,
        stage: tick_global + 7,
        uuid0: tick_global + 8,
        uuid1: tick_global + 9,
        uuid2: tick_global + 10,
        uuid3: tick_global + 11,
        target_set: tick_global + 12,
        selected_name: tick_global + 13,
    };
    let first = imports.functions + functions.len() as u32;
    functions.extend(std::iter::repeat_n(types, EXPORTS.len()));
    bodies.extend([
        scalar_reader(layout.count),
        readiness_reader(layout),
        tick_reader(tick_global),
    ]);
    let name_at = imports.functions + functions.len() as u32;
    functions.push(types + 1);
    bodies.push(name_at_reader(layout));
    let selected_name = imports.functions + functions.len() as u32;
    functions.push(types);
    bodies.push(super::character_actions::emit_selected_name(action_globals));
    let uuid_at = imports.functions + functions.len() as u32;
    functions.push(types + 1);
    bodies.push(uuid_at_reader(layout));
    let world_entered = imports.functions + functions.len() as u32;
    functions.push(types + 2);
    bodies.push(super::character_world::world_entered_reader(world_layout));
    let enqueue = imports.functions + functions.len() as u32;
    functions.push(types + 4);
    bodies.push(super::character_actions::emit_enqueue(action_globals));
    let cancel = imports.functions + functions.len() as u32;
    functions.push(types);
    bodies.push(super::character_actions::emit_cancel(action_globals));
    let configure = imports.functions + functions.len() as u32;
    functions.push(types + 1);
    bodies.push(super::character_actions::emit_configure(action_globals));
    let status = imports.functions + functions.len() as u32;
    functions.push(types);
    bodies.push(super::character_actions::emit_status(action_globals));
    let stage = imports.functions + functions.len() as u32;
    functions.push(types);
    bodies.push(super::character_actions::emit_stage(action_globals));
    let ui_ready = imports.functions + functions.len() as u32;
    functions.push(types);
    bodies.push(super::character_actions::emit_ui_ready(plan.proof));
    let target = imports.functions + functions.len() as u32;
    functions.push(types + 2);
    bodies.push(super::character_actions::emit_target(action_globals));
    let selector = imports.functions + functions.len() as u32;
    functions.push(types + 4);
    bodies.push(super::character_selector::emit_selector_execute(
        plan.proof,
        action_globals,
    ));
    let current_selector_name = imports.functions + functions.len() as u32;
    functions.push(types);
    bodies.push(super::character_actions::emit_current_selector_name(
        plan.proof,
    ));
    let play = imports.functions + functions.len() as u32;
    functions.push(types + 4);
    bodies.push(super::character_actions::emit_play_execute(
        plan.proof,
        action_globals,
        current_selector_name,
    ));
    let execute = imports.functions + functions.len() as u32;
    functions.push(types + 4);
    bodies.push(super::character_actions::emit_execute(
        selector,
        play,
        action_globals,
    ));
    let drain = imports.functions + functions.len() as u32;
    functions.push(types + 3);
    bodies.push(super::character_actions::emit_drain(
        action_globals,
        execute,
    ));
    let original = imports.functions + functions.len() as u32;
    functions.push(TICK_TYPE);
    bodies.push(bodies[tick_local].clone());
    functions.push(TICK_TYPE);
    bodies.push(tick_action_wrapper(tick_global, original, drain));
    bodies[tick_local] = tick_action_wrapper(tick_global, original, drain);
    let export_section = section_by_id(&sections, 7)?;
    let mut export_cursor = 0;
    let count = read_uleb(export_section, &mut export_cursor)?;
    let mut next_exports = uleb(u64::from(count + EXPORTS.len() as u32 + 11));
    next_exports.extend_from_slice(&export_section[export_cursor..]);
    for (offset, name) in EXPORTS.iter().enumerate() {
        next_exports.extend(uleb(name.len() as u64));
        next_exports.extend(name.as_bytes());
        next_exports.push(0);
        next_exports.extend(uleb(u64::from(first + offset as u32)));
    }
    for (name, index) in [
        (NAME_AT_EXPORT, name_at),
        (SELECTED_NAME_EXPORT, selected_name),
        (UUID_AT_EXPORT, uuid_at),
        (super::character_world::WORLD_ENTERED_EXPORT, world_entered),
        (super::character_actions::CHARACTER_ACTION_EXPORT, enqueue),
        (
            super::character_actions::CHARACTER_ACTION_CANCEL_EXPORT,
            cancel,
        ),
        (
            super::character_actions::CHARACTER_ACTION_CONFIGURE_EXPORT,
            configure,
        ),
        (
            super::character_actions::CHARACTER_ACTION_STATUS_EXPORT,
            status,
        ),
        (
            super::character_actions::CHARACTER_ACTION_STAGE_EXPORT,
            stage,
        ),
        (
            super::character_actions::CHARACTER_UI_READY_EXPORT,
            ui_ready,
        ),
        (
            super::character_actions::CHARACTER_ACTION_TARGET_EXPORT,
            target,
        ),
    ] {
        next_exports.extend(uleb(name.len() as u64));
        next_exports.extend(name.as_bytes());
        next_exports.push(0);
        next_exports.extend(uleb(u64::from(index)));
    }
    replace(&mut sections, 1, next_types)?;
    replace(&mut sections, 3, encode_index_vector(&functions))?;
    replace(&mut sections, 6, globals)?;
    replace(&mut sections, 7, next_exports)?;
    replace(&mut sections, 10, encode_code(&bodies))?;
    let mut output = WASM_HEADER.to_vec();
    for section in &sections {
        output.extend(encode_section(section));
    }
    Validator::new()
        .validate_all(&output)
        .map_err(|error| format!("character-read: invalid output: {error}"))?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_jspi_prefill_composes_only_after_exact_function_proofs() {
        let raw = include_bytes!("../../web/Gw.jspi.wasm");
        let glue = include_bytes!("../../web/Gw.jspi.js");
        let prefill =
            super::super::launcher_prefill::rewrite(super::super::Runtime::Jspi, raw, glue)
                .unwrap()
                .unwrap();
        assert_eq!(digest(&prefill), JSPI_PREFILL_SHA256);
        let output = rewrite_jspi(&prefill).unwrap();
        Validator::new().validate_all(&output).unwrap();
        let before = split_sections(&prefill).unwrap();
        let after = split_sections(&output).unwrap();
        let before_functions = parse_index_vector(section_by_id(&before, 3).unwrap()).unwrap();
        let after_functions = parse_index_vector(section_by_id(&after, 3).unwrap()).unwrap();
        let before_bodies = parse_code(section_by_id(&before, 10).unwrap()).unwrap();
        let after_bodies = parse_code(section_by_id(&after, 10).unwrap()).unwrap();
        let imports = imports(section_by_id(&before, 2).unwrap()).unwrap();
        let tick_local = (TICK_FUNCTION - imports.functions) as usize;
        // Three readers, byte-for-byte original, and wrapper are appended;
        // active table keeps f6661 and therefore now resolves wrapper.
        assert_eq!(after_functions.len(), before_functions.len() + 21);
        assert_eq!(after_bodies.len(), before_bodies.len() + 21);
        assert_ne!(after_bodies[tick_local], before_bodies[tick_local]);
        assert_eq!(
            after_bodies[after_bodies.len() - 2],
            before_bodies[tick_local]
        );
        certified_tick_slot(section_by_id(&after, 9).unwrap()).unwrap();
        assert!(has_exports(section_by_id(&after, 7).unwrap()).unwrap());
    }

    #[test]
    fn uuid_pointer_reader_is_bounded_and_returns_only_uuid_storage() {
        use std::process::Command;

        let body = uuid_at_reader(CharacterListLayout {
            pointer: 4,
            count: 8,
            selected_name: 12,
        });
        let mut module = WASM_HEADER.to_vec();
        module.extend(encode_section(&Section {
            id: 1,
            body: vec![1, 0x60, 1, 0x7f, 1, 0x7f],
        }));
        module.extend(encode_section(&Section {
            id: 3,
            body: encode_index_vector(&[0]),
        }));
        module.extend(encode_section(&Section {
            id: 5,
            body: vec![1, 0, 1],
        }));
        module.extend(encode_section(&Section {
            id: 7,
            body: vec![
                2, 4, b'u', b'u', b'i', b'd', 0, 0, 6, b'm', b'e', b'm', b'o', b'r', b'y', 2, 0,
            ],
        }));
        module.extend(encode_section(&Section {
            id: 10,
            body: encode_code(&[body]),
        }));
        Validator::new().validate_all(&module).unwrap();
        let scratch = crate::scratch::TempDir::new("character-uuid-pointer");
        let wasm = scratch.0.join("uuid.wasm");
        std::fs::write(&wasm, module).unwrap();
        let script = r#"
const fs = require('fs');
const {uuid, memory} = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(process.argv[1]))).exports;
const words = new Uint32Array(memory.buffer); words[1] = 64; words[2] = 1;
words[18] = 1; words[19] = 2; words[20] = 3; words[21] = 4;
if (uuid(0) !== 72 || uuid(1) !== 0) process.exit(1);
words[1] = 65500; if (uuid(0) !== 0) process.exit(2);
"#;
        assert!(
            Command::new("node")
                .arg("-e")
                .arg(script)
                .arg(&wasm)
                .status()
                .unwrap()
                .success()
        );
    }

    #[test]
    fn unreviewed_or_malformed_input_is_refused() {
        assert!(rewrite_jspi(b"not wasm").is_err());
    }
}
