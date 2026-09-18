//! Bounded Selector click emitter for certified JSPI character-select UI.
//!
//! GPL-3.0-only lineage: GWoNmac's bounded pre-game character-switch
//! transform, commit `004194ff318320f854240fd227462b19889bef24`.
//! This deliberately accepts only `(Select, roster_index)`.  It re-reads both
//! roster and transient UI state on callback thread, proves copied UTF-16 name
//! against Selector-owned row, then emits at most one adjacent click.

use super::character_actions::{ActionGlobals, ActionProof};
use super::codec::{sleb, uleb};

const SELECT: i64 = 1;
const ROSTER_POINTER: i64 = 0x5a_75e8;
const ROSTER_COUNT: i64 = 0x5a_75f0;
const FRAME_BYTES: i64 = 0x1c8;
const FRAME_CHILD_ID: i64 = 0xb8;
const FRAME_ID: i64 = 0xbc;
const FRAME_HASH: i64 = 0x134;
const FRAME_STATE: i64 = 0x18c;
const FRAME_DISPATCH: i64 = 0xa8;
const FRAME_CALLBACK_COUNT: i64 = 0xb0;
const RECORD_BYTES: i64 = 0x84;
const ACCOUNT_NAME: i64 = 0x18;
const SELECTOR_NAME: i64 = 0x20;
const NAME_UNITS: i64 = 20;
const CALLBACK_BYTES: i64 = 12;

fn local(index: u32) -> Vec<u8> {
    let mut out = vec![0x20];
    out.extend(uleb(index as u64));
    out
}
fn set_local(index: u32) -> Vec<u8> {
    let mut out = vec![0x21];
    out.extend(uleb(index as u64));
    out
}
fn global(index: u32) -> Vec<u8> {
    let mut out = vec![0x23];
    out.extend(uleb(index as u64));
    out
}
fn set_global(index: u32) -> Vec<u8> {
    let mut out = vec![0x24];
    out.extend(uleb(index as u64));
    out
}
fn i32(value: i64) -> Vec<u8> {
    let mut out = vec![0x41];
    out.extend(sleb(value));
    out
}
fn load(offset: i64) -> Vec<u8> {
    let mut out = vec![0x28, 2];
    out.extend(uleb(offset as u64));
    out
}
fn load16(offset: i64) -> Vec<u8> {
    let mut out = vec![0x2f, 1];
    out.extend(uleb(offset as u64));
    out
}
fn store(offset: i64) -> Vec<u8> {
    let mut out = vec![0x36, 2];
    out.extend(uleb(offset as u64));
    out
}

/// Emits `(kind, roster_index) -> i32`.  All refusal paths restore global 0
/// then return `-2`; one dispatched adjacent Selector click returns `1`.
///
/// Locals 2..21 hold scratch, fresh roster/frame/context traversal state. The
/// only payload is forty bytes inside a sixty-four-byte, alignment-preserving
/// reservation below certified client stack.
pub(super) fn emit_selector_execute(proof: ActionProof, globals: ActionGlobals) -> Vec<u8> {
    // Locals 2..22: scratch plus saved pre-reservation stack pointer.
    let mut out = vec![1, 21, 0x7f];
    let stage = |out: &mut Vec<u8>, value: i64| {
        out.extend(i32(value));
        out.extend(set_global(globals.stage));
    };
    stage(&mut out, 11);
    // Local 22 retains original stack pointer. Every post-reservation return
    // writes it back exactly, rather than reconstructing it arithmetically.
    let fail = |out: &mut Vec<u8>| {
        out.extend(local(22));
        out.extend(set_global(0));
        out.extend(i32(-2));
        out.push(0x0f);
    };
    let success = |out: &mut Vec<u8>| {
        out.extend(local(22));
        out.extend(set_global(0));
        out.extend(i32(1));
        out.push(0x0f);
    };
    let pending = |out: &mut Vec<u8>| {
        out.extend(local(22));
        out.extend(set_global(0));
        out.extend(i32(-1));
        out.push(0x0f);
    };
    let ptr_ok = |out: &mut Vec<u8>, pointer: u32, bytes: i64| {
        // `pointer != 0 && memory >= bytes && pointer <= memory - bytes`.
        // Check memory before subtraction so a small memory cannot wrap.
        out.extend(local(pointer));
        out.extend([0x45, 0x45]);
        out.extend([0x3f, 0x00]);
        out.extend(i32(16));
        out.push(0x74);
        out.extend(i32(bytes));
        out.push(0x4f);
        out.push(0x71);
        out.extend(local(pointer));
        out.extend([0x3f, 0x00]);
        out.extend(i32(16));
        out.push(0x74);
        out.extend(i32(bytes));
        out.push(0x6b);
        out.push(0x4d);
        out.push(0x71);
    };

    // Reservation leaves 16-byte alignment unchanged. Refuse malformed or
    // undersized stack before subtraction, while no restoration is required.
    out.extend(global(0));
    out.extend(set_local(22));
    stage(&mut out, 111);
    out.extend(local(22));
    out.extend(i32(64));
    out.push(0x49);
    out.extend([0x04, 0x40]);
    out.extend(i32(-2));
    out.push(0x0f);
    out.push(0x0b);
    stage(&mut out, 112);
    out.extend(local(22));
    out.extend(i32(15));
    out.push(0x71);
    out.push(0x45);
    out.push(0x45);
    out.extend([0x04, 0x40]);
    out.extend(i32(-2));
    out.push(0x0f);
    out.push(0x0b);
    stage(&mut out, 113);
    out.extend(local(22));
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.push(0x4b);
    out.extend([0x04, 0x40]);
    out.extend(i32(-2));
    out.push(0x0f);
    out.push(0x0b);
    out.extend(local(22));
    out.extend(i32(64));
    out.push(0x6b);
    out.extend([0x22, 2]);
    out.extend(set_global(0));
    stage(&mut out, 114);
    out.extend(local(0));
    out.extend(i32(SELECT));
    out.push(0x47);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);

    // Fresh roster index, pointer, and target UTF-16 identity.
    stage(&mut out, 115);
    out.extend(i32(ROSTER_COUNT));
    out.extend(load(0));
    out.extend(set_local(3));
    out.extend(local(3));
    out.extend(i32(1));
    out.push(0x49);
    out.extend(local(3));
    out.extend(i32(64));
    out.push(0x4b);
    out.push(0x72);
    out.extend(local(1));
    out.extend(local(3));
    out.push(0x4f);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(i32(ROSTER_POINTER));
    out.extend(load(0));
    out.extend(set_local(4));
    stage(&mut out, 116);
    // Prove whole fresh roster before deriving `base + index * record_bytes`.
    // This closes arithmetic and memory-span races before target-name access.
    out.extend(local(4));
    out.push(0x45);
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(local(3));
    out.extend(i32(RECORD_BYTES));
    out.push(0x6c);
    out.push(0x49);
    out.push(0x72);
    out.extend(local(4));
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(local(3));
    out.extend(i32(RECORD_BYTES));
    out.push(0x6c);
    out.push(0x6b);
    out.push(0x4b);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    // Frozen UUID binds this queued row across roster reordering. Compare four
    // opaque words before deriving its name or sending a click.
    out.extend(local(4));
    out.extend(local(1));
    out.extend(i32(RECORD_BYTES));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(i32(8));
    out.push(0x6a);
    out.extend(set_local(11));
    for (offset, uuid_global) in [
        (0, globals.uuid0),
        (4, globals.uuid1),
        (8, globals.uuid2),
        (12, globals.uuid3),
    ] {
        out.extend(local(11));
        out.extend(load(offset));
        out.extend(global(uuid_global));
        out.push(0x46);
    }
    out.push(0x71);
    out.push(0x71);
    out.push(0x71);
    out.push(0x45);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(local(4));
    out.extend(local(1));
    out.extend(i32(RECORD_BYTES));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(i32(ACCOUNT_NAME));
    out.push(0x6a);
    out.extend(set_local(11));
    stage(&mut out, 117);
    ptr_ok(&mut out, 11, NAME_UNITS * 2);
    out.extend([0x45, 0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);

    // Locate visible, identity-consistent Selector frame in bounded registry.
    stage(&mut out, 118);
    out.extend(i32(proof.frame_count as i64));
    out.extend(load(0));
    out.extend(set_local(3));
    out.extend(i32(proof.frame_array as i64));
    out.extend(load(0));
    out.extend(set_local(4));
    stage(&mut out, 119);
    out.extend(local(3));
    out.extend(i32(1));
    out.push(0x49);
    out.extend(local(3));
    out.extend(i32(16_384));
    out.push(0x4b);
    out.push(0x72);
    out.extend(local(4));
    out.push(0x45);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    // Bound pointer table before subtracting its dynamic span.
    stage(&mut out, 120);
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(local(3));
    out.extend(i32(2));
    out.push(0x74);
    out.push(0x49);
    out.extend(local(4));
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(local(3));
    out.extend(i32(2));
    out.push(0x74);
    out.push(0x6b);
    out.push(0x4b);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    stage(&mut out, 12);
    out.extend(i32(0));
    out.extend(set_local(5));
    out.extend(i32(0));
    out.extend(set_local(6));
    out.extend([0x02, 0x40, 0x03, 0x40]);
    out.extend(local(5));
    out.extend(local(3));
    out.push(0x4f);
    out.extend([0x0d, 1]);
    out.extend(local(4));
    out.extend(local(5));
    out.extend(i32(4));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(load(0));
    out.extend(set_local(7));
    ptr_ok(&mut out, 7, FRAME_BYTES);
    out.extend([0x04, 0x40]);
    out.extend(local(7));
    out.extend(load(FRAME_ID));
    out.extend(local(5));
    out.push(0x46);
    out.extend([0x04, 0x40]);
    // Preserve hash proof separately, then use one final match-and-visible
    // branch. Three enclosing ifs make `br 4` leave registry scan entirely.
    out.extend(local(7));
    out.extend(load(FRAME_HASH));
    out.extend(i32(proof.selector_hash as i64));
    out.push(0x46);
    out.extend([0x04, 0x40, 0x0b]);
    out.extend(local(7));
    out.extend(load(FRAME_HASH));
    out.extend(i32(proof.selector_hash as i64));
    out.push(0x46);
    out.extend(local(7));
    out.extend(load(FRAME_STATE));
    out.extend(i32(4));
    out.push(0x71);
    out.push(0x45);
    out.push(0x45);
    out.push(0x71);
    out.extend(local(7));
    out.extend(load(FRAME_STATE));
    out.extend(i32(0x200));
    out.push(0x71);
    out.push(0x45);
    out.push(0x71);
    out.extend([0x04, 0x40]);
    out.extend(local(7));
    out.extend(set_local(6));
    out.extend([0x0c, 4]);
    out.push(0x0b);
    out.push(0x0b);
    out.push(0x0b);
    out.extend(local(5));
    out.extend(i32(1));
    out.push(0x6a);
    out.extend(set_local(5));
    out.extend([0x0c, 0, 0x0b, 0x0b]);
    out.extend(local(6));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);

    stage(&mut out, 13);
    // Certified child/resolver pair, then synchronous Selector index read (0x5a).
    out.extend(local(6));
    out.extend(load(FRAME_ID));
    out.extend(i32(0));
    out.push(0x10);
    out.extend(uleb(proof.frame_child as u64));
    out.extend(set_local(10));
    out.extend(local(10));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(local(10));
    out.push(0x10);
    out.extend(uleb(proof.frame_resolver as u64));
    out.extend(set_local(8));
    ptr_ok(&mut out, 8, FRAME_BYTES);
    out.extend([0x45, 0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(local(8));
    out.extend(load(FRAME_ID));
    out.extend(local(10));
    out.push(0x47);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(local(2));
    // Query result is unsigned carousel index. A missing dispatcher write
    // leaves -1, which range proof below refuses before any click.
    out.extend(i32(-1));
    out.extend(store(32));
    out.extend(local(8));
    out.extend(i32(FRAME_DISPATCH));
    out.push(0x6a);
    out.extend(i32(0x5a));
    out.extend(i32(0));
    out.extend(local(2));
    out.extend(i32(32));
    out.push(0x6a);
    out.push(0x10);
    out.extend(uleb(proof.frame_dispatch as u64));
    out.extend(local(2));
    out.extend(load(32));
    out.extend(set_local(9));

    // A click is asynchronous. Until its adjacent index appears, only repeat
    // this read-only query; never send another ambiguous click. Drain clears
    // `pending` before entry, so continuation explicitly requeues Select.
    out.extend(global(globals.expected));
    out.extend(i32(-1));
    out.push(0x47);
    out.extend([0x04, 0x40]);
    out.extend(local(9));
    out.extend(global(globals.expected));
    out.push(0x47);
    out.extend([0x04, 0x40]);
    out.extend(global(globals.attempts));
    out.extend(i32(1));
    out.push(0x6a);
    out.extend(set_global(globals.attempts));
    out.extend(global(globals.attempts));
    out.extend(i32(180));
    out.push(0x4f);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(i32(SELECT));
    out.extend(set_global(globals.pending));
    pending(&mut out);
    out.push(0x0b);
    out.extend(i32(-1));
    out.extend(set_global(globals.expected));
    out.extend(i32(0));
    out.extend(set_global(globals.attempts));
    out.push(0x0b);

    stage(&mut out, 14);
    // f6508 walks rows backwards. First eligible row has a nonzero table slot
    // and negative row+8 flag; its slot must be Selector's exact callback.
    out.extend(local(6));
    out.extend(load(FRAME_DISPATCH));
    out.extend(set_local(12));
    out.extend(local(6));
    out.extend(i32(FRAME_CALLBACK_COUNT));
    out.push(0x6a);
    out.extend(load(0));
    out.extend(set_local(13));
    out.extend(local(13));
    out.extend(i32(1));
    out.push(0x49);
    out.extend(local(13));
    out.extend(i32(64));
    out.push(0x4b);
    out.push(0x72);
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(local(13));
    out.extend(i32(CALLBACK_BYTES));
    out.push(0x6c);
    out.push(0x49);
    out.extend(local(12));
    out.push(0x45);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(local(12));
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(local(13));
    out.extend(i32(CALLBACK_BYTES));
    out.push(0x6c);
    out.push(0x6b);
    out.push(0x4b);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(i32(0));
    out.extend(set_local(14));
    out.extend([0x02, 0x40, 0x03, 0x40]);
    out.extend(local(13));
    out.push(0x45);
    out.extend([0x0d, 1]);
    out.extend(local(13));
    out.extend(i32(1));
    out.push(0x6b);
    out.extend(set_local(13));
    out.extend(local(12));
    out.extend(local(13));
    out.extend(i32(CALLBACK_BYTES));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(set_local(17));
    // Inactive rows are skipped exactly as f6508 skips them.
    out.extend(local(17));
    out.extend(load(0));
    out.push(0x45);
    out.extend([0x04, 0x40, 0x0c, 0x01, 0x0b]);
    out.extend(local(17));
    out.extend(load(8));
    out.extend(i32(0));
    out.push(0x48); // lt_s: f6508's active-row discriminator
    out.push(0x45);
    out.extend([0x04, 0x40, 0x0c, 0x01, 0x0b]);
    // First active callback cannot be substituted by a later/earlier frame.
    out.extend(local(17));
    out.extend(load(0));
    out.extend(i32(proof.selector_callback_slot as i64));
    out.push(0x47);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(local(17));
    out.extend(load(4));
    out.extend(set_local(14));
    out.extend(local(14));
    out.extend([0x0d, 1]);
    out.extend([0x0c, 0, 0x0b, 0x0b]);
    ptr_ok(&mut out, 14, 20);
    out.extend([0x45, 0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(local(14));
    out.extend(load(4));
    out.extend(local(6));
    out.extend(load(FRAME_ID));
    out.push(0x47);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(local(14));
    out.extend(load(8));
    out.extend(set_local(15));
    out.extend(local(14));
    out.extend(load(16));
    out.extend(set_local(16));
    out.extend(local(16));
    out.extend(i32(1));
    out.push(0x49);
    out.extend(local(16));
    out.extend(i32(64));
    out.push(0x4b);
    out.push(0x72);
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(local(16));
    out.extend(i32(2));
    out.push(0x74);
    out.push(0x49);
    out.extend(local(15));
    out.push(0x45);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(local(15));
    out.extend([0x3f, 0x00]);
    out.extend(i32(16));
    out.push(0x74);
    out.extend(local(16));
    out.extend(i32(2));
    out.push(0x74);
    out.push(0x6b);
    out.push(0x4b);
    out.push(0x72);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);

    // Query must have populated a concrete Selector row. This also rejects
    // the -1 sentinel when a malformed dispatcher returns without writing.
    out.extend(local(9));
    out.extend(local(16));
    out.push(0x4f);
    out.extend([0x04, 0x40]);
    stage(&mut out, 151);
    fail(&mut out);
    out.push(0x0b);

    stage(&mut out, 152);
    // Exact bounded name lookup; duplicate identity refuses before any click.
    out.extend(i32(-1));
    out.extend(set_local(5));
    out.extend(i32(0));
    out.extend(set_local(21));
    out.extend(i32(0));
    out.extend(set_local(18));
    out.extend([0x02, 0x40, 0x03, 0x40]);
    out.extend(local(18));
    out.extend(local(16));
    out.push(0x4f);
    out.extend([0x0d, 1]);
    out.extend(local(15));
    out.extend(local(18));
    out.extend(i32(4));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(load(0));
    out.extend(set_local(17));
    out.extend(local(17));
    out.extend([0x04, 0x40]);
    ptr_ok(&mut out, 17, SELECTOR_NAME + NAME_UNITS * 2);
    out.extend([0x45, 0x04, 0x40]);
    stage(&mut out, 157);
    fail(&mut out);
    out.push(0x0b);
    out.extend(i32(1));
    out.extend(set_local(20));
    out.extend(i32(0));
    out.extend(set_local(19));
    out.extend([0x02, 0x40, 0x03, 0x40]);
    out.extend(local(19));
    out.extend(i32(NAME_UNITS));
    out.push(0x4f);
    out.extend([0x0d, 1]);
    out.extend(local(17));
    out.extend(local(19));
    out.extend(i32(2));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(load16(SELECTOR_NAME));
    out.extend(local(11));
    out.extend(local(19));
    out.extend(i32(2));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(load16(0));
    out.push(0x47);
    out.extend([0x04, 0x40]);
    out.extend(i32(0));
    out.extend(set_local(20));
    out.extend([0x0c, 2, 0x0b]);
    // Both strings terminate here. Their bytes after this UTF-16z terminator
    // are outside identity and must not affect the selected row.
    out.extend(local(11));
    out.extend(local(19));
    out.extend(i32(2));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(load16(0));
    out.push(0x45);
    out.extend([0x04, 0x40, 0x0c, 0x02, 0x0b]);
    out.extend(local(19));
    out.extend(i32(1));
    out.push(0x6a);
    out.extend(set_local(19));
    out.extend([0x0c, 0, 0x0b, 0x0b]);
    // A 20-unit unterminated field is not a bounded UTF-16z identity.
    out.extend(local(19));
    out.extend(i32(NAME_UNITS));
    out.push(0x46);
    out.extend([0x04, 0x40]);
    out.extend(i32(0));
    out.extend(set_local(20));
    out.push(0x0b);
    out.extend(local(20));
    out.extend([0x04, 0x40]);
    out.extend(local(5));
    out.extend(i32(-1));
    out.push(0x47);
    out.extend([0x04, 0x40]);
    stage(&mut out, 153);
    fail(&mut out);
    out.push(0x0b);
    out.extend(local(18));
    out.extend(set_local(5));
    out.extend(local(17));
    out.extend(set_local(21));
    out.push(0x0b);
    out.push(0x0b);
    out.extend(local(18));
    out.extend(i32(1));
    out.push(0x6a);
    out.extend(set_local(18));
    out.extend([0x0c, 0, 0x0b, 0x0b]);
    out.extend(local(5));
    out.extend(i32(-1));
    out.push(0x46);
    out.extend([0x04, 0x40]);
    // Preserve bounded model count in missing diagnostics, never client data.
    out.extend(local(16));
    out.extend(i32(15_400));
    out.push(0x6a);
    out.extend(set_global(globals.stage));
    fail(&mut out);
    out.push(0x0b);
    out.extend(local(9));
    out.extend(local(5));
    out.push(0x46);
    out.extend([0x04, 0x40]);
    // Only a real callback query matching target index publishes this row.
    out.extend(local(21));
    out.extend(i32(SELECTOR_NAME));
    out.push(0x6a);
    out.extend(set_global(globals.selected_name));
    success(&mut out);
    out.push(0x0b);
    // Walk only toward resolved target. Sparse UI arrays contain inactive holes;
    // skip those null entries, then issue one click for first real adjacent row.
    // `local(9)` and matched `local(5)` bound monotonic walk within model range.
    out.extend(local(9));
    out.extend(local(5));
    out.push(0x49);
    out.extend([0x04, 0x40]);
    out.extend(local(9));
    out.extend(i32(1));
    out.push(0x6a);
    out.extend(set_local(18));
    out.push(0x05);
    out.extend(local(9));
    out.extend(i32(1));
    out.push(0x6b);
    out.extend(set_local(18));
    out.push(0x0b);
    out.extend([0x02, 0x40, 0x03, 0x40]);
    out.extend(local(15));
    out.extend(local(18));
    out.extend(i32(4));
    out.push(0x6c);
    out.push(0x6a);
    out.extend(load(0));
    out.extend(set_local(21));
    out.extend(local(21));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    out.extend(local(9));
    out.extend(local(5));
    out.push(0x49);
    out.extend([0x04, 0x40]);
    out.extend(local(18));
    out.extend(i32(1));
    out.push(0x6a);
    out.extend(set_local(18));
    out.push(0x05);
    out.extend(local(18));
    out.extend(i32(1));
    out.push(0x6b);
    out.extend(set_local(18));
    out.push(0x0b);
    out.extend([0x0c, 1]);
    out.push(0x0b);
    ptr_ok(&mut out, 21, SELECTOR_NAME + NAME_UNITS * 2);
    out.extend([0x45, 0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(local(18));
    out.extend(set_local(5));
    out.extend([0x0c, 1, 0x0b, 0x0b]);
    stage(&mut out, 16);
    // Private button_param and kMouseAction packet; parent frame owns click.
    out.extend(local(2));
    out.extend(local(21));
    out.extend(i32(SELECTOR_NAME));
    out.push(0x6a);
    out.extend(store(24));
    out.extend(local(2));
    out.extend(i32(0));
    out.extend(store(28));
    out.extend(local(2));
    out.extend(local(6));
    out.extend(load(FRAME_ID));
    out.extend(store(0));
    out.extend(local(2));
    out.extend(local(6));
    out.extend(load(FRAME_CHILD_ID));
    out.extend(store(4));
    out.extend(local(2));
    out.extend(i32(8));
    out.extend(store(8));
    out.extend(local(2));
    out.extend(local(2));
    out.extend(i32(24));
    out.push(0x6a);
    out.extend(store(12));
    out.extend(local(2));
    out.extend(i32(0));
    out.extend(store(16));
    out.extend(local(6));
    out.extend(load(FRAME_ID));
    out.push(0x10);
    out.extend(uleb(proof.frame_parent as u64));
    out.extend(set_local(10));
    out.extend(local(10));
    out.push(0x45);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(local(10));
    out.push(0x10);
    out.extend(uleb(proof.frame_resolver as u64));
    out.extend(set_local(8));
    ptr_ok(&mut out, 8, FRAME_BYTES);
    out.extend([0x45, 0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(local(8));
    out.extend(load(FRAME_ID));
    out.extend(local(10));
    out.push(0x47);
    out.extend([0x04, 0x40]);
    fail(&mut out);
    out.push(0x0b);
    out.extend(local(8));
    out.extend(i32(FRAME_DISPATCH));
    out.push(0x6a);
    out.extend(i32(0x31));
    out.extend(local(2));
    out.extend(i32(0));
    out.push(0x10);
    out.extend(uleb(proof.frame_dispatch as u64));
    out.extend(local(5));
    out.extend(set_global(globals.expected));
    out.extend(i32(0));
    out.extend(set_global(globals.attempts));
    out.extend(i32(SELECT));
    out.extend(set_global(globals.pending));
    pending(&mut out);
    out.push(0x0b);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selector_emitter_reserves_and_restores_private_stack() {
        let proof = ActionProof {
            selector_hash: 0x3161_6b12,
            play_hash: 0,
            frame_child: 6796,
            frame_parent: 6797,
            frame_resolver: 6534,
            frame_dispatch: 6508,
            logout_producer: 0,
            frame_dispatch_offset: 0xa8,
            frame_array: 0x5a1fdc,
            frame_count: 0x5a1fe4,
            callback_rows_offset: 168,
            callback_count_offset: 176,
            callback_row_bytes: 12,
            callback_context_offset: 4,
            selector_context_rows_offset: 8,
            selector_context_count_offset: 16,
            selector_row_name_offset: 32,
            selector_index_message: 0x5a,
            selector_callback_slot: 2_957,
        };
        let globals = ActionGlobals {
            pending: 1,
            argument: 2,
            enabled: 3,
            result: 4,
            expected: 5,
            attempts: 6,
            stage: 7,
            uuid0: 8,
            uuid1: 9,
            uuid2: 10,
            uuid3: 11,
            target_set: 12,
            selected_name: 13,
        };
        let body = emit_selector_execute(proof, globals);
        let reserve = i32(64);
        assert!(
            body.windows(reserve.len())
                .any(|part| part == reserve.as_slice())
        );
        assert!(body.windows(2).any(|part| part == [0x21, 0x16]));
        assert!(body.windows(2).any(|part| part == [0x20, 0x16]));
        assert!(body.windows(2).any(|part| part == [0x24, 0x00]));
        assert!(body.windows(3).any(|part| part == [0x41, 0x31, 0x20]));
    }
}
