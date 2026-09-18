//! Executable queue and Play regression fixtures.
//!
//! Node instantiates emitted Wasm with only certified typed callbacks. Tests
//! observe queue state, packet dispatch, stack restoration, and every refusal.

use super::super::codec::{
    Section, WASM_HEADER, encode_code, encode_index_vector, encode_section, sleb, uleb,
};
use super::{
    ActionGlobals, ActionProof, emit_cancel, emit_configure, emit_drain, emit_enqueue,
    emit_execute, emit_play_execute, emit_status, emit_target,
};

const G: ActionGlobals = ActionGlobals {
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

fn import(name: &str, type_index: u32) -> Vec<u8> {
    let mut out = uleb(3);
    out.extend(b"env");
    out.extend(uleb(name.len() as u64));
    out.extend(name.as_bytes());
    out.push(0);
    out.extend(uleb(type_index as u64));
    out
}

fn fixture() -> Vec<u8> {
    let types = [
        vec![0x60, 1, 0x7f, 1, 0x7f],                   // parent and resolver
        vec![0x60, 4, 0x7f, 0x7f, 0x7f, 0x7f, 0],       // dispatch
        vec![0x60, 2, 0x7f, 0x7f, 1, 0x7f],             // action
        vec![0x60, 0, 1, 0x7f],                         // no-argument result
        vec![0x60, 0, 0],                               // drain
        vec![0x60, 4, 0x7f, 0x7f, 0x7f, 0x7f, 1, 0x7f], // frozen UUID target
    ];
    let mut type_body = uleb(types.len() as u64);
    for ty in types {
        type_body.extend(ty);
    }
    let mut imports = uleb(3);
    imports.extend(import("parent", 0));
    imports.extend(import("resolve", 0));
    imports.extend(import("dispatch", 1));
    let mut globals = uleb(14);
    for value in [0x5f0000i64, 0, 0, 0, 0, -1, 0, 0, 0, 0, 0, 0, 0, 0] {
        globals.extend([0x7f, 1, 0x41]);
        globals.extend(sleb(value));
        globals.push(0x0b);
    }
    // imports 0..2; play 3, execute 4, queue 5..9, target 10, live-name 11, selector stub 12.
    let bodies = [
        emit_play_execute(
            ActionProof {
                selector_hash: 0,
                play_hash: 0x0b04_1d2a,
                frame_child: 0,
                frame_parent: 0,
                frame_resolver: 1,
                frame_dispatch: 2,
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
            },
            G,
            11,
        ),
        emit_execute(12, 3, G),
        emit_enqueue(G),
        emit_cancel(G),
        emit_configure(G),
        emit_status(G),
        emit_drain(G, 4),
        emit_target(G),
        vec![0x00, 0x41, 0x80, 0xc0, 0x8c, 0x00, 0x0b], // current live name
        vec![0x00, 0x41, 0x07, 0x0b],
    ];
    let names = [
        ("play", 3),
        ("execute", 4),
        ("queue", 5),
        ("cancel", 6),
        ("configure", 7),
        ("status", 8),
        ("drain", 9),
        ("target", 10),
        ("live", 11),
        ("memory", 0),
        ("stack", 0),
    ];
    let mut exports = uleb(names.len() as u64);
    for (name, index) in names {
        exports.extend(uleb(name.len() as u64));
        exports.extend(name.as_bytes());
        exports.push(if name == "memory" {
            2
        } else if name == "stack" {
            3
        } else {
            0
        });
        exports.extend(uleb(if name == "memory" {
            0
        } else if name == "stack" {
            0
        } else {
            index
        }));
    }
    let mut wasm = WASM_HEADER.to_vec();
    wasm.extend(encode_section(&Section {
        id: 1,
        body: type_body,
    }));
    wasm.extend(encode_section(&Section {
        id: 2,
        body: imports,
    }));
    wasm.extend(encode_section(&Section {
        id: 3,
        body: encode_index_vector(&[2, 2, 2, 3, 0, 3, 4, 5, 3, 2]),
    }));
    wasm.extend(encode_section(&Section {
        id: 5,
        body: vec![1, 0, 100],
    }));
    wasm.extend(encode_section(&Section {
        id: 6,
        body: globals,
    }));
    wasm.extend(encode_section(&Section {
        id: 7,
        body: exports,
    }));
    wasm.extend(encode_section(&Section {
        id: 10,
        body: encode_code(&bodies),
    }));
    wasm
}

fn run(script: &str) {
    use std::{
        process::Command,
        thread,
        time::{Duration, Instant},
    };
    let temp = crate::scratch::TempDir::new("character-action-play-fixture");
    let wasm = temp.0.join("actions.wasm");
    std::fs::write(&wasm, fixture()).unwrap();
    let mut child = Command::new("node")
        .arg("-e")
        .arg(script)
        .arg(&wasm)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("fixture timed out");
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success(), "fixture exited {status}");
}

const SETUP: &str = r#"
const fs = require('fs'); let calls = []; let trace = []; let ex;
const instance = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(process.argv[1])), {env:{
 parent: id => { trace.push(['parent',id]); return id === 10 ? 20 : 0; }, resolve: id => { trace.push(['resolve',id]); return id === 20 ? 0x21000 : 0; },
 dispatch: (parent, message, arg, packet) => { calls.push([parent,message,arg,packet]); }
}}); ex = instance.exports; const dv = new DataView(ex.memory.buffer);
const u32 = (at, value) => dv.setUint32(at, value >>> 0, true);
const count = 0x5a75f0, rosterPtr = 0x5a75e8, roster = 0x30000, live = 0x32000, table = 0x5a1fdc, tableCount = 0x5a1fe4, array = 0x19000, frame = 0x20000, parent = 0x21000;
u32(count, 1); u32(rosterPtr, roster); [1,2,3,4].forEach((value, index) => u32(roster+8+index*4, value));
// Fixed non-empty UTF-16 selected name and roster row name, both terminated.
[67,121,99,108,111,110,101,0].forEach((unit, index) => { dv.setUint16(live+index*2, unit, true); dv.setUint16(roster+24+index*2, unit, true); });
u32(tableCount, 11); u32(table, array); u32(array+10*4, frame); u32(frame+0xbc,10); u32(frame+0x134,0x0b041d2a); u32(frame+0x18c,4); u32(frame+0x1c4,0xabc); u32(frame+0xb8,0xdef); u32(parent+0xbc,20);
const stack = ex.stack.value;
"#;

#[test]
fn queue_accepts_only_closed_actions_and_drains_once() {
    run(&format!(
        r#"{SETUP}
if (ex.configure(1)!==1 || ex.target(0,0,0,0)!==0 || ex.target(1,2,3,4)!==1 || ex.target(5,6,7,8)!==0) process.exit(1);
if (ex.queue(1,0)!==1 || ex.status()!==-1 || ex.configure(1)!==1 || ex.queue(1,0)!==0) process.exit(2);
if (ex.queue(1,63)!==0 || ex.status()!==-1) process.exit(3);
ex.drain(); if (ex.status()!==7) process.exit(4); ex.drain(); if (ex.status()!==7) process.exit(5);
if (ex.queue(9,0)!==0 || ex.status()!==-2 || ex.queue(1,64)!==0 || ex.status()!==-2 || ex.queue(1,-1)!==0 || ex.status()!==-2 || ex.queue(2,1)!==0 || ex.status()!==-2) {{ console.error('status', ex.status()); process.exit(6); }}
ex.configure(0); if (ex.queue(1,0)!==0 || ex.status()!==-2) process.exit(7);
ex.configure(1); if (ex.target(1,2,3,4)!==1 || ex.queue(1,63)!==1) process.exit(8); ex.cancel(); if (ex.target(1,2,3,4)!==1) process.exit(9); ex.cancel(); ex.drain(); if (ex.status()!==0 || calls.length!==0) process.exit(10);
"#
    ));
}

#[test]
fn play_dispatches_one_exact_packet_and_refuses_invalid_live_state() {
    run(&format!(
        r#"{SETUP}
if (ex.configure(1)!==1 || ex.target(1,2,3,4)!==1) process.exit(1); if (ex.live()!==live) {{ console.error('live',ex.live(),live); process.exit(25); }}
const initial = ex.play(2,0); if (initial!==1 || calls.length!==1 || ex.stack.value!==stack) {{ console.error('play', initial, calls.length, ex.stack.value, stack, trace); process.exit(2); }}
const [,message,packet,arg] = calls[0]; if (message!==0x31 || arg!==0 || dv.getUint32(packet,true)!==0xdef || dv.getUint32(packet+4,true)!==0xdef || dv.getUint32(packet+8,true)!==7 || dv.getUint32(packet+12,true)!==packet+24 || dv.getUint32(packet+16,true)!==0 || dv.getUint32(packet+24,true)!==0 || dv.getUint32(packet+28,true)!==0xabc) process.exit(3);
const refuse = () => ex.play(2,0)===-2 && calls.length===1 && ex.stack.value===stack;
u32(frame+0x18c,0); if (!refuse()) process.exit(4); u32(frame+0x18c,4);
u32(frame+0xbc,1); if (!refuse()) process.exit(5); u32(frame+0xbc,10);
u32(array+10*4,0); if (!refuse()) process.exit(7); u32(array+10*4,frame);
u32(array+10*4,0x640000); if (!refuse()) process.exit(7); u32(array+10*4,frame);
u32(parent+0xbc,21); if (!refuse()) process.exit(8); u32(parent+0xbc,20);
u32(tableCount,0); if (!refuse()) process.exit(9); u32(tableCount,11);
u32(roster+8,9); if (!refuse()) process.exit(10); u32(roster+8,1);
dv.setUint16(live, 0, true); if (!refuse()) process.exit(11); dv.setUint16(live, 67, true);
dv.setUint16(live, 65, true); if (!refuse()) process.exit(12); dv.setUint16(live, 67, true);
dv.setUint16(roster+24, 0, true); if (!refuse()) process.exit(13); dv.setUint16(roster+24, 67, true);
u32(count,2); [67,121,99,108,111,110,101,0].forEach((unit, index) => dv.setUint16(roster+0x84+24+index*2, unit, true)); if (!refuse()) process.exit(14); u32(count,1);
// Callback-time checks bind queued Play to live selected-name and UUID state.
if (ex.queue(2,0)!==1) process.exit(15); dv.setUint16(live, 65, true); ex.drain(); if (ex.status()!==-2 || calls.length!==1) process.exit(16); dv.setUint16(live, 67, true);
if (ex.queue(2,0)!==1) process.exit(17); u32(roster+8,9); ex.drain(); if (ex.status()!==-2 || calls.length!==1) process.exit(18); u32(roster+8,1);
u32(count,2); if (ex.queue(2,0)!==1) process.exit(19); ex.drain(); if (ex.status()!==-2 || calls.length!==1) process.exit(20); u32(count,1);
if (ex.queue(2,0)!==1) process.exit(21); ex.drain(); if (ex.status()!==1 || calls.length!==2) {{ console.error('valid queue', ex.status(), calls.length, trace); process.exit(22); }}
dv.setUint16(live+16, 88, true); dv.setUint16(roster+24+16, 99, true); if (ex.play(2,0)!==1 || calls.length!==3) process.exit(23);
ex.stack.value = 32; if (ex.play(2,0)!==-2 || calls.length!==3 || ex.stack.value!==32) process.exit(24);
"#
    ));
}
