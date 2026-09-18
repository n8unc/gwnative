# Generated-client seam fixtures

These tracked text extracts keep browser-module CI independent of ignored
client downloads while retaining classic and JSPI compatibility coverage.

Source: locally supplied official client files inspected on 18 September 2026.
Their SHA-256 values were:

| Variant | Source SHA-256 |
| --- | --- |
| `Gw.js` | `e6d031a22d9047f30a1e2d1d2e25f6daca552df4a5dc1e3fad83b57bf404463c` |
| `Gw.jspi.js` | `4ee7c8af5aa5f5c2e9a1334642fdc52075464da4d203fc676c7becb31669a2a8` |

`alc-create-context.txt` contains complete extracted `autoResumeAudioContext`
and `_alcCreateContext` functions. Their source was identical in both pinned
variants apart from surrounding whitespace. `Gw.js.txt` and `Gw.jspi.js.txt`
preserve each variant's image-read
callback `2657431` and `__asyncjs__EmscriptenExeFileImageWait` source.
These tests cover pinned seams, not compatibility with future client builds.
Refresh these extracts when a client generation changes a tested seam.
