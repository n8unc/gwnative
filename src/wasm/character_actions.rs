//! Exact-build, callback-drained preferred-character action queue.
//!
//! GPL-3.0-only lineage: GWoNmac's bounded pre-game character-switch
//! transform, commit `004194ff318320f854240fd227462b19889bef24`.
//! This module owns no WebView, DOM, keyboard, or generic heap interface.
//! It certifies the post-prefill JSPI module before exposing bytecode emitters
//! for a single, callback-drained Select or Play request.

use wasmparser::{BinaryReader, ImportSectionReader, TypeRef, Validator};

use super::codec::{parse_code, section_by_id, split_sections, uleb};
use super::{Outcome, digest};

pub(super) const CHARACTER_ACTION_EXPORT: &str = "GwnativeCharacterAction";
pub(super) const CHARACTER_ACTION_CANCEL_EXPORT: &str = "GwnativeCharacterActionCancel";
pub(super) const CHARACTER_ACTION_CONFIGURE_EXPORT: &str = "GwnativeCharacterActionConfigure";
pub(super) const CHARACTER_ACTION_STATUS_EXPORT: &str = "GwnativeCharacterActionStatus";
pub(super) const CHARACTER_ACTION_STAGE_EXPORT: &str = "GwnativeCharacterActionStage";
pub(super) const CHARACTER_UI_READY_EXPORT: &str = "GwnativeCharacterUiReady";
pub(super) const CHARACTER_ACTION_TARGET_EXPORT: &str = "GwnativeCharacterActionTarget";

const JSPI_PREFILL_SHA256: &str =
    "e2ad01cc8f3ccb82263399b72ec596f47e9a5a7439e640f7089463cb5abe07db";
const SELECT: u32 = 1;
const PLAY: u32 = 2;
const MAX_CHARACTERS: u32 = 64;

/// Static proof for every game-owned function an eventual bounded action
/// executor may call. Prefill changes only f10797/f11302 and appends two
/// setters; it preserves these original function indexes and bodies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ActionProof {
    pub selector_hash: u32,
    pub play_hash: u32,
    pub frame_child: u32,
    pub frame_parent: u32,
    pub frame_resolver: u32,
    pub frame_dispatch: u32,
    pub logout_producer: u32,
    pub frame_dispatch_offset: u32,
    pub frame_array: u32,
    pub frame_count: u32,
    pub callback_rows_offset: u32,
    pub callback_count_offset: u32,
    pub callback_row_bytes: u32,
    pub callback_context_offset: u32,
    pub selector_context_rows_offset: u32,
    pub selector_context_count_offset: u32,
    pub selector_row_name_offset: u32,
    pub selector_index_message: u32,
    pub selector_callback_slot: u32,
}

const FUNCTION_PROOFS: &[(u32, &str)] = &[
    (
        6_796,
        "9f73f1018d0bf99fd0d16b6ede0921dbe29cf70a4da4a61c9b24c1e68dbb0bf0",
    ),
    (
        6_797,
        "46c90817c6ab335d5b8d57fdc1e38abd146c2b123dd8dcf0f08aca8245b8a9f2",
    ),
    (
        6_534,
        "f0d5e7c4c71f920541037b1225613e334e2476723a427cab5c2688538265eb47",
    ),
    (
        6_508,
        "ccf496f855fa579dac0d1ea86b95b6a6db21104d2a41b1d03c6bd213ee26ca7e",
    ),
    (
        12_434,
        "b618abba3579ffe6f149a23e2550f6b86571b0f3beaa30acea059153f7cd6b06",
    ),
];

/// Seven private mutable i32 globals allocated by character.rs. `expected` and
/// `attempts` make a delayed Selector click confirmable without re-clicking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ActionGlobals {
    pub pending: u32,
    pub argument: u32,
    pub enabled: u32,
    pub result: u32,
    pub expected: u32,
    pub attempts: u32,
    /// Closed diagnostic phase only; never an address, identity, or count.
    pub stage: u32,
    pub uuid0: u32,
    pub uuid1: u32,
    pub uuid2: u32,
    pub uuid3: u32,
    pub target_set: u32,
    pub selected_name: u32,
}

/// Certified plan, deliberately separate from section mutation. Character.rs
/// supplies appended function indexes after it has selected exact Wasm types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ActionPlan {
    pub proof: ActionProof,
    pub private_globals: u32,
}

fn imported_functions(section: &[u8]) -> Outcome<u32> {
    let reader = ImportSectionReader::new(BinaryReader::new(section, 0))
        .map_err(|error| format!("character-action: imports: {error}"))?;
    let mut functions = 0u32;
    for entry in reader.into_imports() {
        if matches!(
            entry
                .map_err(|error| format!("character-action: import: {error}"))?
                .ty,
            TypeRef::Func(_) | TypeRef::FuncExact(_)
        ) {
            functions = functions
                .checked_add(1)
                .ok_or("character-action: too many imports")?;
        }
    }
    Ok(functions)
}

/// Fail closed before an action transform can reuse any derived location.
pub(super) fn action_plan(input: &[u8]) -> Outcome<ActionPlan> {
    Validator::new()
        .validate_all(input)
        .map_err(|error| format!("character-action: invalid input: {error}"))?;
    if digest(input) != JSPI_PREFILL_SHA256 {
        return Err("character-action: unsupported input".into());
    }
    let sections = split_sections(input)?;
    let imports = imported_functions(section_by_id(&sections, 2)?)?;
    let bodies = parse_code(section_by_id(&sections, 10)?)?;
    for &(function, expected) in FUNCTION_PROOFS {
        let local = function
            .checked_sub(imports)
            .ok_or("character-action: imported proof")? as usize;
        let actual = bodies.get(local).ok_or("character-action: missing proof")?;
        if digest(actual) != expected {
            return Err(format!(
                "character-action: certified function {function} changed"
            ));
        }
    }
    Ok(ActionPlan {
        proof: super::character_action_proof::certify(input)?,
        private_globals: 13,
    })
}

fn get_global(index: u32) -> Vec<u8> {
    let mut out = vec![0x23];
    out.extend(uleb(index as u64));
    out
}
fn set_global(index: u32) -> Vec<u8> {
    let mut out = vec![0x24];
    out.extend(uleb(index as u64));
    out
}
fn get_local(index: u32) -> Vec<u8> {
    let mut out = vec![0x20];
    out.extend(uleb(index as u64));
    out
}
fn i32(value: i64) -> Vec<u8> {
    let mut out = vec![0x41];
    out.extend(super::codec::sleb(value));
    out
}
fn load(offset: u32) -> Vec<u8> {
    let mut out = vec![0x28, 0x02];
    out.extend(uleb(offset as u64));
    out
}
fn load16(offset: u32) -> Vec<u8> {
    let mut out = vec![0x2f, 0x01];
    out.extend(uleb(offset as u64));
    out
}
fn store(offset: u32) -> Vec<u8> {
    let mut out = vec![0x36, 0x02];
    out.extend(uleb(offset as u64));
    out
}
fn set_local(index: u32) -> Vec<u8> {
    let mut out = vec![0x21];
    out.extend(uleb(index as u64));
    out
}

fn stage(out: &mut Vec<u8>, globals: ActionGlobals, value: i64) {
    out.extend(i32(value));
    out.extend(set_global(globals.stage));
}

/// Emits bounded `Play` on game callback. It rechecks roster readiness and
/// derives live Play frame from certified registry/hash, then builds the fixed
/// 40-byte packet on Wasm stack global zero. No page pointer crosses boundary.
pub(super) fn emit_play_execute(
    proof: ActionProof,
    globals: ActionGlobals,
    current_selector_name: u32,
) -> Vec<u8> {
    // params kind,index; locals stack,count,table,index,frame,parent-id,parent,base-stack,roster
    const STACK: u32 = 2;
    const COUNT: u32 = 3;
    const TABLE: u32 = 4;
    const INDEX: u32 = 5;
    const FRAME: u32 = 6;
    const PARENT_ID: u32 = 7;
    const PARENT: u32 = 8;
    const BASE_STACK: u32 = 9;
    const LIVE_NAME: u32 = 10;
    let mut out = vec![0x01, 0x0a, 0x7f];
    let early = |out: &mut Vec<u8>| {
        out.extend(i32(-2));
        out.extend([0x0f]);
    };
    stage(&mut out, globals, 20);
    out.extend(get_local(0));
    out.extend(i32(PLAY as i64));
    out.push(0x47);
    out.extend([0x04, 0x40]);
    early(&mut out);
    out.push(0x0b);
    stage(&mut out, globals, 21);
    out.extend(get_local(1));
    out.push(0x45);
    out.push(0x45);
    out.extend([0x04, 0x40]);
    early(&mut out);
    out.push(0x0b);
    stage(&mut out, globals, 22);
    // All fixed reads below require this exact observed memory span. Check it
    // before dereferencing a client-owned pointer or deriving a subtraction.
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(i32(0x5a75f4));
    out.push(0x49);
    out.extend([0x04, 0x40]);
    early(&mut out);
    out.push(0x0b);
    stage(&mut out, globals, 23);
    // Fresh bounded roster index/count check immediately before action.
    out.extend(i32(0x5a75f0));
    out.extend(load(0));
    out.extend(set_local(COUNT));
    out.extend(get_local(COUNT));
    out.extend(i32(1));
    out.push(0x49);
    out.extend(get_local(COUNT));
    out.extend(i32(64));
    out.push(0x4b);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    early(&mut out);
    out.push(0x0b);
    // Query current Selector state on this callback, never the stale
    // last-played buffer. The resolver returns a bounded UTF-16z row name.
    out.push(0x10);
    out.extend(uleb(current_selector_name as u64));
    out.extend(set_local(LIVE_NAME));
    out.extend(get_local(LIVE_NAME));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    early(&mut out);
    out.push(0x0b);
    // Fresh live name plus UUID uniqueness check. The frozen target must
    // still be the one highlighted Selector row before Play is touched.
    out.extend(i32(0x5a75e8));
    out.extend(load(0));
    out.extend(set_local(PARENT));
    out.extend(get_local(PARENT));
    out.push(0x45);
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(get_local(COUNT));
    out.extend(i32(0x84));
    out.push(0x6c);
    out.push(0x49);
    out.push(0x72);
    out.extend(get_local(PARENT));
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(get_local(COUNT));
    out.extend(i32(0x84));
    out.push(0x6c);
    out.push(0x6b);
    out.push(0x4b);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    early(&mut out);
    out.push(0x0b);
    // A selected name is a bounded, non-empty UTF-16z field. Later bytes
    // after its terminator are not part of its identity.
    out.extend(get_local(LIVE_NAME));
    out.extend(load16(0));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    early(&mut out);
    out.push(0x0b);
    out.extend(i32(0));
    out.extend(set_local(INDEX));
    out.extend(i32(0));
    out.extend(set_local(PARENT_ID));
    out.extend(i32(0));
    out.extend(set_local(BASE_STACK));
    out.extend(i32(0));
    out.extend(set_local(STACK));
    out.extend([0x02, 0x40, 0x03, 0x40]);
    out.extend(get_local(INDEX));
    out.extend(get_local(COUNT));
    out.push(0x4f);
    out.extend([0x0d, 0x01]);
    out.extend(get_local(PARENT));
    out.extend(get_local(INDEX));
    out.extend(i32(0x84));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(set_local(TABLE));
    // `FRAME` is a bounded UTF-16 unit cursor; `PARENT_ID` counts matches.
    // The enclosing row block makes a name mismatch skip UUID evaluation.
    out.extend(i32(0));
    out.extend(set_local(FRAME));
    out.extend([0x02, 0x40, 0x02, 0x40, 0x03, 0x40]);
    out.extend(get_local(FRAME));
    out.extend(i32(20));
    out.push(0x4f);
    // No UTF-16 terminator within 20 units is not a matching live name.
    out.extend([0x0d, 0x02]);
    out.extend(get_local(TABLE));
    out.extend(get_local(FRAME));
    out.extend(i32(2));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(load16(24));
    out.extend(get_local(LIVE_NAME));
    out.extend(get_local(FRAME));
    out.extend(i32(2));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(load16(0));
    out.push(0x47);
    out.extend([0x04, 0x40, 0x0c, 0x03, 0x0b]);
    out.extend(get_local(LIVE_NAME));
    out.extend(get_local(FRAME));
    out.extend(i32(2));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(load16(0));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    out.extend(i32(1));
    out.extend(set_local(STACK));
    // Exit inner block after verified matching terminator.
    out.extend([0x0c, 0x02, 0x0b]);
    out.extend(get_local(FRAME));
    out.extend(i32(1));
    out.push(0x6a);
    out.extend(set_local(FRAME));
    out.extend([0x0c, 0x00, 0x0b, 0x0b]);
    for (offset, uuid) in [
        (8, globals.uuid0),
        (12, globals.uuid1),
        (16, globals.uuid2),
        (20, globals.uuid3),
    ] {
        out.extend(get_local(TABLE));
        out.extend(load(offset));
        out.extend(get_global(uuid));
        out.push(0x46);
    }
    out.push(0x71);
    out.push(0x71);
    out.push(0x71);
    out.extend(set_local(BASE_STACK));
    // Count each matching selected name separately from its target UUID.
    // A duplicate display name is ambiguous even when only one UUID matches.
    out.extend(get_local(PARENT_ID));
    out.extend(i32(1));
    out.push(0x6a);
    out.extend(set_local(PARENT_ID));
    out.push(0x0b);
    out.extend(get_local(INDEX));
    out.extend(i32(1));
    out.push(0x6a);
    out.extend(set_local(INDEX));
    out.extend([0x0c, 0x00, 0x0b, 0x0b]);
    out.extend(get_local(PARENT_ID));
    out.extend(i32(1));
    out.push(0x47);
    out.extend(get_local(BASE_STACK));
    out.extend(i32(1));
    out.push(0x47);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    early(&mut out);
    out.push(0x0b);
    stage(&mut out, globals, 24);
    // Reserve 64 bytes to retain the client's 16-byte stack alignment; only
    // first 40 form the fixed dispatch packet. Validate global zero before
    // decrementing it, so a malformed client stack cannot wrap or trap.
    out.extend([0x23, 0x00]);
    out.extend(set_local(BASE_STACK));
    out.extend(get_local(BASE_STACK));
    out.extend(i32(64));
    out.push(0x49);
    out.extend(get_local(BASE_STACK));
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.push(0x4b);
    out.push(0x72);
    out.extend(get_local(BASE_STACK));
    out.extend(i32(15));
    out.push(0x71);
    out.push(0x45);
    out.push(0x45);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    early(&mut out);
    out.push(0x0b);
    stage(&mut out, globals, 25);
    out.extend(get_local(BASE_STACK));
    out.extend(i32(64));
    out.push(0x6b);
    out.extend([0x22]);
    out.extend(uleb(STACK as u64));
    out.extend([0x24, 0x00]);
    let fail = |out: &mut Vec<u8>| {
        out.extend(get_local(STACK));
        out.extend(i32(64));
        out.push(0x6a);
        out.extend([0x24, 0x00]);
        out.extend(i32(-2));
        out.push(0x0f);
    };
    out.extend(i32(proof.frame_count as i64));
    out.extend(load(0));
    out.extend(set_local(COUNT));
    out.extend(i32(proof.frame_array as i64));
    out.extend(load(0));
    out.extend(set_local(TABLE));
    out.extend(get_local(COUNT));
    out.extend(i32(1));
    out.push(0x49);
    out.extend(get_local(COUNT));
    out.extend(i32(16_384));
    out.push(0x4b);
    out.push(0x72);
    out.extend(get_local(TABLE));
    out.push(0x45);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    // Prove `count * 4` fits before deriving `memory - span`; malformed
    // transient metadata must not underflow this bound.
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(get_local(COUNT));
    out.extend(i32(4));
    out.push(0x6c);
    out.push(0x49);
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(get_local(COUNT));
    out.extend(i32(4));
    out.push(0x6c);
    out.push(0x6b);
    out.extend(get_local(TABLE));
    out.push(0x49);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(i32(0));
    out.extend(set_local(INDEX));
    out.extend(i32(0));
    out.extend(set_local(FRAME));
    out.extend([0x02, 0x40, 0x03, 0x40]);
    out.extend(get_local(INDEX));
    out.extend(get_local(COUNT));
    out.push(0x4f);
    out.extend([0x0d, 0x01]);
    out.extend(get_local(TABLE));
    out.extend(get_local(INDEX));
    out.extend(i32(4));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(load(0));
    out.extend(set_local(PARENT));
    // non-null complete frame + label + visible state.
    out.extend(get_local(PARENT));
    out.push(0x45);
    out.extend([0x04, 0x40, 0x05]);
    out.extend(get_local(PARENT));
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(i32(0x1c8));
    out.push(0x6b);
    out.push(0x4b);
    out.extend([0x04, 0x40, 0x05]);
    out.extend(get_local(PARENT));
    out.extend(load(0xbc));
    out.extend(get_local(INDEX));
    out.push(0x46);
    out.extend(get_local(PARENT));
    out.extend(load(0x134));
    out.extend(i32(proof.play_hash as i64));
    out.push(0x46);
    out.push(0x71);
    out.extend(get_local(PARENT));
    out.extend(load(0x18c));
    out.extend(i32(4));
    out.push(0x71);
    out.push(0x45);
    out.push(0x45);
    out.push(0x71);
    out.extend(get_local(PARENT));
    out.extend(load(0x18c));
    out.extend(i32(0x200));
    out.push(0x71);
    out.push(0x45);
    out.push(0x71);
    out.extend([0x04, 0x40]);
    out.extend(get_local(PARENT));
    out.extend(set_local(FRAME));
    out.extend([0x0c, 0x04, 0x0b]);
    out.push(0x0b);
    out.push(0x0b);
    out.extend(get_local(INDEX));
    out.extend(i32(1));
    out.push(0x6a);
    out.extend(set_local(INDEX));
    out.extend([0x0c, 0x00, 0x0b, 0x0b]);
    out.extend(get_local(FRAME));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(get_local(FRAME));
    out.extend(load(0xbc));
    out.push(0x10);
    out.extend(uleb(proof.frame_parent as u64));
    out.extend(set_local(PARENT_ID));
    out.extend(get_local(PARENT_ID));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(get_local(PARENT_ID));
    out.push(0x10);
    out.extend(uleb(proof.frame_resolver as u64));
    out.extend(set_local(PARENT));
    out.extend(get_local(PARENT));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    // Resolver output is client-owned: bound it and recheck claimed identity
    // before either parent field participates in packet dispatch.
    out.extend(get_local(PARENT));
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(i32(0x1c8));
    out.push(0x6b);
    out.push(0x4b);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(get_local(PARENT));
    out.extend(load(0xbc));
    out.extend(get_local(PARENT_ID));
    out.push(0x47);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    stage(&mut out, globals, 26);
    // exact ButtonClick packet from reference.
    out.extend(get_local(STACK));
    out.extend(i32(0));
    stage(&mut out, globals, 27);
    out.extend(store(24));
    out.extend(get_local(STACK));
    out.extend(get_local(FRAME));
    out.extend(load(0x1c4));
    out.extend(store(28));
    out.extend(get_local(STACK));
    out.extend(get_local(FRAME));
    out.extend(load(0xb8));
    out.extend(store(0));
    out.extend(get_local(STACK));
    out.extend(get_local(FRAME));
    out.extend(load(0xb8));
    out.extend(store(4));
    out.extend(get_local(STACK));
    out.extend(i32(7));
    out.extend(store(8));
    out.extend(get_local(STACK));
    out.extend(get_local(STACK));
    out.extend(i32(24));
    out.push(0x6a);
    out.extend(store(12));
    out.extend(get_local(STACK));
    out.extend(i32(0));
    out.extend(store(16));
    out.extend(get_local(PARENT));
    out.extend(i32(proof.frame_dispatch_offset as i64));
    out.push(0x6a);
    out.extend(i32(0x31));
    out.extend(get_local(STACK));
    out.extend(i32(0));
    out.push(0x10);
    out.extend(uleb(proof.frame_dispatch as u64));
    out.extend(get_local(STACK));
    out.extend(i32(64));
    out.push(0x6a);
    out.extend([0x24, 0x00]);
    out.extend(i32(1));
    out.push(0x0b);
    out
}

/// Resolves current Selector row name only while a game callback is draining.
/// It queries certified message `0x5a`, then bounds the current handler model.
/// It returns a live UTF-16 name pointer or zero and always restores global 0.
pub(super) fn emit_current_selector_name(proof: ActionProof) -> Vec<u8> {
    const COUNT: u32 = 0;
    const TABLE: u32 = 1;
    const INDEX: u32 = 2;
    const SELECTOR: u32 = 3;
    const CHILD_ID: u32 = 4;
    const CHILD: u32 = 5;
    const WRAPPER: u32 = 6;
    const MODEL: u32 = 7;
    const ROW: u32 = 8;
    const BASE: u32 = 9;
    const STACK: u32 = 10;
    const CANDIDATE: u32 = 11;
    let mut out = vec![0x01, 0x0c, 0x7f];
    let fail = |out: &mut Vec<u8>| {
        out.extend(get_local(BASE));
        out.extend([0x24, 0x00]);
        out.extend(i32(0));
        out.push(0x0f);
    };
    let invalid_ptr = |out: &mut Vec<u8>, local: u32, bytes: i64| {
        out.extend(get_local(local));
        out.push(0x45);
        out.extend([0x3f, 0x00]);
        out.extend(i32(16));
        out.push(0x74);
        out.extend(i32(bytes));
        out.push(0x49);
        out.push(0x72);
        out.extend(get_local(local));
        out.extend([0x3f, 0x00]);
        out.extend(i32(16));
        out.push(0x74);
        out.extend(i32(bytes));
        out.push(0x6b);
        out.push(0x4b);
        out.push(0x72);
    };
    // Reserve query scratch before an action-thread frame dispatch.
    out.extend([0x23, 0x00]);
    out.extend(set_local(BASE));
    out.extend(get_local(BASE));
    out.extend(i32(64));
    out.push(0x49);
    out.extend(get_local(BASE));
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.push(0x4b);
    out.push(0x72);
    out.extend(get_local(BASE));
    out.extend(i32(15));
    out.push(0x71);
    out.push(0x45);
    out.push(0x45);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    out.extend(i32(0));
    out.push(0x0f);
    out.push(0x0b);
    out.extend(get_local(BASE));
    out.extend(i32(64));
    out.push(0x6b);
    out.extend(set_local(STACK));
    out.extend(get_local(STACK));
    out.extend([0x24, 0x00]);
    // Current certified ID-manager table is bounded before traversal.
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(i32((proof.frame_count + 4) as i64));
    out.push(0x49);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(i32(proof.frame_count as i64));
    out.extend(load(0));
    out.extend(set_local(COUNT));
    out.extend(i32(proof.frame_array as i64));
    out.extend(load(0));
    out.extend(set_local(TABLE));
    out.extend(get_local(COUNT));
    out.extend(i32(1));
    out.push(0x49);
    out.extend(get_local(COUNT));
    out.extend(i32(16_384));
    out.push(0x4b);
    out.push(0x72);
    out.extend(get_local(TABLE));
    out.push(0x45);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(get_local(COUNT));
    out.extend(i32(4));
    out.push(0x6c);
    out.push(0x49);
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(get_local(COUNT));
    out.extend(i32(4));
    out.push(0x6c);
    out.push(0x6b);
    out.extend(get_local(TABLE));
    out.push(0x49);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(i32(0));
    out.extend(set_local(INDEX));
    out.extend(i32(0));
    out.extend(set_local(SELECTOR));
    out.extend([0x02, 0x40, 0x03, 0x40]);
    out.extend(get_local(INDEX));
    out.extend(get_local(COUNT));
    out.push(0x4f);
    out.extend([0x0d, 0x01]);
    out.extend(get_local(TABLE));
    out.extend(get_local(INDEX));
    out.extend(i32(4));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(load(0));
    out.extend(set_local(CANDIDATE));
    invalid_ptr(&mut out, CANDIDATE, 0x1c8);
    out.extend([0x04, 0x40, 0x05]);
    out.extend(get_local(CANDIDATE));
    out.extend(load(0xbc));
    out.extend(get_local(INDEX));
    out.push(0x46);
    out.extend(get_local(CANDIDATE));
    out.extend(load(0x134));
    out.extend(i32(proof.selector_hash as i64));
    out.push(0x46);
    out.push(0x71);
    out.extend(get_local(CANDIDATE));
    out.extend(load(0x18c));
    out.extend(i32(4));
    out.push(0x71);
    out.push(0x45);
    out.push(0x45);
    out.push(0x71);
    out.extend(get_local(CANDIDATE));
    out.extend(load(0x18c));
    out.extend(i32(0x200));
    out.push(0x71);
    out.push(0x45);
    out.push(0x71);
    out.extend([0x04, 0x40]);
    out.extend(get_local(CANDIDATE));
    out.extend(set_local(SELECTOR));
    out.extend([0x0c, 0x01, 0x0b]);
    out.push(0x0b);
    out.extend(get_local(INDEX));
    out.extend(i32(1));
    out.push(0x6a);
    out.extend(set_local(INDEX));
    out.extend([0x0c, 0x00, 0x0b, 0x0b]);
    out.extend(get_local(SELECTOR));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    // Resolver output and its identity remain bounded before query dispatch.
    out.extend(get_local(SELECTOR));
    out.extend(load(0xbc));
    out.extend(i32(0));
    out.push(0x10);
    out.extend(uleb(proof.frame_child as u64));
    out.extend(set_local(CHILD_ID));
    out.extend(get_local(CHILD_ID));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(get_local(CHILD_ID));
    out.push(0x10);
    out.extend(uleb(proof.frame_resolver as u64));
    out.extend(set_local(CHILD));
    invalid_ptr(&mut out, CHILD, 0x1c8);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(get_local(CHILD));
    out.extend(load(0xbc));
    out.extend(get_local(CHILD_ID));
    out.push(0x47);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    // Certified current handler rows: frame +168/+176, row context wrapper +4.
    out.extend(get_local(SELECTOR));
    out.extend(load(proof.callback_rows_offset));
    out.extend(set_local(TABLE));
    out.extend(get_local(SELECTOR));
    out.extend(load(proof.callback_count_offset));
    out.extend(set_local(COUNT));
    out.extend(get_local(COUNT));
    out.extend(i32(1));
    out.push(0x49);
    out.extend(get_local(COUNT));
    out.extend(i32(64));
    out.push(0x4b);
    out.push(0x72);
    out.extend(get_local(TABLE));
    out.push(0x45);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(get_local(COUNT));
    out.extend(i32(proof.callback_row_bytes as i64));
    out.push(0x6c);
    out.push(0x49);
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(get_local(COUNT));
    out.extend(i32(proof.callback_row_bytes as i64));
    out.push(0x6c);
    out.push(0x6b);
    out.extend(get_local(TABLE));
    out.push(0x49);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    // f6508 selects first reverse active row (nonzero table slot and row+8 < 0).
    // That exact row must bind certified Selector callback slot; do not skip it.
    out.extend(get_local(COUNT));
    out.extend(set_local(INDEX));
    out.extend(i32(0));
    out.extend(set_local(MODEL));
    out.extend([0x02, 0x40, 0x03, 0x40]);
    out.extend(get_local(INDEX));
    out.push(0x45);
    out.extend([0x0d, 0x01]);
    out.extend(get_local(INDEX));
    out.extend(i32(1));
    out.push(0x6b);
    out.extend(set_local(INDEX));
    out.extend(get_local(TABLE));
    out.extend(get_local(INDEX));
    out.extend(i32(proof.callback_row_bytes as i64));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(set_local(WRAPPER));
    out.extend(get_local(WRAPPER));
    out.extend(load(0));
    out.push(0x45);
    out.extend([0x04, 0x40, 0x0c, 0x01, 0x0b]);
    out.extend(get_local(WRAPPER));
    out.extend(load(8));
    out.extend(i32(0));
    out.push(0x48);
    out.push(0x45);
    out.extend([0x04, 0x40, 0x0c, 0x01, 0x0b]);
    out.extend(get_local(WRAPPER));
    out.extend(load(0));
    out.extend(i32(proof.selector_callback_slot as i64));
    out.push(0x47);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    // f11951 receives address row+4 then loads model; direct row+4 load
    // is therefore exact model pointer. Do not dereference it twice.
    out.extend(get_local(WRAPPER));
    out.extend(load(proof.callback_context_offset));
    out.extend(set_local(MODEL));
    out.extend([0x0c, 0x01, 0x0b, 0x0b]);
    invalid_ptr(&mut out, MODEL, 20);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(get_local(MODEL));
    out.extend(load(4));
    out.extend(get_local(SELECTOR));
    out.extend(load(0xbc));
    out.push(0x47);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(get_local(MODEL));
    out.extend(load(proof.selector_context_rows_offset));
    out.extend(set_local(TABLE));
    out.extend(get_local(MODEL));
    out.extend(load(proof.selector_context_count_offset));
    out.extend(set_local(COUNT));
    out.extend(get_local(COUNT));
    out.extend(i32(1));
    out.push(0x49);
    out.extend(get_local(COUNT));
    out.extend(i32(64));
    out.push(0x4b);
    out.push(0x72);
    out.extend(get_local(TABLE));
    out.push(0x45);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(get_local(COUNT));
    out.extend(i32(4));
    out.push(0x6c);
    out.push(0x49);
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(get_local(COUNT));
    out.extend(i32(4));
    out.push(0x6c);
    out.push(0x6b);
    out.extend(get_local(TABLE));
    out.push(0x49);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    // Query only on callback thread. f11951 writes selected index to stack+32.
    out.extend(get_local(STACK));
    out.extend(i32(-1));
    out.extend(store(32));
    out.extend(get_local(CHILD));
    out.extend(i32(proof.frame_dispatch_offset as i64));
    out.push(0x6a);
    out.extend(i32(proof.selector_index_message as i64));
    out.extend(i32(0));
    out.extend(get_local(STACK));
    out.extend(i32(32));
    out.push(0x6a);
    out.push(0x10);
    out.extend(uleb(proof.frame_dispatch as u64));
    out.extend(get_local(STACK));
    out.extend(load(32));
    out.extend(set_local(INDEX));
    out.extend(get_local(INDEX));
    out.extend(get_local(COUNT));
    out.push(0x4f);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(get_local(TABLE));
    out.extend(get_local(INDEX));
    out.extend(i32(4));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(load(0));
    out.extend(set_local(ROW));
    invalid_ptr(&mut out, ROW, proof.selector_row_name_offset as i64 + 40);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(get_local(ROW));
    out.extend(i32(proof.selector_row_name_offset as i64));
    out.push(0x6a);
    out.extend(set_local(ROW));
    out.extend(get_local(ROW));
    out.extend(load16(0));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(get_local(BASE));
    out.extend([0x24, 0x00]);
    out.extend(get_local(ROW));
    out.push(0x0b);
    out
}

/// Emits a read-only `() -> i32` proof that the exact character-select UI is
/// present. It returns one only when both certified Selector and Play frames,
/// their resolver identities, and Selector-owned callback context agree.
pub(super) fn emit_ui_ready(proof: ActionProof) -> Vec<u8> {
    const COUNT: u32 = 0;
    const TABLE: u32 = 1;
    const INDEX: u32 = 2;
    const SELECTOR: u32 = 3;
    const CHILD_ID: u32 = 4;
    const CHILD: u32 = 5;
    const CALLBACK_ROWS: u32 = 6;
    const CALLBACK_COUNT: u32 = 7;
    const CONTEXT: u32 = 8;
    const PLAY_FRAME: u32 = 9;
    const PARENT_ID: u32 = 10;
    const PARENT: u32 = 11;
    const CANDIDATE: u32 = 12;
    let mut out = vec![0x01, 0x0d, 0x7f];
    let refuse = |out: &mut Vec<u8>| {
        out.extend(i32(0));
        out.push(0x0f);
    };
    let pointer_in_memory = |out: &mut Vec<u8>, pointer: u32, bytes: i64| {
        // `pointer != 0 && memory >= bytes && pointer <= memory - bytes`.
        // Memory is proved before subtraction so malformed transient pointers
        // cannot underflow their enclosing range.
        out.extend(get_local(pointer));
        out.extend([0x45, 0x45]);
        out.extend([0x3f, 0x00]);
        out.extend(i32(16));
        out.push(0x74);
        out.extend(i32(bytes));
        out.push(0x4f);
        out.push(0x71);
        out.extend(get_local(pointer));
        out.extend([0x3f, 0x00]);
        out.extend(i32(16));
        out.push(0x74);
        out.extend(i32(bytes));
        out.push(0x6b);
        out.push(0x4d);
        out.push(0x71);
    };

    // Every fixed static read below is contained by this exact observed span.
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(i32(0x5a75f4));
    out.push(0x49);
    out.extend([0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);
    out.extend(i32(0x5a75f0));
    out.extend(load(0));
    out.extend(set_local(CALLBACK_COUNT));
    out.extend(get_local(CALLBACK_COUNT));
    out.extend(i32(1));
    out.push(0x49);
    out.extend(get_local(CALLBACK_COUNT));
    out.extend(i32(64));
    out.push(0x4b);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);
    out.extend(i32(proof.frame_count as i64));
    out.extend(load(0));
    out.extend(set_local(COUNT));
    out.extend(i32(proof.frame_array as i64));
    out.extend(load(0));
    out.extend(set_local(TABLE));
    out.extend(get_local(COUNT));
    out.extend(i32(1));
    out.push(0x49);
    out.extend(get_local(COUNT));
    out.extend(i32(16_384));
    out.push(0x4b);
    out.push(0x72);
    out.extend(get_local(TABLE));
    out.push(0x45);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);
    // Bounded registry table before `table + index * 4` is derived.
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(get_local(COUNT));
    out.extend(i32(4));
    out.push(0x6c);
    out.push(0x49);
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(get_local(COUNT));
    out.extend(i32(4));
    out.push(0x6c);
    out.push(0x6b);
    out.extend(get_local(TABLE));
    out.push(0x49);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);

    // Find exact, visible Selector frame. Frame ID must equal registry slot.
    out.extend(i32(0));
    out.extend(set_local(INDEX));
    out.extend(i32(0));
    out.extend(set_local(SELECTOR));
    out.extend([0x02, 0x40, 0x03, 0x40]);
    out.extend(get_local(INDEX));
    out.extend(get_local(COUNT));
    out.push(0x4f);
    out.extend([0x0d, 0x01]);
    out.extend(get_local(TABLE));
    out.extend(get_local(INDEX));
    out.extend(i32(4));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(load(0));
    out.extend(set_local(CANDIDATE));
    pointer_in_memory(&mut out, CANDIDATE, 0x1c8);
    out.extend([0x04, 0x40]);
    out.extend(get_local(CANDIDATE));
    out.extend(load(0xbc));
    out.extend(get_local(INDEX));
    out.push(0x46);
    out.extend(get_local(CANDIDATE));
    out.extend(load(0x134));
    out.extend(i32(proof.selector_hash as i64));
    out.push(0x46);
    out.push(0x71);
    out.extend(get_local(CANDIDATE));
    out.extend(load(0x18c));
    out.extend(i32(4));
    out.push(0x71);
    out.push(0x45);
    out.push(0x45);
    out.push(0x71);
    out.extend(get_local(CANDIDATE));
    out.extend(load(0x18c));
    out.extend(i32(0x200));
    out.push(0x71);
    out.push(0x45);
    out.push(0x71);
    out.extend([0x04, 0x40]);
    out.extend(get_local(CANDIDATE));
    out.extend(set_local(SELECTOR));
    out.extend([0x0c, 0x01]);
    out.push(0x0b);
    out.push(0x0b);
    out.extend(get_local(INDEX));
    out.extend(i32(1));
    out.push(0x6a);
    out.extend(set_local(INDEX));
    out.extend([0x0c, 0x00, 0x0b, 0x0b]);
    out.extend(get_local(SELECTOR));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);

    // Selector child identity is exact before its callback metadata is read.
    out.extend(get_local(SELECTOR));
    out.extend(load(0xbc));
    out.extend(i32(0));
    out.push(0x10);
    out.extend(uleb(proof.frame_child as u64));
    out.extend(set_local(CHILD_ID));
    out.extend(get_local(CHILD_ID));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);
    out.extend(get_local(CHILD_ID));
    out.push(0x10);
    out.extend(uleb(proof.frame_resolver as u64));
    out.extend(set_local(CHILD));
    pointer_in_memory(&mut out, CHILD, 0x1c8);
    out.extend([0x45, 0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);
    out.extend(get_local(CHILD));
    out.extend(load(0xbc));
    out.extend(get_local(CHILD_ID));
    out.push(0x47);
    out.extend([0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);

    // Latest non-null Selector callback row must name Selector frame itself.
    out.extend(get_local(SELECTOR));
    out.extend(load(0xa8));
    out.extend(set_local(CALLBACK_ROWS));
    out.extend(get_local(SELECTOR));
    out.extend(load(0xb0));
    out.extend(set_local(CALLBACK_COUNT));
    out.extend(get_local(CALLBACK_COUNT));
    out.extend(i32(1));
    out.push(0x49);
    out.extend(get_local(CALLBACK_COUNT));
    out.extend(i32(64));
    out.push(0x4b);
    out.push(0x72);
    out.extend(get_local(CALLBACK_ROWS));
    out.push(0x45);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(get_local(CALLBACK_COUNT));
    out.extend(i32(12));
    out.push(0x6c);
    out.push(0x49);
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(get_local(CALLBACK_COUNT));
    out.extend(i32(12));
    out.push(0x6c);
    out.push(0x6b);
    out.extend(get_local(CALLBACK_ROWS));
    out.push(0x49);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);
    // Match f6508 first reverse active callback row and its certified handler.
    out.extend(get_local(CALLBACK_COUNT));
    out.extend(set_local(INDEX));
    out.extend(i32(0));
    out.extend(set_local(CONTEXT));
    out.extend([0x02, 0x40, 0x03, 0x40]);
    out.extend(get_local(INDEX));
    out.push(0x45);
    out.extend([0x0d, 0x01]);
    out.extend(get_local(INDEX));
    out.extend(i32(1));
    out.push(0x6b);
    out.extend(set_local(INDEX));
    out.extend(get_local(CALLBACK_ROWS));
    out.extend(get_local(INDEX));
    out.extend(i32(12));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(set_local(CANDIDATE));
    out.extend(get_local(CANDIDATE));
    out.extend(load(0));
    out.push(0x45);
    out.extend([0x04, 0x40, 0x0c, 0x01, 0x0b]);
    out.extend(get_local(CANDIDATE));
    out.extend(load(8));
    out.extend(i32(0));
    out.push(0x48);
    out.push(0x45);
    out.extend([0x04, 0x40, 0x0c, 0x01, 0x0b]);
    out.extend(get_local(CANDIDATE));
    out.extend(load(0));
    out.extend(i32(proof.selector_callback_slot as i64));
    out.push(0x47);
    out.extend([0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);
    out.extend(get_local(CANDIDATE));
    out.extend(load(4));
    out.extend(set_local(CONTEXT));
    out.extend([0x0c, 0x01, 0x0b, 0x0b]);
    pointer_in_memory(&mut out, CONTEXT, 20);
    out.extend([0x45, 0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);
    out.extend(get_local(CONTEXT));
    out.extend(load(4));
    out.extend(get_local(SELECTOR));
    out.extend(load(0xbc));
    out.push(0x47);
    out.extend([0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);

    // Find exact, visible Play frame independently from Selector frame.
    out.extend(i32(0));
    out.extend(set_local(INDEX));
    out.extend(i32(0));
    out.extend(set_local(PLAY_FRAME));
    out.extend([0x02, 0x40, 0x03, 0x40]);
    out.extend(get_local(INDEX));
    out.extend(get_local(COUNT));
    out.push(0x4f);
    out.extend([0x0d, 0x01]);
    out.extend(get_local(TABLE));
    out.extend(get_local(INDEX));
    out.extend(i32(4));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(load(0));
    out.extend(set_local(CANDIDATE));
    pointer_in_memory(&mut out, CANDIDATE, 0x1c8);
    out.extend([0x04, 0x40]);
    out.extend(get_local(CANDIDATE));
    out.extend(load(0xbc));
    out.extend(get_local(INDEX));
    out.push(0x46);
    out.extend(get_local(CANDIDATE));
    out.extend(load(0x134));
    out.extend(i32(proof.play_hash as i64));
    out.push(0x46);
    out.push(0x71);
    out.extend(get_local(CANDIDATE));
    out.extend(load(0x18c));
    out.extend(i32(4));
    out.push(0x71);
    out.push(0x45);
    out.push(0x45);
    out.push(0x71);
    out.extend(get_local(CANDIDATE));
    out.extend(load(0x18c));
    out.extend(i32(0x200));
    out.push(0x71);
    out.push(0x45);
    out.push(0x71);
    out.extend([0x04, 0x40]);
    out.extend(get_local(CANDIDATE));
    out.extend(set_local(PLAY_FRAME));
    out.extend([0x0c, 0x01]);
    out.push(0x0b);
    out.push(0x0b);
    out.extend(get_local(INDEX));
    out.extend(i32(1));
    out.push(0x6a);
    out.extend(set_local(INDEX));
    out.extend([0x0c, 0x00, 0x0b, 0x0b]);
    out.extend(get_local(PLAY_FRAME));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);
    out.extend(get_local(PLAY_FRAME));
    out.extend(load(0xbc));
    out.push(0x10);
    out.extend(uleb(proof.frame_parent as u64));
    out.extend(set_local(PARENT_ID));
    out.extend(get_local(PARENT_ID));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);
    out.extend(get_local(PARENT_ID));
    out.push(0x10);
    out.extend(uleb(proof.frame_resolver as u64));
    out.extend(set_local(PARENT));
    pointer_in_memory(&mut out, PARENT, 0x1c8);
    out.extend([0x45, 0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);
    out.extend(get_local(PARENT));
    out.extend(load(0xbc));
    out.extend(get_local(PARENT_ID));
    out.push(0x47);
    out.extend([0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);
    out.extend(i32(1));
    out.push(0x0b);
    out
}

/// Emits `(kind, index) -> i32`. Status is `-1` while one request waits for
/// callback drain, `0` idle/cancelled, `1` accepted (return only), `-2`
/// refused, and positive executor values only after callback completion.
/// Only Select(0..63) and Play(0) are accepted; only one request exists.
pub(super) fn emit_enqueue(globals: ActionGlobals) -> Vec<u8> {
    let mut out = vec![0x00];
    let reject = |out: &mut Vec<u8>| {
        out.extend(i32(-2));
        out.extend(set_global(globals.result));
        out.extend(i32(0));
        out.push(0x0f);
    };
    out.extend(get_global(globals.enabled));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    reject(&mut out);
    out.push(0x0b);
    out.extend(get_global(globals.target_set));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    reject(&mut out);
    out.push(0x0b);
    out.extend(get_global(globals.pending));
    out.extend([0x45, 0x45, 0x04, 0x40]);
    out.extend(i32(0));
    out.push(0x0f);
    out.push(0x0b);
    // kind must be Select or Play.
    out.extend(get_local(0));
    out.extend(i32(SELECT as i64));
    out.push(0x46);
    out.extend(get_local(0));
    out.extend(i32(PLAY as i64));
    out.push(0x46);
    out.push(0x72);
    out.push(0x45);
    out.extend([0x04, 0x40]);
    reject(&mut out);
    out.push(0x0b);
    // Select index is bounded; Play accepts only its canonical zero argument.
    out.extend(get_local(0));
    out.extend(i32(SELECT as i64));
    out.push(0x46);
    out.extend([0x04, 0x40]);
    out.extend(get_local(1));
    out.extend(i32(MAX_CHARACTERS as i64));
    out.push(0x4f);
    out.extend([0x04, 0x40]);
    reject(&mut out);
    out.push(0x0b);
    out.push(0x05);
    out.extend(get_local(1));
    out.push(0x45);
    out.push(0x45);
    out.extend([0x04, 0x40]);
    reject(&mut out);
    out.push(0x0b);
    out.push(0x0b);
    out.extend(i32(0));
    out.extend(set_global(globals.selected_name));
    out.extend(get_local(1));
    out.extend(set_global(globals.argument));
    out.extend(i32(-1));
    out.extend(set_global(globals.expected));
    out.extend(i32(0));
    out.extend(set_global(globals.attempts));
    out.extend(get_local(0));
    out.extend(set_global(globals.pending));
    out.extend(i32(-1));
    out.extend(set_global(globals.result));
    stage(&mut out, globals, 1);
    out.extend(i32(1));
    out.push(0x0b);
    out
}

/// Freezes one opaque UUID as four i32 words. It has no row, pointer, name,
/// or frame input, and refuses replacement once any target has been frozen.
pub(super) fn emit_target(globals: ActionGlobals) -> Vec<u8> {
    let mut out = vec![0x00];
    let refuse = |out: &mut Vec<u8>| {
        out.extend(i32(0));
        out.push(0x0f);
    };
    out.extend(get_global(globals.enabled));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);
    out.extend(get_global(globals.pending));
    out.push(0x45);
    out.push(0x45);
    out.extend([0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);
    out.extend(get_global(globals.target_set));
    out.push(0x45);
    out.push(0x45);
    out.extend([0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);
    out.extend(get_local(0));
    out.extend(get_local(1));
    out.push(0x72);
    out.extend(get_local(2));
    out.push(0x72);
    out.extend(get_local(3));
    out.push(0x72);
    out.push(0x45);
    out.extend([0x04, 0x40]);
    refuse(&mut out);
    out.push(0x0b);
    for (local, global) in [
        (0, globals.uuid0),
        (1, globals.uuid1),
        (2, globals.uuid2),
        (3, globals.uuid3),
    ] {
        out.extend(get_local(local));
        out.extend(set_global(global));
    }
    out.extend(i32(1));
    out.extend(set_global(globals.target_set));
    out.extend(i32(1));
    out.push(0x0b);
    out
}

/// Emits `() -> i32`; cancellation clears all actionable state before return.
pub(super) fn emit_cancel(globals: ActionGlobals) -> Vec<u8> {
    let mut out = vec![0x00];
    for global in [
        globals.pending,
        globals.argument,
        globals.result,
        globals.attempts,
        globals.stage,
        globals.uuid0,
        globals.uuid1,
        globals.uuid2,
        globals.uuid3,
        globals.target_set,
        globals.selected_name,
    ] {
        out.extend(i32(0));
        out.extend(set_global(global));
    }
    out.extend(i32(-1));
    out.extend(set_global(globals.expected));
    out.extend(i32(1));
    out.push(0x0b);
    out
}

/// Emits `(enabled) -> i32`. Exact one enables; all other values disable and
/// cancel. No pointer, frame id, name, or caller-selected dispatcher crosses it.
pub(super) fn emit_configure(globals: ActionGlobals) -> Vec<u8> {
    let mut out = vec![0x00];
    out.extend(get_local(0));
    out.extend(i32(1));
    out.push(0x46);
    out.extend(set_global(globals.enabled));
    out.extend(get_global(globals.enabled));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    for global in [
        globals.pending,
        globals.argument,
        globals.result,
        globals.attempts,
        globals.stage,
        globals.uuid0,
        globals.uuid1,
        globals.uuid2,
        globals.uuid3,
        globals.target_set,
        globals.selected_name,
    ] {
        out.extend(i32(0));
        out.extend(set_global(global));
    }
    out.extend(i32(-1));
    out.extend(set_global(globals.expected));
    out.push(0x0b);
    out.extend(i32(1));
    out.push(0x0b);
    out
}

/// Emits current confirmed Selector row name pointer, never dispatching a frame call.
pub(super) fn emit_selected_name(globals: ActionGlobals) -> Vec<u8> {
    let mut out = vec![0x00];
    out.extend(get_global(globals.enabled));
    out.push(0x45);
    out.extend([0x04, 0x7f]);
    out.extend(i32(0));
    out.push(0x05);
    out.extend(get_global(globals.selected_name));
    out.push(0x0b);
    out.push(0x0b);
    out
}

/// Emits `() -> i32`, exposing only closed queue state to adapter polling.
pub(super) fn emit_status(globals: ActionGlobals) -> Vec<u8> {
    let mut out = vec![0x00];
    out.extend(get_global(globals.result));
    out.push(0x0b);
    out
}

/// Emits `() -> i32`, exposing only a closed executor phase for diagnosis.
pub(super) fn emit_stage(globals: ActionGlobals) -> Vec<u8> {
    let mut out = vec![0x00];
    out.extend(get_global(globals.stage));
    out.push(0x0b);
    out
}

/// Emits `() -> ()`. Clears pending before direct executor call, preventing a
/// reentrant callback from dispatching same request twice. Executor receives
/// only closed kind/index pair and writes closed status into private global.
pub(super) fn emit_drain(globals: ActionGlobals, execute: u32) -> Vec<u8> {
    let mut out = vec![0x00];
    out.extend(get_global(globals.pending));
    out.push(0x45);
    out.extend([0x04, 0x40, 0x0f, 0x0b]);
    out.extend(get_global(globals.enabled));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    out.extend(i32(0));
    out.extend(set_global(globals.pending));
    out.extend(i32(0));
    out.extend(set_global(globals.result));
    out.push(0x0f);
    out.push(0x0b);
    out.extend(get_global(globals.pending));
    out.extend(get_global(globals.argument));
    out.extend(i32(0));
    out.extend(set_global(globals.pending));
    out.push(0x10);
    out.extend(uleb(execute as u64));
    out.extend(set_global(globals.result));
    out.push(0x0b);
    out
}

/// Routes only two closed actions to their separately bounded executors.
pub(super) fn emit_execute(selector: u32, play: u32, globals: ActionGlobals) -> Vec<u8> {
    let mut out = vec![0x00];
    out.extend(get_local(0));
    out.extend(i32(SELECT as i64));
    out.push(0x46);
    out.extend([0x04, 0x7f]);
    stage(&mut out, globals, 10);
    out.extend(get_local(0));
    out.extend(get_local(1));
    out.push(0x10);
    out.extend(uleb(selector as u64));
    out.push(0x05);
    stage(&mut out, globals, 20);
    out.extend(get_local(0));
    out.extend(get_local(1));
    out.push(0x10);
    out.extend(uleb(play as u64));
    out.push(0x0b);
    out.push(0x0b);
    out
}

#[cfg(test)]
mod tests {
    use super::super::codec::{
        Section, WASM_HEADER, encode_code, encode_index_vector, encode_section,
    };
    use super::*;

    fn module(types: Vec<Vec<u8>>, function_types: &[u32], bodies: &[Vec<u8>]) -> Vec<u8> {
        let mut type_body = uleb(types.len() as u64);
        for ty in types {
            type_body.extend(ty);
        }
        let mut wasm = WASM_HEADER.to_vec();
        let mut globals = vec![13];
        for _ in 0..13 {
            globals.extend([0x7f, 1, 0x41, 0, 0x0b]);
        }
        wasm.extend(encode_section(&Section {
            id: 1,
            body: type_body,
        }));
        wasm.extend(encode_section(&Section {
            id: 3,
            body: encode_index_vector(function_types),
        }));
        wasm.extend(encode_section(&Section {
            id: 6,
            body: globals,
        }));
        wasm.extend(encode_section(&Section {
            id: 10,
            body: encode_code(bodies),
        }));
        wasm
    }
    #[test]
    fn queue_emitters_validate_as_closed_wasm() {
        let g = ActionGlobals {
            pending: 0,
            argument: 1,
            enabled: 2,
            result: 3,
            expected: 4,
            attempts: 5,
            stage: 6,
            uuid0: 7,
            uuid1: 8,
            uuid2: 9,
            uuid3: 10,
            target_set: 11,
            selected_name: 12,
        };
        let types = vec![
            vec![0x60, 2, 0x7f, 0x7f, 1, 0x7f],
            vec![0x60, 0, 1, 0x7f],
            vec![0x60, 1, 0x7f, 1, 0x7f],
            vec![0x60, 0, 0],
        ];
        let bodies = vec![
            emit_enqueue(g),
            emit_cancel(g),
            emit_configure(g),
            emit_status(g),
            emit_drain(g, 5),
            vec![0x00, 0x41, 1, 0x0b],
        ];
        let wasm = module(types, &[0, 1, 2, 1, 3, 0], &bodies);
        Validator::new().validate_all(&wasm).unwrap();
    }
    #[test]
    fn plan_refuses_unknown_or_corrupt_inputs() {
        assert!(action_plan(b"not wasm").is_err());
    }
}

#[cfg(test)]
#[path = "character_action_fixtures.rs"]
mod regression_tests;

#[cfg(test)]
#[path = "character_ui_ready_fixtures.rs"]
mod ui_ready_regression_tests;

#[cfg(test)]
#[path = "character_action_play_fixtures.rs"]
mod play_regression_tests;

#[cfg(test)]
#[path = "character_selection_query_fixtures.rs"]
mod selection_query_regression_tests;
