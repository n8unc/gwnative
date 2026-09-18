# Cursor-only compatibility

Native cursor support has a release-reviewed certificate independent of the
downloaded template-transform feed. It selects one exact JSPI pair and one
exact Asyncify pair by full Wasm and generated-JavaScript SHA-256 values. Any
other pair, runtime, or failed structural proof leaves cursor support disabled.
This certificate authorizes `featureMask: 1` only: first 17 layout words are
zero, so target and game-state observation is not enabled.

The reviewed artifact family is `85594456b6c33aa5a6001bbe65d66b32e918a42dfa3b76224081f245996461af`:

| Runtime | Wasm SHA-256 | Glue SHA-256 |
| --- | --- | --- |
| JSPI | `1eb07332632e2fca8aabf5baa14fa1a1e6a2a59ec7134dfb8f6231d924c9fd7b` | `4ee7c8af5aa5f5c2e9a1334642fdc52075464da4d203fc676c7becb31669a2a8` |
| Asyncify | `373c65abcfe6c26161ffd59cb8ea298eb3d1814f4778aeaf24d8978c1fdfa8ef` | `e6d031a22d9047f30a1e2d2d1e25f6daca552df4a5dc1e3fad83b57bf404463c` |

Both current artifacts reproduce the structural proof used before the cursor
certificate is exposed: data
`82ac696353b647245101804aa43ce0f713d58297967c2ca54e7935cff32b45dc`, element
`469ef6f8f6da821169de1ee65a912097e3f82281a4cdbd24d52b3f911cf46f7f`, and the
first ten globals
`2eb48324dc89101a8eefdca8c6f8b7bee31868c237d4b879db2d373238628032`.

Cursor words are `[5910960, 5910964, 5910968, 2730272, 0, 12, 8, 0, 8, 12, 20, 24]`.
Static review traces FrCursor functions 6213, 6215, 6216 and related cursor
functions 6219, 6231, and 6234: active-art, software-model, and show-count
globals are `5910960`, `5910964`, and `5910968`. The render path through
2962, 2912, and 2834 uses color buffer `2730272`, 1024 words, and 128-byte
pitch. Function 750 checks texture handle key `grtx`; the pointer chain keeps
the reviewed offsets `0`, `12`, `8`, `0`, `8`, `12`, `20`, and `24` for hotspot,
art texture, handle key/object, view texture, texture type, width, and height.

Static proof does not establish that live game memory currently yields a valid
cursor. Host preparation is static validation only; it does not prove live
memory, CSS pixels, or gameplay transitions.
The focused host test is:

```sh
GWNATIVE_CURSOR_TEST_ROOT="/path/to/exact/installed/web" cargo test wasm::tests::external_cursor_pair_is_ready_without_template_support -- --exact
```

This test must pass for both runtimes before claiming host preparation. It does
not sign or migrate template-transform support; unknown pairs remain disabled.

On 2026-09-18, fresh-profile JSPI and Asyncify startup with build 38888 logged
the reviewed artifact family, `passive observer active`, `game cursor active`,
and first frame. This validates startup installation and visible cursor
CSS/pixels in both sessions. Gameplay cursor-art changes, hide/show transitions, pointer-lock
drag release, and focus recovery remain pending live checks. Automated cursor
consumer tests cover these presentation transitions with published snapshots.
