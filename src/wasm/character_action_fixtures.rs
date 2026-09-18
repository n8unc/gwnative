//! Executable regression fixtures for closed Selector action bytecode.
//!
//! These instantiate emitted Wasm with a small typed callback surface. They do
//! not accept opcode presence as behavior: dispatcher calls, carousel state,
//! malformed pointers, and global zero restoration are observed in Node.

use super::super::character_selector::emit_selector_execute;
use super::super::codec::{
    Section, WASM_HEADER, encode_code, encode_index_vector, encode_section, sleb, uleb,
};
use super::{ActionGlobals, ActionProof};

fn import(module: &str, name: &str, type_index: u32) -> Vec<u8> {
    let mut out = uleb(module.len() as u64);
    out.extend(module.as_bytes());
    out.extend(uleb(name.len() as u64));
    out.extend(name.as_bytes());
    out.push(0);
    out.extend(uleb(type_index as u64));
    out
}

fn fixture(body: Vec<u8>) -> Vec<u8> {
    let types = [
        vec![0x60, 2, 0x7f, 0x7f, 1, 0x7f],       // child and selector
        vec![0x60, 1, 0x7f, 1, 0x7f],             // parent/resolver
        vec![0x60, 4, 0x7f, 0x7f, 0x7f, 0x7f, 0], // dispatcher
    ];
    let mut type_body = uleb(types.len() as u64);
    for ty in types {
        type_body.extend(ty);
    }
    let mut imports = uleb(4);
    imports.extend(import("env", "child", 0));
    imports.extend(import("env", "parent", 1));
    imports.extend(import("env", "resolve", 1));
    imports.extend(import("env", "dispatch", 2));
    let mut globals = uleb(14);
    for value in [6_000_000i64, 0, 0, 1, 0, -1, 0, 0, 0, 0, 0, 0, 1, 0] {
        globals.extend([0x7f, 1, 0x41]);
        globals.extend(sleb(value));
        globals.push(0x0b);
    }
    let exports = [
        ("selector", 0, 4),
        ("memory", 2, 0),
        ("stack", 3, 0),
        ("pending", 3, 1),
        ("expected", 3, 5),
        ("attempts", 3, 6),
        ("stage", 3, 7),
        ("selected_name", 3, 13),
    ];
    let mut export_body = uleb(exports.len() as u64);
    for (name, kind, index) in exports {
        export_body.extend(uleb(name.len() as u64));
        export_body.extend(name.as_bytes());
        export_body.push(kind);
        export_body.extend(uleb(index));
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
        body: encode_index_vector(&[0]),
    }));
    wasm.extend(encode_section(&Section {
        id: 5,
        body: vec![1, 0, 93],
    }));
    wasm.extend(encode_section(&Section {
        id: 6,
        body: globals,
    }));
    wasm.extend(encode_section(&Section {
        id: 7,
        body: export_body,
    }));
    wasm.extend(encode_section(&Section {
        id: 10,
        body: encode_code(&[body]),
    }));
    wasm
}

fn emitted_module() -> Vec<u8> {
    let proof = ActionProof {
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
    };
    fixture(emit_selector_execute(
        proof,
        ActionGlobals {
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
        },
    ))
}

fn run(script: &str) {
    use std::process::Command;
    use std::time::{Duration, Instant};
    let temp = crate::scratch::TempDir::new("character-selector-fixture");
    let wasm = temp.0.join("selector.wasm");
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
            panic!("selector fixture exceeded 10 seconds");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success(), "selector fixture failed: {status}");
}

const SETUP: &str = r#"
const fs = require('fs');
let selected = 0, clicks = 0, delay = false, dropQuery = false, visible = ['A','B','C','Target'];
const selectorFrame = 0x10000, childFrame = 0x11000, parentFrame = 0x12000;
let ex;
const instance = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(process.argv[1])), {env:{
 child: () => 55, parent: () => 77,
 resolve: id => id === 55 ? childFrame : id === 77 ? parentFrame : 0,
 dispatch: (frame, message, a, payload) => {
   if (message === 0x5a) {
     if (frame !== childFrame + 0xa8 || a !== 0 || payload !== ex.stack.value + 32) throw new Error('wrong Selector query packet');
     if (!dropQuery) new DataView(ex.memory.buffer).setInt32(payload, selected, true);
   }
   if (message === 0x31) {
     if (frame !== parentFrame + 0xa8 || payload !== 0 || a !== ex.stack.value) throw new Error('wrong Selector click envelope');
     const packet = new DataView(ex.memory.buffer);
     if (packet.getUint32(a, true) !== 0 || packet.getUint32(a+4, true) !== 42 || packet.getUint32(a+8, true) !== 8 || packet.getUint32(a+12, true) !== a+24 || packet.getUint32(a+16, true) !== 0 || packet.getUint32(a+28, true) !== 0) throw new Error('wrong Selector click packet');
     const nameAt = packet.getUint32(a+24, true); let name = '';
     for (let i=0;i<20;i+=1) { const unit=packet.getUint16(nameAt+i*2,true); if (!unit) break; name += String.fromCharCode(unit); }
     const actual = visible.indexOf(name);
     if (actual < 0) throw new Error('unknown Selector click');
     const direction = actual > selected ? 1 : -1;
     for (let i = selected + direction; i !== actual; i += direction) if (visible[i] !== null) throw new Error('crossed a non-null Selector row');
     clicks += 1; if (!delay) selected = actual;
   }
 }
}}); ex = instance.exports;
const bytes = new Uint8Array(ex.memory.buffer), dv = new DataView(ex.memory.buffer);
const u32 = (at, value) => dv.setUint32(at, value >>> 0, true);
const name = (at, value) => { for (let i=0;i<20;i+=1) dv.setUint16(at+i*2, value.charCodeAt(i)||0, true); };
const frameArray = 0x5a1fdc, frameCount = 0x5a1fe4, rosterPtr = 0x5a75e8, rosterCount = 0x5a75f0;
const roster = 0x20000, frameTable = 0x0f000, callbackRows = 0x13000, context = 0x14000, choices = 0x15000;
u32(frameCount, 1); u32(frameArray, frameTable); u32(frameTable, selectorFrame); u32(rosterCount, 1); u32(rosterPtr, roster); name(roster+0x18, 'Target');
u32(selectorFrame+0xbc, 0); u32(selectorFrame+0x134, 0x31616b12); u32(selectorFrame+0x18c, 4); u32(selectorFrame+0xa8, callbackRows); u32(selectorFrame+0xb0, 1); u32(callbackRows, 2957); u32(callbackRows+4, context); u32(callbackRows+8, 0xffffffff);
u32(context+4, 0); u32(context+8, choices); u32(context+12, 4); u32(context+16, 4);
for (const [i, value] of ['A','B','C','Target'].entries()) { const at=0x16000+i*0x100; u32(choices+i*4,at); name(at+0x20,value); }
u32(selectorFrame+0xb8,42); u32(childFrame+0xbc,55); u32(parentFrame+0xbc,77);
const stack = ex.stack.value;
"#;

#[test]
fn carousel_continues_three_steps_without_repeating_a_delayed_click() {
    run(&format!(
        r#"{SETUP}
if (ex.selector(1,0) !== -1 || clicks !== 1 || ex.expected.value !== 1 || ex.selected_name.value !== 0 || ex.stack.value !== stack) process.exit(1);
selected = 0; delay = true; if (ex.selector(1,0) !== -1 || clicks !== 1 || ex.attempts.value !== 1 || ex.stack.value !== stack) process.exit(2);
delay = false; selected = 1; if (ex.selector(1,0) !== -1 || clicks !== 2 || ex.expected.value !== 2 || ex.stack.value !== stack) process.exit(3);
selected = 2; if (ex.selector(1,0) !== -1 || clicks !== 3 || ex.expected.value !== 3 || ex.stack.value !== stack) process.exit(4);
selected = 3; if (ex.selector(1,0) !== 1 || clicks !== 3 || ex.selected_name.value !== 0x16320 || ex.stack.value !== stack) process.exit(5);
// UTF-16 bytes after common terminator are not identity. The callback row and
// frozen roster name still refer to Target, so no extra carousel click occurs.
dv.setUint16(roster+0x18+16, 0x1111, true); dv.setUint16(0x16300+0x20+16, 0x2222, true);
if (ex.selector(1,0) !== 1 || clicks !== 3 || ex.stack.value !== stack) process.exit(10);
name(roster+0x18, 'A'); selected = 3;
if (ex.selector(1,0) !== -1 || clicks !== 4 || ex.expected.value !== 2 || ex.stack.value !== stack) process.exit(6);
if (ex.selector(1,0) !== -1 || clicks !== 5 || ex.expected.value !== 1 || ex.stack.value !== stack) process.exit(7);
if (ex.selector(1,0) !== -1 || clicks !== 6 || ex.expected.value !== 0 || ex.stack.value !== stack) process.exit(8);
if (ex.selector(1,0) !== 1 || clicks !== 6 || ex.stack.value !== stack) process.exit(9);
"#
    ));
}

#[test]
fn hidden_duplicate_and_bad_pointers_refuse_without_stack_or_click_side_effects() {
    run(&format!(
        r#"{SETUP}
const refuse = () => ex.selector(1,0) === -2 && clicks === 0 && ex.stack.value === stack;
u32(selectorFrame+0x18c,0); if (!refuse()) process.exit(1); u32(selectorFrame+0x18c,4);
name(0x16200+0x20,'Target'); u32(choices+2*4,0x16200); if (!refuse()) process.exit(2); name(0x16200+0x20,'C');
u32(rosterPtr,0); if (!refuse()) process.exit(3);
u32(rosterPtr,roster); u32(callbackRows,7); if (!refuse()) process.exit(4); u32(callbackRows,2957);
u32(callbackRows+4,0x640000); if (!refuse()) process.exit(5); u32(callbackRows+4,context);
u32(callbackRows+4,0); if (!refuse()) process.exit(6); u32(callbackRows+4,context);
"#
    ));
}

#[test]
fn first_active_callback_must_be_selector_while_inactive_rows_are_skipped() {
    run(&format!(
        r#"{SETUP}
u32(selectorFrame+0xb0,2); u32(callbackRows+12,7); u32(callbackRows+16,context); u32(callbackRows+20,0xffffffff);
if (ex.selector(1,0) !== -2 || clicks !== 0 || ex.stack.value !== stack) process.exit(1);
u32(callbackRows+20,0); if (ex.selector(1,0) !== -1 || clicks !== 1 || ex.stack.value !== stack) process.exit(2);
"#
    ));
}

#[test]
fn missing_selector_query_result_refuses_without_click_or_stack_change() {
    run(&format!(
        r#"{SETUP}
dropQuery = true;
if (ex.selector(1,0) !== -2 || clicks !== 0 || ex.stack.value !== stack || ex.stage.value !== 151) process.exit(1);
"#
    ));
}

#[test]
fn selector_refusal_diagnostics_follow_existing_query_and_scan_guards() {
    run(&format!(
        r#"{SETUP}
const refuse = stage => ex.selector(1,0) === -2 && clicks === 0 && ex.stack.value === stack && ex.stage.value === stage;
u32(choices+2*4, 0x640000); if (!refuse(157)) process.exit(1); u32(choices+2*4, 0x16200);
name(0x16100+0x20, 'Target'); if (!refuse(153)) process.exit(2); name(0x16100+0x20, 'B');
name(0x16300+0x20, 'Absent'); if (!refuse(15404)) process.exit(3);
"#
    ));
}

#[test]
fn sparse_shuffled_ui_skips_nulls_and_uses_nonzero_roster_target_identity() {
    run(&format!(
        r#"{SETUP}
u32(rosterCount, 9);
for (let i = 0; i < 9; i += 1) name(roster + i * 0x84 + 0x18, `Other${{i}}`);
name(roster + 0x84 + 0x18, 'Target');
visible = ['Other', null, 'PreTarget', 'Target', 'Later'];
u32(context+16, 5);
u32(choices, 0x16000); name(0x16000+0x20, 'Other');
u32(choices+4, 0);
u32(choices+8, 0x16200); name(0x16200+0x20, 'PreTarget');
u32(choices+12, 0x16300); name(0x16300+0x20, 'Target');
u32(choices+16, 0x16400); name(0x16400+0x20, 'Later');
if (ex.selector(1,1) !== -1 || clicks !== 1 || ex.expected.value !== 2 || ex.stack.value !== stack) process.exit(1);
selected = 2;
if (ex.selector(1,1) !== -1 || clicks !== 2 || ex.expected.value !== 3 || ex.stack.value !== stack) process.exit(2);
selected = 3;
if (ex.selector(1,1) !== 1 || clicks !== 2 || ex.selected_name.value !== 0x16320 || ex.stack.value !== stack) process.exit(3);
name(0x16400+0x20, 'Target'); visible[4] = 'Target';
if (ex.selector(1,1) !== -2 || clicks !== 2 || ex.stage.value !== 153 || ex.stack.value !== stack) process.exit(4);
"#
    ));
}

#[test]
fn sparse_shuffled_ui_skips_nulls_toward_a_lower_target_index() {
    run(&format!(
        r#"{SETUP}
u32(rosterCount, 9);
for (let i = 0; i < 9; i += 1) name(roster + i * 0x84 + 0x18, `Other${{i}}`);
name(roster + 0x84 + 0x18, 'Target');
visible = ['Before', 'Target', 'PreTarget', null, 'Other'];
u32(context+16, 5);
u32(choices, 0x16000); name(0x16000+0x20, 'Before');
u32(choices+4, 0x16100); name(0x16100+0x20, 'Target');
u32(choices+8, 0x16200); name(0x16200+0x20, 'PreTarget');
u32(choices+12, 0);
u32(choices+16, 0x16400); name(0x16400+0x20, 'Other');
selected = 4;
if (ex.selector(1,1) !== -1 || clicks !== 1 || ex.expected.value !== 2 || ex.stack.value !== stack) process.exit(1);
selected = 4;
if (ex.selector(1,1) !== -1 || clicks !== 1 || ex.expected.value !== 2 || ex.attempts.value !== 1 || ex.stack.value !== stack) process.exit(2);
selected = 2;
if (ex.selector(1,1) !== -1 || clicks !== 2 || ex.expected.value !== 1 || ex.stack.value !== stack) process.exit(3);
selected = 1;
if (ex.selector(1,1) !== 1 || clicks !== 2 || ex.selected_name.value !== 0x16120 || ex.stack.value !== stack) process.exit(4);
"#
    ));
}
