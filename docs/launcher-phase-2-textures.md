# Phase 2 texture packs

Status: prepared library and bounded WebGL seam implemented. Character-select
replacement and concurrent Account isolation observed; wider world-scene
behavior remains unproven.

## Reuse and format scope

`gwonmac` reference commit `004194ff318320f854240fd227462b19889bef24` is
GPL-3.0-only. GWNative is GPL-3.0-only. Its legacy TPF/DDS approach can be
adapted under same licence; this Rust implementation is an independent port
and this record preserves source attribution.

Supported source container is legacy TexMod TPF: XOR-obfuscated ZIP with
ZipCrypto entries, stored or deflated payloads, exactly one `texmod.def`, and
32-bit `0x…|image-name` mappings. Archive parser bounds source, entry count,
entry expansion and aggregate expansion; rejects unsafe paths, Zip64/split
archives, invalid encryption/compression flags, bad password/check bytes,
checksum failures, duplicate target-to-different-image mappings, missing
images and hash widths above 32 bits.

Supported prepared images: PNG and DDS uncompressed 8/16/32-bit masked RGB/A,
DXT1, DXT3, DXT5 top mip decoded to bounded RGBA. DDS DX10, arbitrary uMod
containers, native Direct3D compressed replacement, and live reload are not
supported or claimed.

## Library contract

`TextureLibrary::new(base)` owns only derived data below `base`; source TPFs
remain user-owned and are never modified. `scan(source)` considers immediate
regular case-insensitive `.tpf` files. Current in-memory device/inode source
identity survives a same-filesystem rename. Content SHA-256 identifies
immutable revisions. Durable registry preserves path identity across atomic
replacement and inode identity across rename; it rehydrates last-known-good
prepared revisions after restart.

Prepared revisions are content-addressed under `revisions/tpf-rgba-dxt-v3/<sha256>.json`,
so identical sources share one conversion result. v1 assets remain usable by
existing pinned sessions; v3 recompiles from managed immutable source bytes when
available. Validated original bytes are
preserved under `sources/<sha256>.tpf`; watched originals remain unchanged.

On a valid replacement, new revision is atomically published. An invalid
replacement retains prior revision for future selections and reports its error.
A missing source bypasses future sessions while existing pinned session files
remain unchanged. `pin_to_file(session_id, selections)` writes an atomic,
session-pinned manifest under library `sessions/`; caller must transport that
file through the child-specific manifest-path environment value. Pixel data and
credentials are never serialized into process arguments or environment values.
Ordered selections use first target mapping wins.

## WebGL seam

`web/texture-packs.js` tracks active unit, 2D bindings and immutable storage.
It replaces complete RGBA `glTexImage2D` and `glTexSubImage2D` level-zero
uploads; tracked immutable storage also receives exact prepared RGBA subregions,
then bounded compatible RGBA mips. It computes legacy
running CRC-32 over direct, R/B-swapped and vertical variants and restores
client heap in `finally`. Partial, unknown or malformed uploads delegate.
Optional raw DXT chains use exact compatible-format block substitution only;
they are not evidence of a live compressed route.

Synthetic tests prove bounded interception behavior. On 2026-09-18, a JSPI
character-select run armed 124 Minimalus mappings and recorded 11 hash matches,
16 replacements, 11 matched textures, 157 `glTexStorage2D` and 1,077
`glTexSubImage2D` calls; compressed and `glTexImage2D` calls were zero. A
native screenshot showed Minimalus flat buttons where baseline was beveled.
This establishes the observed character-select UI path, not orientation/alpha/mips
under all content. Subsequent world, concurrent-account and limited performance
evidence appears below.

## Compatibility matrix

| Input/path | Parser/preparation | WebGL replacement | Evidence |
| --- | --- | --- | --- |
| Legacy 32-bit TexMod TPF + uncompressed DDS | implemented | bounded RGBA top mip | synthetic DDS + seam test |
| Legacy 32-bit TexMod TPF + DXT1/3/5 DDS | decoded RGBA | bounded RGBA top mip when client upload is RGBA | decoder implementation; live upload unverified |
| Supplied `Minimalus.UI.v3.2.tpf` | local parse/preparation test | character-select RGBA subimage replacement observed | 11 matches / 16 replacements in one JSPI character-select run |
| PNG TPF image | bounded decoder implemented | complete RGBA image/subimage plus generated mip | synthetic seam; Minimalus character-select observation |
| 64-bit hash TPF | rejected | bypass | explicit parser rule |
| uMod container | unverified | bypass | no representative container supplied |
| compressed WebGL | exact raw DXT blocks only when manifest supplies compatible chain | unobserved | synthetic seam only; live counters zero |
| tracked RGBA partial sub-image | prepared matching region | bounded replacement | seam test and real WebGL2 pixel readback |
| unknown partial sub-image | original upload | bypass | association is dropped; mixed-content recovery is not claimed |

## Native evidence and limits

The integrated library parsed `Minimalus.UI.v3.2.tpf`; fresh character-select
and world windows used its immutable session manifest. Character-select baseline,
unmatched appearance, simultaneous Account selections and limited
frame-time/host-memory samples passed as recorded below.
World-scene replacement was also observed for Cycone in Ran Musu Gardens:
Minimalus flat party controls, health/energy bars and toolbar appeared while
character and environment textures remained intact. Total WebKit/GPU memory
remains unmeasured.
Synthetic pixel tests cover orientation/alpha/mips without establishing every
live content case. Do not extend character-select evidence to those cases.

## Native comparison sample

Same JSPI client, Main Alt/Mesmichi character-select scene, 1280 × 768 logical
viewport, scale 2, default uncapped rate, foreground window and detailed frame
audit enabled in both runs. Warm mark-to-mark samples on 2026-09-18:

| Selection | Submitted frame intervals | Mean interval | Native host footprint |
| --- | ---: | ---: | ---: |
| Original textures | 5,822 | 7.063 ms | 77.88 MiB |
| Minimalus | 14,652 | 7.596 ms | 85.20 MiB |

Minimalus flat controls remained visible and original character/background
textures remained intact. These sequential samples include ordinary background
development work and are not an isolated performance benchmark. Frame interval
is the logical submission boundary, not measured display presentation. Host
footprint excludes WebKit and GPU allocations; total texture memory overhead
has not been established. Evidence logs: local
`/tmp/gwnative-phase2-baseline-performance.log`,
`/tmp/gwnative-phase2-minimalus-performance.log` and
`/tmp/gwnative-phase2-performance-summary.json`.

Native simultaneous-account check then passed: Main (Spiritard, no pack) and
Main Alt (Mesmichi, Minimalus) were both running through separate Profile
origins. Main retained beveled original controls; Main Alt retained flat
Minimalus controls when inspected after Main's login. This demonstrates
native account isolation for the observed character-select textures.
