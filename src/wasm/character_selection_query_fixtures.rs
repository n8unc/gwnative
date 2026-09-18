//! Executable fixtures for callback-only live Selector-name query.

use super::super::codec::{
    Section, WASM_HEADER, encode_code, encode_index_vector, encode_section, sleb, uleb,
};
use super::{ActionProof, emit_current_selector_name};

fn import(name: &str, type_index: u32) -> Vec<u8> {
    let mut out = uleb(3);
    out.extend(b"env");
    out.extend(uleb(name.len() as u64));
    out.extend(name.as_bytes());
    out.push(0);
    out.extend(uleb(type_index as u64));
    out
}

fn proof() -> ActionProof {
    ActionProof {
        selector_hash: 0x3161_6b12,
        play_hash: 0,
        frame_child: 0,
        frame_parent: 1,
        frame_resolver: 2,
        frame_dispatch: 3,
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
    }
}

fn emitted_module() -> Vec<u8> {
    let types = [
        vec![0x60, 2, 0x7f, 0x7f, 1, 0x7f],
        vec![0x60, 1, 0x7f, 1, 0x7f],
        vec![0x60, 4, 0x7f, 0x7f, 0x7f, 0x7f, 0],
        vec![0x60, 0, 1, 0x7f],
    ];
    let mut type_body = uleb(types.len() as u64);
    for ty in types {
        type_body.extend(ty);
    }
    let mut imports = uleb(4);
    imports.extend(import("child", 0));
    imports.extend(import("parent", 1));
    imports.extend(import("resolve", 1));
    imports.extend(import("dispatch", 2));
    let mut exports = uleb(3);
    for (name, kind, index) in [("query", 0, 4), ("memory", 2, 0), ("stack", 3, 0)] {
        exports.extend(uleb(name.len() as u64));
        exports.extend(name.as_bytes());
        exports.push(kind);
        exports.extend(uleb(index));
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
        body: encode_index_vector(&[3]),
    }));
    wasm.extend(encode_section(&Section {
        id: 5,
        body: vec![1, 0, 93],
    }));
    wasm.extend(encode_section(&Section {
        id: 6,
        body: [vec![1, 0x7f, 1, 0x41], sleb(6_000_000), vec![0x0b]].concat(),
    }));
    wasm.extend(encode_section(&Section {
        id: 7,
        body: exports,
    }));
    wasm.extend(encode_section(&Section {
        id: 10,
        body: encode_code(&[emit_current_selector_name(proof())]),
    }));
    wasm
}

fn run(script: &str) {
    use std::process::Command;
    use std::time::{Duration, Instant};
    let temporary = crate::scratch::TempDir::new("character-selection-query");
    let wasm = temporary.0.join("query.wasm");
    std::fs::write(&wasm, emitted_module()).unwrap();
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
            let _ = child.kill();
            let _ = child.wait();
            panic!("selection-query fixture exceeded 10 seconds");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success(), "selection-query fixture failed: {status}");
}

const SETUP: &str = r#"
const fs = require('fs');
let selected = 1, writeQuery = true, queryCalls = 0, ex;
const selector = 0x10000, childFrame = 0x11000, frameTable = 0x0f000;
const callbackRows = 0x12000, model = 0x13000, rows = 0x14000;
const instance = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(process.argv[1])), {env:{
 child: (id, zero) => id === 0 && zero === 0 ? 55 : 0,
 parent: () => 0,
 resolve: id => id === 55 ? childFrame : 0,
 dispatch: (frame, message, zero, payload) => {
   queryCalls += 1;
   if (frame !== childFrame + 0xa8 || message !== 0x5a || zero !== 0 || payload !== ex.stack.value + 32) throw new Error('wrong query envelope');
   if (writeQuery) new DataView(ex.memory.buffer).setInt32(payload, selected, true);
 }
}}); ex = instance.exports;
const dv = new DataView(ex.memory.buffer), u32 = (at, value) => dv.setUint32(at, value >>> 0, true);
const name = (at, value) => { for (let i=0;i<20;i+=1) dv.setUint16(at+i*2, value.charCodeAt(i)||0, true); };
const frameArray=0x5a1fdc, frameCount=0x5a1fe4;
u32(frameCount, 1); u32(frameArray, frameTable); u32(frameTable, selector);
u32(selector+0xbc, 0); u32(selector+0x134, 0x31616b12); u32(selector+0x18c, 4);
u32(selector+0xa8, callbackRows); u32(selector+0xb0, 1);
u32(callbackRows, 2957); u32(callbackRows+4, model); u32(callbackRows+8, 0xffffffff);
u32(model+4, 0); u32(model+8, rows); u32(model+16, 3);
for (const [i, value] of ['A','B','C'].entries()) { const row=0x15000+i*0x100; u32(rows+i*4,row); name(row+32,value); }
u32(childFrame+0xbc,55); const stack=ex.stack.value;
const expectZero = () => { const before=queryCalls; if (ex.query() !== 0 || ex.stack.value !== stack) throw new Error('query did not refuse and restore stack'); return queryCalls-before; };
"#;

#[test]
fn query_returns_live_name_from_direct_row_context_model() {
    run(&format!(
        r#"{SETUP}
const expected = 0x15000 + selected * 0x100 + 32;
if (ex.query() !== expected || ex.stack.value !== stack || queryCalls !== 1) process.exit(1);
"#
    ));
}

#[test]
fn query_refuses_wrong_or_inactive_callback_and_malformed_live_data() {
    run(&format!(
        r#"{SETUP}
u32(callbackRows, 7); if (expectZero() !== 0) process.exit(1); u32(callbackRows,2957);
u32(callbackRows+8, 0); if (expectZero() !== 0) process.exit(2); u32(callbackRows+8,0xffffffff);
u32(callbackRows, 0); if (expectZero() !== 0) process.exit(3); u32(callbackRows,2957);
u32(callbackRows+4, 0x600000); if (expectZero() !== 0) process.exit(4); u32(callbackRows+4,model);
u32(rows+selected*4, 0x600000); if (expectZero() !== 1) process.exit(5); u32(rows+selected*4,0x15000+selected*0x100);
writeQuery=false; if (expectZero() !== 1) process.exit(6);
"#
    ));
}
