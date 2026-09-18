//! Executable regression fixtures for closed character-select UI readiness.
//!
//! The emitted reader is observational: Node supplies only the certified
//! frame identity callbacks and fixtures assert it returns exactly zero or one.

use super::super::codec::{
    Section, WASM_HEADER, encode_code, encode_index_vector, encode_section, uleb,
};
use super::{ActionProof, emit_ui_ready};

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
    let mut types = uleb(3);
    types.extend([0x60, 2, 0x7f, 0x7f, 1, 0x7f]); // child callback
    types.extend([0x60, 1, 0x7f, 1, 0x7f]); // parent/resolver
    types.extend([0x60, 0, 1, 0x7f]); // readiness reader
    let mut imports = uleb(3);
    imports.extend(import("child", 0));
    imports.extend(import("parent", 1));
    imports.extend(import("resolve", 1));
    let body = emit_ui_ready(ActionProof {
        selector_hash: 0x3161_6b12,
        play_hash: 0x0b04_1d2a,
        frame_child: 0,
        frame_parent: 1,
        frame_resolver: 2,
        frame_dispatch: 0,
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
    });
    let mut exports = uleb(2);
    for (name, kind, index) in [("ready", 0, 3), ("memory", 2, 0)] {
        exports.extend(uleb(name.len() as u64));
        exports.extend(name.as_bytes());
        exports.push(kind);
        exports.extend(uleb(index));
    }
    let mut wasm = WASM_HEADER.to_vec();
    wasm.extend(encode_section(&Section { id: 1, body: types }));
    wasm.extend(encode_section(&Section {
        id: 2,
        body: imports,
    }));
    wasm.extend(encode_section(&Section {
        id: 3,
        body: encode_index_vector(&[2]),
    }));
    wasm.extend(encode_section(&Section {
        id: 5,
        body: vec![1, 0, 100],
    }));
    wasm.extend(encode_section(&Section {
        id: 7,
        body: exports,
    }));
    wasm.extend(encode_section(&Section {
        id: 10,
        body: encode_code(&[body]),
    }));
    wasm
}

fn run(script: &str) {
    use std::{
        process::Command,
        thread,
        time::{Duration, Instant},
    };
    let temp = crate::scratch::TempDir::new("character-ui-ready-fixture");
    let wasm = temp.0.join("ui-ready.wasm");
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
const fs = require('fs'); let ex;
const instance = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(process.argv[1])), {env:{
 child: id => id === 0 ? 10 : 0,
 parent: id => id === 1 ? 20 : 0,
 resolve: id => ({10: selectorChild, 20: playParent})[id] || 0,
}}); ex = instance.exports;
const dv = new DataView(ex.memory.buffer), u32 = (at, value) => dv.setUint32(at, value >>> 0, true);
const frameArray = 0x5a1fdc, frameCount = 0x5a1fe4, rosterCount = 0x5a75f0;
const table = 0xf000, selector = 0x10000, play = 0x11000, selectorChild = 0x12000, playParent = 0x13000, callbackRows = 0x14000, context = 0x15000;
u32(rosterCount, 1); u32(frameCount, 2); u32(frameArray, table); u32(table, selector); u32(table+4, play);
u32(selector+0xbc, 0); u32(selector+0x134, 0x31616b12); u32(selector+0x18c, 4); u32(selector+0xa8, callbackRows); u32(selector+0xb0, 1);
u32(selectorChild+0xbc, 10); u32(callbackRows, 2957); u32(callbackRows+8, -1); u32(callbackRows+4, context); u32(context+4, 0);
u32(play+0xbc, 1); u32(play+0x134, 0x0b041d2a); u32(play+0x18c, 4); u32(playParent+0xbc, 20);
"#;

#[test]
fn ready_requires_both_certified_visible_frames_and_owned_context() {
    run(&format!(
        r#"{SETUP}
if (ex.ready() !== 1) process.exit(1);
u32(selector+0x18c, 0); if (ex.ready() !== 0) process.exit(2); u32(selector+0x18c, 4);
u32(play+0x18c, 0x204); if (ex.ready() !== 0) process.exit(3); u32(play+0x18c, 4);
u32(selector+0x134, 0); if (ex.ready() !== 0) process.exit(4); u32(selector+0x134, 0x31616b12);
u32(play+0xbc, 7); if (ex.ready() !== 0) process.exit(5); u32(play+0xbc, 1);
if (ex.ready() !== 1) process.exit(6);
"#
    ));
}

#[test]
fn ready_refuses_untrusted_bounds_or_callback_context() {
    run(&format!(
        r#"{SETUP}
const refuse = () => ex.ready() === 0;
u32(rosterCount, 0); if (!refuse()) process.exit(1); u32(rosterCount, 1);
u32(frameCount, 0); if (!refuse()) process.exit(2); u32(frameCount, 2);
u32(frameArray, 0); if (!refuse()) process.exit(3); u32(frameArray, table);
u32(table, 0); if (!refuse()) process.exit(4); u32(table, selector);
u32(selector+0xb0, 0); if (!refuse()) process.exit(5); u32(selector+0xb0, 1);
u32(callbackRows+4, 0); if (!refuse()) process.exit(6); u32(callbackRows+4, context);
u32(context+4, 9); if (!refuse()) process.exit(7); u32(context+4, 0);
u32(selectorChild+0xbc, 11); if (!refuse()) process.exit(8); u32(selectorChild+0xbc, 10);
u32(playParent+0xbc, 21); if (!refuse()) process.exit(9); u32(playParent+0xbc, 20);
if (ex.ready() !== 1) process.exit(10);
"#
    ));
}
