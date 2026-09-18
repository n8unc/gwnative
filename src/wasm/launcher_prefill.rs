//! Exact-pair launcher credential-request gate.
//!
//! This is deliberately separate from signed template certificates.  It has no
//! game-state layout and does not authorize their transforms: it pins one
//! reviewed official Wasm/glue pair per runtime, then adds a private mutable
//! flag for `Prefs/SavePassword` and an account-name pointer used only at
//! login-screen initialization. The callback account-match check stays intact.

use wasmparser::{BinaryReader, ImportSectionReader, TypeRef};

use super::certificate::Runtime;
use super::codec::{
    Section, WASM_HEADER, encode_code, encode_index_vector, encode_section, parse_code,
    parse_index_vector, read_uleb, section_by_id, split_sections, uleb,
};
use super::{Outcome, digest};

pub(super) const ABI: u32 = 2;
pub(super) const EXPORT: &str = "GwnativeSetLauncherCredentialsAvailable";

const NAME_EXPORT: &str = "GwnativeSetLauncherAccountName";

const JSPI_WASM: &str = "1eb07332632e2fca8aabf5baa14fa1a1e6a2a59ec7134dfb8f6231d924c9fd7b";
const JSPI_GLUE: &str = "4ee7c8af5aa5f5c2e9a1334642fdc52075464da4d203fc676c7becb31669a2a8";
const ASYNCIFY_WASM: &str = "373c65abcfe6c26161ffd59cb8ea298eb3d1814f4778aeaf24d8978c1fdfa8ef";
const ASYNCIFY_GLUE: &str = "e6d031a22d9047f30a1e2d1d2e25f6daca552df4a5dc1e3fad83b57bf404463c";

struct Policy {
    wasm: &'static str,
    glue: &'static str,
    output: &'static str,
    getter_body: &'static str,
    getter_type: u32,
    login_body: &'static str,
    name_anchor: &'static [u8],
    globals: u32,
}
fn policy(runtime: Runtime) -> Policy {
    match runtime {
        Runtime::Jspi => Policy {
            wasm: JSPI_WASM,
            glue: JSPI_GLUE,
            output: "e2ad01cc8f3ccb82263399b72ec596f47e9a5a7439e640f7089463cb5abe07db",
            getter_body: "57f46245907727645ed8b540b4f6523747e7954390ec454d7f667963beb99e33",
            getter_type: 12,
            login_body: "baff95ca8838a6a0bcafa94bb09f3885575a999a3494973a2bd0bf63ae94de7f",
            name_anchor: &[
                0x02, 0x40, 0x20, 0x04, 0x45, 0x0d, 0x00, 0x20, 0x04, 0x2f, 0x01, 0x00, 0x45, 0x0d,
                0x00, 0x20, 0x06, 0x20, 0x04, 0x10, 0xbb, 0xd7, 0x80, 0x80, 0x00,
            ],
            globals: 10,
        },
        Runtime::Asyncify => Policy {
            wasm: ASYNCIFY_WASM,
            glue: ASYNCIFY_GLUE,
            output: "302de822367f2088e9dd040435f92799af9bc0efb5d23677acd4d0f3e68026ab",
            getter_body: "2ea3e2a34421d88ae476843bf84a7d5c775d432182dc4dce3e5cccb99368bb91",
            getter_type: 3,
            login_body: "345bb23373373a32c664bc7b71ddc3ae3e5beef4a29ac2d8ecaa181580e0ae73",
            name_anchor: &[
                0x20, 0x04, 0x21, 0x65, 0x20, 0x65, 0x45, 0x21, 0x66, 0x20, 0x66, 0x0d, 0x01, 0x20,
                0x04, 0x21, 0x67, 0x20, 0x67, 0x2f, 0x01, 0x00,
            ],
            globals: 12,
        },
    }
}

pub(super) fn matches(runtime: Runtime, wasm: &str, glue: &str) -> bool {
    let p = policy(runtime);
    (wasm, glue) == (p.wasm, p.glue)
}

fn imports(section: &[u8]) -> Outcome<(u32, u32)> {
    let mut functions = 0;
    let mut globals = 0;
    let reader = ImportSectionReader::new(BinaryReader::new(section, 0))
        .map_err(|e| format!("launcher-prefill: imports: {e}"))?;
    for import in reader.into_imports() {
        match import
            .map_err(|e| format!("launcher-prefill: import: {e}"))?
            .ty
        {
            TypeRef::Func(_) => functions += 1,
            TypeRef::Global(_) => globals += 1,
            _ => {}
        }
    }
    Ok((functions, globals))
}

fn type_count(section: &[u8]) -> Outcome<u32> {
    let mut p = 0;
    let count = read_uleb(section, &mut p)?;
    // Current exact modules use only function types. Consume every entry so
    // append location is checked rather than assumed.
    for _ in 0..count {
        if section.get(p) != Some(&0x60) {
            return Err("launcher-prefill: non-function type".into());
        }
        p += 1;
        for _ in 0..2 {
            let n = read_uleb(section, &mut p)? as usize;
            p = p
                .checked_add(n)
                .filter(|x| *x <= section.len())
                .ok_or("launcher-prefill: truncated type")?;
        }
    }
    if p != section.len() {
        return Err("launcher-prefill: malformed type section".into());
    }
    Ok(count)
}

fn local_prefix(body: &[u8]) -> Outcome<usize> {
    let mut p = 0;
    let n = read_uleb(body, &mut p)?;
    for _ in 0..n {
        let _ = read_uleb(body, &mut p)?;
        p = p
            .checked_add(1)
            .filter(|x| *x <= body.len())
            .ok_or("launcher-prefill: truncated local")?;
    }
    if p >= body.len() || body.last() != Some(&0x0b) {
        return Err("launcher-prefill: malformed getter body".into());
    }
    Ok(p)
}

fn export_has_name(section: &[u8], wanted: &str) -> Outcome<bool> {
    let mut p = 0;
    let n = read_uleb(section, &mut p)?;
    for _ in 0..n {
        let len = read_uleb(section, &mut p)? as usize;
        let end = p
            .checked_add(len)
            .filter(|x| *x <= section.len())
            .ok_or("launcher-prefill: truncated export name")?;
        if &section[p..end] == wanted.as_bytes() {
            return Ok(true);
        }
        p = end;
        let kind = *section.get(p).ok_or("launcher-prefill: truncated export")?;
        p += 1;
        match kind {
            0..=3 => {
                let _ = read_uleb(section, &mut p)?;
            }
            _ => return Err("launcher-prefill: invalid export kind".into()),
        }
    }
    if p != section.len() {
        return Err("launcher-prefill: malformed export section".into());
    }
    Ok(false)
}

fn replace(sections: &mut [Section], id: u8, body: Vec<u8>) -> Outcome<()> {
    let item = sections
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("launcher-prefill: missing section {id}"))?;
    item.body = body;
    Ok(())
}

/// Produce flag-aware getter.  `None` means current artifact pair is not reviewed.
pub(super) fn rewrite(runtime: Runtime, input: &[u8], glue: &[u8]) -> Outcome<Option<Vec<u8>>> {
    let p = policy(runtime);
    if !matches(runtime, &digest(input), &digest(glue)) {
        return Ok(None);
    }
    rewrite_checked(runtime, input, Some(p.output)).map(Some)
}

/// Compose after an independently verified signed template output. Caller must
/// compare `input` with certificate's signed output hash before this call.
pub(super) fn rewrite_certified(
    runtime: Runtime,
    input: &[u8],
    glue: &[u8],
) -> Outcome<Option<Vec<u8>>> {
    if digest(glue) != policy(runtime).glue {
        return Ok(None);
    }
    rewrite_checked(runtime, input, None).map(Some)
}

fn rewrite_checked(
    runtime: Runtime,
    input: &[u8],
    expected_output: Option<&str>,
) -> Outcome<Vec<u8>> {
    let p = policy(runtime);
    wasmparser::validate(input).map_err(|e| format!("launcher-prefill: invalid input: {e}"))?;
    let mut sections = split_sections(input)?;
    let (imported_functions, imported_globals) = imports(section_by_id(&sections, 2)?)?;
    let mut functions = parse_index_vector(section_by_id(&sections, 3)?)?;
    let mut bodies = parse_code(section_by_id(&sections, 10)?)?;
    if functions.len() != bodies.len() {
        return Err("launcher-prefill: function/code count differs".into());
    }
    let getter = 10_797u32
        .checked_sub(imported_functions)
        .ok_or("launcher-prefill: getter import underflow")? as usize;
    if functions.get(getter) != Some(&p.getter_type)
        || digest(
            bodies
                .get(getter)
                .ok_or("launcher-prefill: getter missing")?,
        ) != p.getter_body
    {
        return Err("launcher-prefill: getter guard failed".into());
    }
    if export_has_name(section_by_id(&sections, 7)?, EXPORT)?
        || export_has_name(section_by_id(&sections, 7)?, NAME_EXPORT)?
    {
        return Err("launcher-prefill: export collision".into());
    }
    let types = type_count(section_by_id(&sections, 1)?)?;
    let mut global_body = section_by_id(&sections, 6)?.to_vec();
    let mut gp = 0;
    let globals = read_uleb(&global_body, &mut gp)?;
    if globals != p.globals || gp == global_body.len() {
        return Err("launcher-prefill: global guard failed".into());
    }
    let flag_global = imported_globals + globals;
    let name_global = flag_global + 1;
    let login = 11_302u32
        .checked_sub(imported_functions)
        .ok_or("launcher-prefill: login import underflow")? as usize;
    let login_body = bodies.get(login).ok_or("launcher-prefill: login missing")?;
    if digest(login_body) != p.login_body {
        return Err("launcher-prefill: login body guard failed".into());
    }
    let offsets: Vec<_> = login_body
        .windows(p.name_anchor.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == p.name_anchor).then_some(offset))
        .collect();
    if offsets.len() != 1 {
        return Err("launcher-prefill: login name anchor guard failed".into());
    }
    // Seed only the initial account-name field, before its existing setter and
    // credential request. The later username-match check is unchanged. Asyncify's
    // anchor sits inside its normal-execution guard, never its rewind path.
    let mut name_seed = vec![0x23];
    name_seed.extend_from_slice(&uleb(u64::from(flag_global)));
    name_seed.push(0x23);
    name_seed.extend_from_slice(&uleb(u64::from(name_global)));
    name_seed.extend_from_slice(&[0x45, 0x45, 0x71, 0x04, 0x40, 0x23]);
    name_seed.extend_from_slice(&uleb(u64::from(name_global)));
    name_seed.extend_from_slice(&[0x21, 0x04, 0x0b]);
    let mut next_login = login_body.clone();
    next_login.splice(offsets[0]..offsets[0], name_seed);
    bodies[login] = next_login;
    // Rebuild type count with original payload after its vector count.
    let type_section = section_by_id(&sections, 1)?;
    let mut tp = 0;
    let _ = read_uleb(type_section, &mut tp)?;
    let mut next_type = uleb(u64::from(types + 1));
    next_type.extend_from_slice(&type_section[tp..]);
    next_type.extend_from_slice(&[0x60, 0x01, 0x7f, 0x00]);
    functions.push(types);
    functions.push(types);
    let body = &bodies[getter];
    let prefix = local_prefix(body)?;
    let mut getter_body = body[..prefix].to_vec();
    // `override == 0` must leave a real remembered setting untouched. Only a
    // nonzero override forces the gate on.
    getter_body.extend_from_slice(&[0x20, 0x00, 0x41, 0xdf, 0x00, 0x46, 0x04, 0x7f, 0x23]);
    getter_body.extend_from_slice(&uleb(u64::from(flag_global)));
    getter_body.extend_from_slice(&[0x04, 0x7f, 0x41, 0x01, 0x05]);
    getter_body.extend_from_slice(&body[prefix..body.len() - 1]);
    getter_body.push(0x0b);
    getter_body.push(0x05);
    getter_body.extend_from_slice(&body[prefix..body.len() - 1]);
    getter_body.extend_from_slice(&[0x0b, 0x0b]);
    bodies[getter] = getter_body;
    let mut setter = vec![0x00, 0x20, 0x00, 0x45, 0x45, 0x24];
    setter.extend_from_slice(&uleb(u64::from(flag_global)));
    setter.push(0x0b);
    bodies.push(setter);
    let mut name_setter = vec![0x00, 0x20, 0x00, 0x24];
    name_setter.extend_from_slice(&uleb(u64::from(name_global)));
    name_setter.push(0x0b);
    bodies.push(name_setter);
    global_body.truncate(gp);
    global_body.splice(.., uleb(u64::from(globals + 2)));
    global_body.extend_from_slice(&section_by_id(&sections, 6)?[gp..]);
    global_body.extend_from_slice(&[0x7f, 0x01, 0x41, 0x00, 0x0b]);
    global_body.extend_from_slice(&[0x7f, 0x01, 0x41, 0x00, 0x0b]);
    let export_section = section_by_id(&sections, 7)?;
    let mut ep = 0;
    let exports = read_uleb(export_section, &mut ep)?;
    let mut next_exports = uleb(u64::from(exports + 2));
    next_exports.extend_from_slice(&export_section[ep..]);
    for (offset, export) in [EXPORT, NAME_EXPORT].iter().enumerate() {
        next_exports.extend_from_slice(&uleb(export.len() as u64));
        next_exports.extend_from_slice(export.as_bytes());
        next_exports.push(0);
        next_exports.extend_from_slice(&uleb(u64::from(
            imported_functions + functions.len() as u32 - 2 + offset as u32,
        )));
    }
    replace(&mut sections, 1, next_type)?;
    replace(&mut sections, 3, encode_index_vector(&functions))?;
    replace(&mut sections, 6, global_body)?;
    replace(&mut sections, 7, next_exports)?;
    replace(&mut sections, 10, encode_code(&bodies))?;
    let mut output = WASM_HEADER.to_vec();
    for section in &sections {
        output.extend_from_slice(&encode_section(section));
    }
    wasmparser::validate(&output).map_err(|e| format!("launcher-prefill: invalid output: {e}"))?;
    if expected_output.is_some_and(|expected| digest(&output) != expected) {
        return Err(format!(
            "launcher-prefill: output guard failed: {}",
            digest(&output)
        ));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use super::*;

    #[test]
    fn reviewed_pairs_export_a_reversible_launcher_gate() {
        for (runtime, wasm, glue, name) in [
            (
                Runtime::Jspi,
                include_bytes!("../../web/Gw.jspi.wasm").as_slice(),
                include_bytes!("../../web/Gw.jspi.js").as_slice(),
                "jspi",
            ),
            (
                Runtime::Asyncify,
                include_bytes!("../../web/Gw.wasm").as_slice(),
                include_bytes!("../../web/Gw.js").as_slice(),
                "asyncify",
            ),
        ] {
            let output = rewrite(runtime, wasm, glue).unwrap().unwrap();
            let path = std::env::temp_dir().join(format!(
                "gwnative-launcher-prefill-{name}-{}.wasm",
                std::process::id()
            ));
            fs::write(&path, &output).unwrap();
            let mut probe = Command::new("node");
            probe
                .arg("scripts/probe-wasm-launcher-prefill.mjs")
                .arg(name)
                .arg(&path)
                .current_dir(env!("CARGO_MANIFEST_DIR"));
            let status = probe.status().unwrap();
            fs::remove_file(path).unwrap();
            assert!(status.success());
        }
    }
}
