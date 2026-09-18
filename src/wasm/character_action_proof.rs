//! Exact-build binding for preferred-character native actions.
//!
//! `f6534` is certified as bounded native ID resolver. Its table stores frame
//! pointers; generated code enumerates same table only while `id < count` and
//! compares frame caption hash. This avoids stale visible-registry globals.
//!
//! Label-hash translation follows GPL-3.0-only GWoNmac's
//! `enhancement-pre-game-proof.ts`; implementation below was independently
//! checked against current `f365` operator sequence.

use wasmparser::{
    BinaryReader, DataKind, ElementItems, ElementKind, FunctionBody, ImportSectionReader, Operator,
    Parser, Payload, TypeRef, Validator,
};

use super::character_actions::ActionProof;
use super::codec::{parse_code, section_by_id, split_sections};
use super::{Outcome, digest};

const JSPI_PREFILL_SHA256: &str =
    "e2ad01cc8f3ccb82263399b72ec596f47e9a5a7439e640f7089463cb5abe07db";

const LABEL_HASH_FUNCTION: u32 = 365;
const LABEL_HASH_BODY: &str = "9f937282e191571677a8f87f49635de095b93269f32af7e766a76ff505d80595";
const HASH_TABLE_ADDRESS: usize = 1_249_904;
const HASH_TABLE_SHA256: &str = "7d3ef45c38a522afbdc9e7fda537a0fc1eee1c1182400b83e7ed4ae825f02220";

const FRAME_CHILD: u32 = 6_796;
const FRAME_CHILD_BODY: &str = "9f73f1018d0bf99fd0d16b6ede0921dbe29cf70a4da4a61c9b24c1e68dbb0bf0";
const FRAME_PARENT: u32 = 6_797;
const FRAME_PARENT_BODY: &str = "46c90817c6ab335d5b8d57fdc1e38abd146c2b123dd8dcf0f08aca8245b8a9f2";
const FRAME_RESOLVER: u32 = 6_534;
const FRAME_ARRAY: u32 = 0x5a3b1c;
const FRAME_COUNT: u32 = 0x5a3b24;
const FRAME_RESOLVER_BODY: &str =
    "f0d5e7c4c71f920541037b1225613e334e2476723a427cab5c2688538265eb47";
const FRAME_CONSTRUCTOR: u32 = 6_676;
const FRAME_CONSTRUCTOR_BODY: &str =
    "1d5c9323bc4bdcc4d061aebe566b33c4aeff8a5ad5076b95b6a2dbc159cbe7ea";
const FRAME_INITIALIZER: u32 = 6_648;
const FRAME_INITIALIZER_BODY: &str =
    "c192ca8cbc1f34eb0fd50d43380e725096a4af6cdd35d7dcd7f1e59f212add5f";
const FRAME_HASH_INITIALIZER: u32 = 6_548;
const FRAME_HASH_INITIALIZER_BODY: &str =
    "4b2678c4a9fb6ca41c7726012073463fd8297299335d0531d78259a1c137ec68";
const FRAME_CHILD_LOOKUP: u32 = 6_560;
const FRAME_CHILD_LOOKUP_BODY: &str =
    "f353d6f6cfb1c40dacade9e078618b380602561c6e2032968db544d0ccb17b31";
const FRAME_STATE_CONSUMER: u32 = 6_502;
const FRAME_STATE_CONSUMER_BODY: &str =
    "d02743fe4f5055520fd2215a7a93367442e301728ced3318c2ee0db0ab0ef477";
const SELECTOR_CONSTRUCTOR: u32 = 12_030;
const SELECTOR_CONSTRUCTOR_BODY: &str =
    "36a2a74bf2016674a8370bccf5e8cc1fa26235afadb658ef024f4b36b99d51aa";
const SELECTOR_CALLBACK_SLOT: u32 = 2_957;
const SELECTOR_CALLBACK_WRAPPER: u32 = 11_950;
const SELECTOR_CALLBACK_WRAPPER_BODY: &str =
    "4349230af1880aab7fd39a3048102211db2a12ff31a68f53e42b31ff44a451a6";
const SELECTOR_CALLBACK_HANDLER: u32 = 11_951;
const SELECTOR_CALLBACK_HANDLER_BODY: &str =
    "a4622d2712d0b31156709c2aa8a5adb348aecc2438e3ae1468e81a7be17b931b";
const SELECTOR_QUERY_DISPATCH: u32 = 6_841;
const SELECTOR_QUERY_DISPATCH_BODY: &str =
    "29041a4f537194fae813ddd84c99a06b6f716dd6fe2b4adc64d60f32857abd9f";
const FRAME_BYTES: u64 = 456;
const FRAME_ID_OFFSET: u64 = 188;
const FRAME_HASH_OFFSET: u64 = 308;
const FRAME_MAP_OFFSET: u64 = 296;
const FRAME_MAP_HASH_OFFSET: u64 = 12;
const FRAME_STATE_OFFSET: u64 = 396;
const FRAME_CALLBACK_ROWS_OFFSET: u32 = 168;
const FRAME_CALLBACK_COUNT_OFFSET: u32 = 176;
const FRAME_CALLBACK_ROW_BYTES: u32 = 12;
const FRAME_CALLBACK_CONTEXT_OFFSET: u32 = 4;
const SELECTOR_CONTEXT_FRAME_ID_OFFSET: u32 = 4;
const SELECTOR_CONTEXT_ROWS_OFFSET: u32 = 8;
const SELECTOR_CONTEXT_COUNT_OFFSET: u32 = 16;
const SELECTOR_ROW_NAME_OFFSET: u32 = 32;
const SELECTOR_INDEX_MESSAGE: u32 = 0x5a;
const ID_MANAGER_ARRAY: u64 = 5_913_372;
const ID_MANAGER_COUNT: u64 = 5_913_380;
const FRAME_DISPATCH: u32 = 6_508;
const FRAME_DISPATCH_BODY: &str =
    "ccf496f855fa579dac0d1ea86b95b6a6db21104d2a41b1d03c6bd213ee26ca7e";
const LOGOUT_PRODUCER: u32 = 12_434;
const LOGOUT_PRODUCER_BODY: &str =
    "b618abba3579ffe6f149a23e2550f6b86571b0f3beaa30acea059153f7cd6b06";

const FRAME_DISPATCH_OFFSET: u32 = 0xa8;
const MAX_STATIC_MEMORY: usize = 16 * 1024 * 1024;

fn imported_functions(section: &[u8]) -> Outcome<u32> {
    let reader = ImportSectionReader::new(BinaryReader::new(section, 0))
        .map_err(|error| format!("character-action-proof: imports: {error}"))?;
    let mut count = 0u32;
    for item in reader.into_imports() {
        if matches!(
            item.map_err(|error| format!("character-action-proof: import: {error}"))?
                .ty,
            TypeRef::Func(_) | TypeRef::FuncExact(_)
        ) {
            count = count
                .checked_add(1)
                .ok_or("character-action-proof: imports overflow")?;
        }
    }
    Ok(count)
}

fn body<'a>(bodies: &'a [Vec<u8>], imports: u32, index: u32) -> Outcome<&'a [u8]> {
    bodies
        .get(
            index
                .checked_sub(imports)
                .ok_or("character-action-proof: imported function")? as usize,
        )
        .map(Vec::as_slice)
        .ok_or("character-action-proof: missing function".into())
}

fn exact_body(
    bodies: &[Vec<u8>],
    imports: u32,
    index: u32,
    expected: &str,
    role: &str,
) -> Outcome<()> {
    if digest(body(bodies, imports, index)?) != expected {
        return Err(format!("character-action-proof: {role} changed"));
    }
    Ok(())
}

fn hash_table_offset(body: &[u8]) -> Outcome<usize> {
    let mut operators = FunctionBody::new(BinaryReader::new(body, 0))
        .get_operators_reader()
        .map_err(|error| format!("character-action-proof: hash body: {error}"))?;
    let mut offsets = Vec::new();
    while !operators.eof() {
        if let Operator::I32Load { memarg } = operators
            .read()
            .map_err(|error| format!("character-action-proof: hash operator: {error}"))?
        {
            let offset = usize::try_from(memarg.offset)
                .map_err(|_| "character-action-proof: hash table offset overflow")?;
            if offset > 1_000_000 {
                offsets.push(offset);
            }
        }
    }
    match offsets.as_slice() {
        [offset] => Ok(*offset),
        _ => Err("character-action-proof: label hash table load ambiguous".into()),
    }
}

fn i32_load_offsets(body: &[u8]) -> Outcome<Vec<u64>> {
    let mut operators = FunctionBody::new(BinaryReader::new(body, 0))
        .get_operators_reader()
        .map_err(|error| format!("character-action-proof: body: {error}"))?;
    let mut offsets = Vec::new();
    while !operators.eof() {
        if let Operator::I32Load { memarg } = operators
            .read()
            .map_err(|error| format!("character-action-proof: body operator: {error}"))?
        {
            offsets.push(memarg.offset);
        }
    }
    Ok(offsets)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SemanticOp {
    LocalGet(u32),
    LocalSet(u32),
    LocalTee(u32),
    I32Const(i32),
    I32Add,
    I32Mul,
    I32Shl,
    I32Eqz,
    I32GeU,
    I32LtU,
    I32GtS,
    Call(u32),
    I32Load(u64),
    I32Store(u64),
}

/// Retain only operand-bearing operations needed to prove a relationship.
/// Exact body hashes bind omitted control-flow and arithmetic; this sequence
/// additionally binds each derived offset to its producer or consumer.
fn semantic_ops(body: &[u8]) -> Outcome<Vec<SemanticOp>> {
    let mut reader = FunctionBody::new(BinaryReader::new(body, 0))
        .get_operators_reader()
        .map_err(|error| format!("character-action-proof: semantic body: {error}"))?;
    let mut result = Vec::new();
    while !reader.eof() {
        let operation = match reader
            .read()
            .map_err(|error| format!("character-action-proof: semantic operator: {error}"))?
        {
            Operator::LocalGet { local_index } => Some(SemanticOp::LocalGet(local_index)),
            Operator::LocalSet { local_index } => Some(SemanticOp::LocalSet(local_index)),
            Operator::LocalTee { local_index } => Some(SemanticOp::LocalTee(local_index)),
            Operator::I32Const { value } => Some(SemanticOp::I32Const(value)),
            Operator::I32Add => Some(SemanticOp::I32Add),
            Operator::I32Mul => Some(SemanticOp::I32Mul),
            Operator::I32Shl => Some(SemanticOp::I32Shl),
            Operator::I32Eqz => Some(SemanticOp::I32Eqz),
            Operator::I32GeU => Some(SemanticOp::I32GeU),
            Operator::I32LtU => Some(SemanticOp::I32LtU),
            Operator::I32GtS => Some(SemanticOp::I32GtS),
            Operator::Call { function_index } => Some(SemanticOp::Call(function_index)),
            Operator::I32Load { memarg } => Some(SemanticOp::I32Load(memarg.offset)),
            Operator::I32Store { memarg } => Some(SemanticOp::I32Store(memarg.offset)),
            _ => None,
        };
        if let Some(operation) = operation {
            result.push(operation);
        }
    }
    Ok(result)
}

fn contains_sequence(actual: &[SemanticOp], expected: &[SemanticOp], role: &str) -> Outcome<()> {
    if actual
        .windows(expected.len())
        .any(|candidate| candidate == expected)
    {
        Ok(())
    } else {
        Err(format!(
            "character-action-proof: {role} relationship changed"
        ))
    }
}

fn contains_in_order(actual: &[SemanticOp], expected: &[SemanticOp], role: &str) -> Outcome<()> {
    let mut remaining = expected.iter();
    let mut wanted = remaining.next();
    for operation in actual {
        if wanted == Some(operation) {
            wanted = remaining.next();
        }
    }
    if wanted.is_none() {
        Ok(())
    } else {
        Err(format!(
            "character-action-proof: {role} relationship changed"
        ))
    }
}

fn contains_all(values: &[u64], expected: &[u64], role: &str) -> Outcome<()> {
    if expected.iter().all(|needle| values.contains(needle)) {
        Ok(())
    } else {
        Err(format!("character-action-proof: {role} operands changed"))
    }
}

fn active_offset(expr: wasmparser::ConstExpr<'_>) -> Outcome<usize> {
    let mut operators = expr.get_operators_reader();
    let Operator::I32Const { value } = operators
        .read()
        .map_err(|error| format!("character-action-proof: data offset: {error}"))?
    else {
        return Err("character-action-proof: data offset is not i32.const".into());
    };
    if value < 0
        || !matches!(
            operators
                .read()
                .map_err(|error| format!("character-action-proof: data end: {error}"))?,
            Operator::End
        )
    {
        return Err("character-action-proof: invalid data offset expression".into());
    }
    Ok(value as usize)
}

fn table_function_at(input: &[u8], wanted: u32) -> Outcome<u32> {
    for payload in Parser::new(0).parse_all(input) {
        let Payload::ElementSection(reader) =
            payload.map_err(|error| format!("character-action-proof: element section: {error}"))?
        else {
            continue;
        };
        for element in reader {
            let element =
                element.map_err(|error| format!("character-action-proof: element: {error}"))?;
            let ElementKind::Active {
                table_index: None | Some(0),
                offset_expr,
            } = element.kind
            else {
                continue;
            };
            let start = u32::try_from(active_offset(offset_expr)?)
                .map_err(|_| "character-action-proof: table offset overflow")?;
            let ElementItems::Functions(functions) = element.items else {
                continue;
            };
            let entries: Vec<u32> = functions
                .into_iter()
                .collect::<Result<_, _>>()
                .map_err(|error| format!("character-action-proof: table entry: {error}"))?;
            let end = start
                .checked_add(
                    u32::try_from(entries.len())
                        .map_err(|_| "character-action-proof: table too large")?,
                )
                .ok_or("character-action-proof: table range overflow")?;
            if (start..end).contains(&wanted) {
                return entries
                    .get((wanted - start) as usize)
                    .copied()
                    .ok_or("character-action-proof: table slot missing".into());
            }
        }
    }
    Err("character-action-proof: table slot not initialized".into())
}

fn static_memory(input: &[u8]) -> Outcome<Vec<u8>> {
    let mut memory = Vec::new();
    for payload in Parser::new(0).parse_all(input) {
        let Payload::DataSection(reader) =
            payload.map_err(|error| format!("character-action-proof: data section: {error}"))?
        else {
            continue;
        };
        for data in reader {
            let data = data.map_err(|error| format!("character-action-proof: data: {error}"))?;
            let DataKind::Active {
                memory_index: 0,
                offset_expr,
            } = data.kind
            else {
                continue;
            };
            let start = active_offset(offset_expr)?;
            let end = start
                .checked_add(data.data.len())
                .ok_or("character-action-proof: data range overflow")?;
            if end > MAX_STATIC_MEMORY {
                return Err("character-action-proof: static data exceeds bound".into());
            }
            if memory.len() < end {
                memory.resize(end, 0);
            }
            memory[start..end].copy_from_slice(data.data);
        }
    }
    Ok(memory)
}

fn utf16z(value: &str) -> Vec<u8> {
    value
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .chain([0, 0])
        .collect()
}

fn unique_label(memory: &[u8], value: &str) -> Outcome<usize> {
    let bytes = utf16z(value);
    let addresses: Vec<_> = memory
        .windows(bytes.len())
        .enumerate()
        .filter_map(|(address, candidate)| (candidate == bytes).then_some(address))
        .collect();
    match addresses.as_slice() {
        [address] => Ok(*address),
        _ => Err(format!(
            "character-action-proof: {value} label is not unique static UTF-16"
        )),
    }
}

fn label_hash(table: &[u8], value: &str) -> u32 {
    let mut result = 844_963_502u32;
    let mut rolling = 3_804_322_973u32;
    let mut sum = 561_029_770u32;
    for unit in value.encode_utf16() {
        let normalized = if (97..=122).contains(&unit) {
            unit - 32
        } else {
            unit
        };
        rolling = u32::from(normalized) ^ rolling.wrapping_shl(3);
        let offset = ((rolling & 15) * 4) as usize;
        sum = sum.wrapping_add(u32::from_le_bytes(
            table[offset..offset + 4]
                .try_into()
                .expect("fixed hash table"),
        ));
        result = sum.wrapping_add(rolling) ^ result;
    }
    result
}

/// Bind native lookup/actions to current exact prefill input. Callers enumerate
/// only certified resolver-table IDs below count, reject null pointers, then
/// compare hash before asking native child/parent helpers or dispatching.
pub(super) fn certify(input: &[u8]) -> Outcome<ActionProof> {
    Validator::new()
        .validate_all(input)
        .map_err(|error| format!("character-action-proof: invalid input: {error}"))?;
    if digest(input) != JSPI_PREFILL_SHA256 {
        return Err("character-action-proof: unsupported input".into());
    }
    let sections = split_sections(input)?;
    let imports = imported_functions(section_by_id(&sections, 2)?)?;
    let bodies = parse_code(section_by_id(&sections, 10)?)?;
    exact_body(
        &bodies,
        imports,
        LABEL_HASH_FUNCTION,
        LABEL_HASH_BODY,
        "label hash",
    )?;
    exact_body(
        &bodies,
        imports,
        FRAME_CHILD,
        FRAME_CHILD_BODY,
        "frame child lookup",
    )?;
    exact_body(
        &bodies,
        imports,
        FRAME_PARENT,
        FRAME_PARENT_BODY,
        "frame parent lookup",
    )?;
    exact_body(
        &bodies,
        imports,
        FRAME_RESOLVER,
        FRAME_RESOLVER_BODY,
        "frame resolver",
    )?;
    exact_body(
        &bodies,
        imports,
        FRAME_CONSTRUCTOR,
        FRAME_CONSTRUCTOR_BODY,
        "frame constructor",
    )?;
    exact_body(
        &bodies,
        imports,
        FRAME_INITIALIZER,
        FRAME_INITIALIZER_BODY,
        "frame initializer",
    )?;
    exact_body(
        &bodies,
        imports,
        FRAME_HASH_INITIALIZER,
        FRAME_HASH_INITIALIZER_BODY,
        "frame hash initializer",
    )?;
    exact_body(
        &bodies,
        imports,
        FRAME_CHILD_LOOKUP,
        FRAME_CHILD_LOOKUP_BODY,
        "frame child map lookup",
    )?;
    exact_body(
        &bodies,
        imports,
        FRAME_STATE_CONSUMER,
        FRAME_STATE_CONSUMER_BODY,
        "frame state consumer",
    )?;
    exact_body(
        &bodies,
        imports,
        SELECTOR_CONSTRUCTOR,
        SELECTOR_CONSTRUCTOR_BODY,
        "Selector constructor",
    )?;
    exact_body(
        &bodies,
        imports,
        SELECTOR_CALLBACK_WRAPPER,
        SELECTOR_CALLBACK_WRAPPER_BODY,
        "Selector callback wrapper",
    )?;
    exact_body(
        &bodies,
        imports,
        SELECTOR_CALLBACK_HANDLER,
        SELECTOR_CALLBACK_HANDLER_BODY,
        "Selector callback handler",
    )?;
    exact_body(
        &bodies,
        imports,
        SELECTOR_QUERY_DISPATCH,
        SELECTOR_QUERY_DISPATCH_BODY,
        "Selector query dispatcher",
    )?;
    exact_body(
        &bodies,
        imports,
        FRAME_DISPATCH,
        FRAME_DISPATCH_BODY,
        "frame dispatcher",
    )?;
    contains_all(
        &i32_load_offsets(body(&bodies, imports, FRAME_RESOLVER)?)?,
        &[ID_MANAGER_ARRAY, ID_MANAGER_COUNT],
        "ID manager",
    )?;
    contains_sequence(
        &semantic_ops(body(&bodies, imports, FRAME_RESOLVER)?)?,
        &[
            SemanticOp::LocalGet(0),
            SemanticOp::I32Const(0),
            SemanticOp::I32Load(ID_MANAGER_COUNT),
            SemanticOp::I32GeU,
        ],
        "ID manager count bound",
    )?;
    contains_sequence(
        &semantic_ops(body(&bodies, imports, FRAME_RESOLVER)?)?,
        &[
            SemanticOp::I32Const(0),
            SemanticOp::I32Load(ID_MANAGER_ARRAY),
            SemanticOp::LocalGet(0),
            SemanticOp::I32Const(2),
            SemanticOp::I32Shl,
            SemanticOp::I32Add,
            SemanticOp::I32Load(0),
        ],
        "ID manager pointer entry",
    )?;
    // `f6676` allocates a 456-byte frame, sends its `+168` member through the
    // same dispatcher used by emitted actions, initializes `+396`, and returns
    // the frame ID at `+188`. These are relationships, not loose constants.
    contains_sequence(
        &semantic_ops(body(&bodies, imports, FRAME_CONSTRUCTOR)?)?,
        &[
            SemanticOp::I32Const(FRAME_BYTES as i32),
            SemanticOp::I32Const(0),
            SemanticOp::I32Const(1_099_066),
            SemanticOp::I32Const(306),
            SemanticOp::Call(332),
            SemanticOp::LocalGet(0),
            SemanticOp::LocalGet(1),
            SemanticOp::LocalGet(2),
            SemanticOp::LocalGet(5),
            SemanticOp::Call(FRAME_INITIALIZER),
            SemanticOp::LocalTee(0),
            SemanticOp::I32Const(FRAME_DISPATCH_OFFSET as i32),
            SemanticOp::I32Add,
            SemanticOp::LocalTee(5),
        ],
        "frame allocation and dispatch member",
    )?;
    contains_sequence(
        &semantic_ops(body(&bodies, imports, FRAME_CONSTRUCTOR)?)?,
        &[
            SemanticOp::LocalGet(0),
            SemanticOp::I32Const(FRAME_STATE_OFFSET as i32),
            SemanticOp::I32Add,
            SemanticOp::I32Const(4),
            SemanticOp::I32Const(0),
            SemanticOp::I32Const(0),
            SemanticOp::Call(6410),
            SemanticOp::LocalGet(5),
            SemanticOp::I32Const(2),
            SemanticOp::I32Const(0),
            SemanticOp::I32Const(0),
            SemanticOp::Call(6524),
            SemanticOp::LocalGet(5),
            SemanticOp::I32Const(10),
            SemanticOp::I32Const(0),
            SemanticOp::I32Const(0),
            SemanticOp::Call(FRAME_DISPATCH),
            SemanticOp::LocalGet(0),
            SemanticOp::I32Load(FRAME_ID_OFFSET),
        ],
        "frame state, dispatch, and ID",
    )?;
    // `f6648` passes `frame + 296` to f6548. f6548 hashes its string argument
    // with f365 and stores that result at its argument +12, proving frame +308.
    contains_sequence(
        &semantic_ops(body(&bodies, imports, FRAME_INITIALIZER)?)?,
        &[
            SemanticOp::LocalGet(0),
            SemanticOp::I32Const(FRAME_MAP_OFFSET as i32),
            SemanticOp::I32Add,
            SemanticOp::LocalGet(1),
            SemanticOp::LocalGet(3),
            SemanticOp::LocalGet(4),
            SemanticOp::Call(FRAME_HASH_INITIALIZER),
        ],
        "frame hash initializer call",
    )?;
    contains_sequence(
        &semantic_ops(body(&bodies, imports, FRAME_HASH_INITIALIZER)?)?,
        &[
            SemanticOp::LocalGet(3),
            SemanticOp::I32Const(-1),
            SemanticOp::Call(LABEL_HASH_FUNCTION),
            SemanticOp::LocalSet(4),
        ],
        "frame label hash producer",
    )?;
    contains_sequence(
        &semantic_ops(body(&bodies, imports, FRAME_HASH_INITIALIZER)?)?,
        &[
            SemanticOp::LocalGet(0),
            SemanticOp::LocalGet(4),
            SemanticOp::I32Store(FRAME_MAP_HASH_OFFSET),
        ],
        "frame label hash store",
    )?;
    if FRAME_MAP_OFFSET.checked_add(FRAME_MAP_HASH_OFFSET) != Some(FRAME_HASH_OFFSET) {
        return Err("character-action-proof: invalid frame hash layout".into());
    }
    // f6796 passes `frame +296` to f6560, whose matching node is converted
    // back to its owning frame before f6796 consumes frame +188. This binds
    // emitted child lookup to same frame representation as constructor.
    contains_sequence(
        &semantic_ops(body(&bodies, imports, FRAME_CHILD)?)?,
        &[
            SemanticOp::LocalGet(0),
            SemanticOp::I32Const(FRAME_MAP_OFFSET as i32),
            SemanticOp::I32Add,
            SemanticOp::LocalGet(1),
            SemanticOp::Call(FRAME_CHILD_LOOKUP),
        ],
        "frame child lookup key",
    )?;
    contains_sequence(
        &semantic_ops(body(&bodies, imports, FRAME_CHILD_LOOKUP)?)?,
        &[
            SemanticOp::LocalGet(3),
            SemanticOp::I32Const(-296),
            SemanticOp::I32Add,
            SemanticOp::LocalSet(2),
        ],
        "frame child lookup result",
    )?;
    contains_sequence(
        &semantic_ops(body(&bodies, imports, FRAME_STATE_CONSUMER)?)?,
        &[
            SemanticOp::LocalGet(0),
            SemanticOp::I32Const(392),
            SemanticOp::I32Add,
            SemanticOp::I32Const(64),
            SemanticOp::Call(6408),
            SemanticOp::LocalSet(3),
            SemanticOp::LocalGet(0),
            SemanticOp::I32Load(FRAME_STATE_OFFSET),
            SemanticOp::LocalSet(4),
        ],
        "frame state consumer",
    )?;
    contains_sequence(
        &semantic_ops(body(&bodies, imports, FRAME_DISPATCH)?)?,
        &[
            SemanticOp::LocalGet(0),
            // f6508 receives `frame + 168`, proven above from f6676.
            SemanticOp::I32Load((FRAME_CALLBACK_COUNT_OFFSET - FRAME_CALLBACK_ROWS_OFFSET) as u64),
            SemanticOp::LocalTee(5),
            SemanticOp::I32Eqz,
            SemanticOp::LocalGet(0),
            SemanticOp::I32Load(0),
            SemanticOp::LocalTee(6),
            SemanticOp::LocalGet(5),
            SemanticOp::I32Const(FRAME_CALLBACK_ROW_BYTES as i32),
            SemanticOp::I32Mul,
            SemanticOp::I32Add,
            SemanticOp::LocalSet(7),
        ],
        "frame callback rows",
    )?;
    // f6508 dispatches only first reverse row whose table function is nonzero
    // and signed row+8 is negative. It packs the address `row+4` at callback
    // arg0+8; f11951 later loads that address once for Selector model.
    contains_sequence(
        &semantic_ops(body(&bodies, imports, FRAME_DISPATCH)?)?,
        &[
            SemanticOp::LocalGet(7),
            SemanticOp::I32Const(-12),
            SemanticOp::I32Add,
            SemanticOp::LocalTee(5),
            SemanticOp::I32Load(0),
            SemanticOp::LocalTee(8),
            SemanticOp::I32Eqz,
        ],
        "frame callback reverse function row",
    )?;
    contains_sequence(
        &semantic_ops(body(&bodies, imports, FRAME_DISPATCH)?)?,
        &[
            SemanticOp::LocalGet(7),
            SemanticOp::I32Const(-4),
            SemanticOp::I32Add,
            SemanticOp::I32Load(0),
            SemanticOp::LocalTee(9),
            SemanticOp::I32Const(-1),
            SemanticOp::I32GtS,
        ],
        "frame callback active row flag",
    )?;
    contains_sequence(
        &semantic_ops(body(&bodies, imports, FRAME_DISPATCH)?)?,
        &[
            SemanticOp::LocalGet(4),
            SemanticOp::LocalGet(7),
            SemanticOp::I32Const(-8),
            SemanticOp::I32Add,
            SemanticOp::I32Store(16),
        ],
        "frame callback row context packet",
    )?;
    // f12030 constructs the unique static Selector label with frame ID 22 and
    // callback table slot 2957. Table slot 2957 is f11950, which forwards all
    // three callback arguments to f11951. This binds row layout below to this
    // particular Selector rather than another typed frame callback.
    contains_sequence(
        &semantic_ops(body(&bodies, imports, SELECTOR_CONSTRUCTOR)?)?,
        &[
            SemanticOp::LocalGet(0),
            SemanticOp::I32Load(4),
            SemanticOp::I32Const(0),
            SemanticOp::I32Const(22),
            SemanticOp::I32Const(SELECTOR_CALLBACK_SLOT as i32),
            SemanticOp::I32Const(0),
            SemanticOp::I32Const(1_533_820),
            SemanticOp::Call(FRAME_CONSTRUCTOR),
        ],
        "Selector construction",
    )?;
    if table_function_at(input, SELECTOR_CALLBACK_SLOT)? != SELECTOR_CALLBACK_WRAPPER {
        return Err("character-action-proof: Selector callback table entry changed".into());
    }
    contains_sequence(
        &semantic_ops(body(&bodies, imports, SELECTOR_CALLBACK_WRAPPER)?)?,
        &[
            SemanticOp::LocalGet(0),
            SemanticOp::LocalGet(1),
            SemanticOp::LocalGet(2),
            SemanticOp::Call(SELECTOR_CALLBACK_HANDLER),
        ],
        "Selector callback forwarding",
    )?;
    // f6508 stores the *address* of row+4 in its callback packet. f11951
    // loads that packet member, then loads row+4 once to obtain Selector
    // model. A direct native-side reader must therefore load row+4 exactly
    // once; row+4 is model, not a wrapper that itself points to model.
    contains_in_order(
        &semantic_ops(body(&bodies, imports, SELECTOR_CALLBACK_HANDLER)?)?,
        &[
            SemanticOp::LocalGet(0),
            SemanticOp::I32Load(8),
            SemanticOp::LocalTee(2),
            SemanticOp::LocalGet(2),
            SemanticOp::I32Load(0),
            SemanticOp::LocalTee(2),
            SemanticOp::LocalGet(2),
            SemanticOp::I32Load(SELECTOR_CONTEXT_COUNT_OFFSET as u64),
        ],
        "Selector callback context count",
    )?;
    contains_in_order(
        &semantic_ops(body(&bodies, imports, SELECTOR_CALLBACK_HANDLER)?)?,
        &[
            SemanticOp::LocalGet(2),
            SemanticOp::I32Load(SELECTOR_CONTEXT_ROWS_OFFSET as u64),
            SemanticOp::LocalGet(0),
            SemanticOp::I32Const(2),
            SemanticOp::I32Shl,
            SemanticOp::I32Add,
            SemanticOp::I32Load(0),
            SemanticOp::LocalTee(5),
        ],
        "Selector callback row pointer",
    )?;
    contains_in_order(
        &semantic_ops(body(&bodies, imports, SELECTOR_CALLBACK_HANDLER)?)?,
        &[
            SemanticOp::LocalGet(1),
            SemanticOp::LocalGet(5),
            SemanticOp::I32Const(SELECTOR_ROW_NAME_OFFSET as i32),
            SemanticOp::I32Add,
            SemanticOp::LocalTee(5),
        ],
        "Selector callback row name",
    )?;
    contains_in_order(
        &semantic_ops(body(&bodies, imports, SELECTOR_CALLBACK_HANDLER)?)?,
        &[
            SemanticOp::LocalGet(0),
            SemanticOp::I32Load(8),
            SemanticOp::LocalTee(5),
            SemanticOp::LocalGet(5),
            SemanticOp::I32Load(0),
            SemanticOp::LocalTee(0),
            SemanticOp::LocalGet(0),
            SemanticOp::I32Load(SELECTOR_CONTEXT_FRAME_ID_OFFSET as u64),
            SemanticOp::I32Const(0),
            SemanticOp::Call(FRAME_CHILD),
            SemanticOp::I32Const(SELECTOR_INDEX_MESSAGE as i32),
        ],
        "Selector current-index query",
    )?;
    exact_body(
        &bodies,
        imports,
        LOGOUT_PRODUCER,
        LOGOUT_PRODUCER_BODY,
        "logout producer",
    )?;

    // Current f365 loads its 16-word table from this exact immediate.
    if hash_table_offset(body(&bodies, imports, LABEL_HASH_FUNCTION)?)? != HASH_TABLE_ADDRESS {
        return Err("character-action-proof: label hash table address changed".into());
    }
    let memory = static_memory(input)?;
    let table = memory
        .get(HASH_TABLE_ADDRESS..HASH_TABLE_ADDRESS + 64)
        .ok_or("character-action-proof: hash table outside static data")?;
    if digest(table) != HASH_TABLE_SHA256 {
        return Err("character-action-proof: label hash table changed".into());
    }
    // Current data placement, plus uniqueness, makes a translated or duplicate
    // caption fail before it becomes native action input.
    if unique_label(&memory, "Play")? != 1_533_598
        || unique_label(&memory, "Selector")? != 1_533_820
    {
        return Err("character-action-proof: label placement changed".into());
    }
    let selector_hash = label_hash(table, "Selector");
    let play_hash = label_hash(table, "Play");
    if selector_hash != 0x3161_6b12 || play_hash != 0x0b04_1d2a {
        return Err("character-action-proof: label hash result changed".into());
    }
    Ok(ActionProof {
        selector_hash,
        play_hash,
        frame_child: FRAME_CHILD,
        frame_parent: FRAME_PARENT,
        frame_resolver: FRAME_RESOLVER,
        frame_dispatch: FRAME_DISPATCH,
        logout_producer: LOGOUT_PRODUCER,
        frame_dispatch_offset: FRAME_DISPATCH_OFFSET,
        frame_array: FRAME_ARRAY,
        frame_count: FRAME_COUNT,
        callback_rows_offset: FRAME_CALLBACK_ROWS_OFFSET,
        callback_count_offset: FRAME_CALLBACK_COUNT_OFFSET,
        callback_row_bytes: FRAME_CALLBACK_ROW_BYTES,
        callback_context_offset: FRAME_CALLBACK_CONTEXT_OFFSET,
        selector_context_rows_offset: SELECTOR_CONTEXT_ROWS_OFFSET,
        selector_context_count_offset: SELECTOR_CONTEXT_COUNT_OFFSET,
        selector_row_name_offset: SELECTOR_ROW_NAME_OFFSET,
        selector_index_message: SELECTOR_INDEX_MESSAGE,
        selector_callback_slot: SELECTOR_CALLBACK_SLOT,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_prefill_certifies_bounded_id_manager_contract() {
        let raw = include_bytes!("../../web/Gw.jspi.wasm");
        let glue = include_bytes!("../../web/Gw.jspi.js");
        let prefill =
            super::super::launcher_prefill::rewrite(super::super::Runtime::Jspi, raw, glue)
                .unwrap()
                .unwrap();
        let proof = certify(&prefill).unwrap();
        assert_eq!(proof.selector_hash, 0x3161_6b12);
        assert_eq!(proof.play_hash, 0x0b04_1d2a);
        assert_eq!(proof.frame_child, FRAME_CHILD);
        assert_eq!(proof.callback_rows_offset, FRAME_CALLBACK_ROWS_OFFSET);
        assert_eq!(proof.callback_count_offset, FRAME_CALLBACK_COUNT_OFFSET);
        assert_eq!(proof.callback_row_bytes, FRAME_CALLBACK_ROW_BYTES);
        assert_eq!(proof.selector_callback_slot, SELECTOR_CALLBACK_SLOT);
        assert_eq!(proof.callback_context_offset, FRAME_CALLBACK_CONTEXT_OFFSET);
        assert_eq!(
            proof.selector_context_rows_offset,
            SELECTOR_CONTEXT_ROWS_OFFSET
        );
        assert_eq!(
            proof.selector_context_count_offset,
            SELECTOR_CONTEXT_COUNT_OFFSET
        );
        assert_eq!(proof.selector_row_name_offset, SELECTOR_ROW_NAME_OFFSET);
        assert_eq!(proof.selector_index_message, SELECTOR_INDEX_MESSAGE);
    }

    fn uleb(mut value: u32) -> Vec<u8> {
        let mut encoded = Vec::new();
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            encoded.push(byte | if value == 0 { 0 } else { 0x80 });
            if value == 0 {
                return encoded;
            }
        }
    }

    fn sleb(mut value: i32) -> Vec<u8> {
        let mut encoded = Vec::new();
        loop {
            let byte = (value as u8) & 0x7f;
            value >>= 7;
            let done = (value == 0 && byte & 0x40 == 0) || (value == -1 && byte & 0x40 != 0);
            encoded.push(byte | if done { 0 } else { 0x80 });
            if done {
                return encoded;
            }
        }
    }

    fn section(id: u8, bytes: Vec<u8>, module: &mut Vec<u8>) {
        module.push(id);
        module.extend(uleb(bytes.len() as u32));
        module.extend(bytes);
    }

    fn data_segment(address: i32, bytes: &[u8], section: &mut Vec<u8>) {
        section.push(0);
        section.push(0x41);
        section.extend(sleb(address));
        section.push(0x0b);
        section.extend(uleb(bytes.len() as u32));
        section.extend(bytes);
    }

    fn isolated_hash_module(body: &[u8], table: &[u8]) -> Vec<u8> {
        let mut module = b"\0asm\x01\0\0\0".to_vec();
        section(1, vec![1, 0x60, 2, 0x7f, 0x7f, 1, 0x7f], &mut module);
        section(3, vec![1, 0], &mut module);
        section(5, vec![1, 0, 20], &mut module);
        section(7, vec![1, 1, b'f', 0, 0], &mut module);
        let mut code = vec![1];
        code.extend(uleb(body.len() as u32));
        code.extend(body);
        section(10, code, &mut module);
        let mut data = vec![2];
        data_segment(64, &utf16z("Selector")[..16], &mut data);
        data_segment(HASH_TABLE_ADDRESS as i32, table, &mut data);
        section(11, data, &mut module);
        module
    }

    #[test]
    fn current_f365_executes_with_selector_hash() {
        let raw = include_bytes!("../../web/Gw.jspi.wasm");
        let sections = split_sections(raw).unwrap();
        let imports = imported_functions(section_by_id(&sections, 2).unwrap()).unwrap();
        let bodies = parse_code(section_by_id(&sections, 10).unwrap()).unwrap();
        let memory = static_memory(raw).unwrap();
        let module = isolated_hash_module(
            body(&bodies, imports, LABEL_HASH_FUNCTION).unwrap(),
            &memory[HASH_TABLE_ADDRESS..HASH_TABLE_ADDRESS + 64],
        );
        let path = std::env::temp_dir().join(format!("gwnative-f365-{}.wasm", std::process::id()));
        std::fs::write(&path, module).unwrap();
        let status = std::process::Command::new("node")
            .args(["-e", "const fs=require('fs');WebAssembly.instantiate(fs.readFileSync(process.argv[1])).then(x=>process.exit((x.instance.exports.f(64,8)>>>0)===0x31616b12?0:1))"])
            .arg(&path)
            .status()
            .unwrap();
        let _ = std::fs::remove_file(path);
        assert!(status.success());
    }

    #[test]
    fn rejects_any_non_exact_input() {
        assert!(certify(b"not wasm").is_err());
    }
}
