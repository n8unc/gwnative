# WASM multibox storage viability

Date: 2026-09-18. Scope: GWNative host and ArenaNet WASM game client. Windows
archive extraction excluded following user clarification. Research only; no
runtime changes or logged-in multibox experiment performed.

## Verdict

Desired architecture is viable and largely implemented: shared immutable
game-image chunks, separate mutable profile state, and a virtual contiguous
image presented to each game client. No archive extraction or per-client copy
of the bulk game image is required on this path.

Current code establishes storage architecture, not measured live multibox
performance or exhaustive isolation of every game write.

## Existing path

```text
WASM client A -> image adapter A -> host A --+
                                          +-> shared hash-addressed chunks/
WASM client B -> image adapter B -> host B --+

client A writable files -> profile A WebKit store / IndexedDB
client B writable files -> profile B WebKit store / IndexedDB
```

- [Image adapter](../../web/image.js): `Module.image` provides open, close,
  size, asynchronous read and cache operations for `Gw.snapshot`. Byte ranges
  become loopback HTTP Range requests. There is no image-write operation in
  this adapter. `writeBytes` copies results into WASM memory; it does not modify
  the shared image.
- [Chunk store](../../src/chunks/mod.rs): `read_into` maps byte ranges through
  the selected manifest to content hashes. Cached content is verified before
  use; missing chunks are fetched. Each store holds its own manifest.
- [Chunk files](../../src/chunks/files.rs): hashes name files under a two-level
  directory. New cache content is written to unique temporary files and
  renamed into place. Concurrent readers do not observe a partial publication.
  This is logical image immutability; hosts can populate or repair the cache.
- [Paths](../../src/paths.rs): default cache is shared, while profile support
  directories and default named-profile client roots are separate. Overriding
  `--cache` can intentionally choose a different cache.
- [Persistent filesystem](../../web/filesystem.js): ordinary game writes use
  `app:` backed by IDBFS, with initial population and shutdown flushing. Its
  comment identifies an account record in writable `Gw.dat`; that filename
  must not be confused with the shared bulk `Gw.snapshot`. Actual mutable-file
  sizes and write frequency still need observation.
- [Profile contract](../profiles.md): separate WebKit data-store identities,
  origins, settings and Keychain identities; per-profile instance exclusion.
  [Instance acquisition](../../src/main.rs) bypasses only the global lock when
  an explicitly isolated instance is requested.
- [Cache leases](../../src/cache.rs) defer destructive cleanup while peers are
  active. [Manifest retention](../../src/patch.rs) retains content needed by
  cached profile active and rollback manifests, including different patch roots.

The directory contains image chunks, not named textures, maps or decoded
assets. Chunk boundaries need not match asset boundaries. Asset-level browsing
would be an additional format-parsing feature, unnecessary for multibox sharing.

## Remaining gaps

1. **Cross-process download coordination.** `ChunkStore::open` creates its own
   in-flight map and request semaphores. Two processes missing the same hash
   can both fetch it. Atomic publication gives shared final storage, not a
   machine-wide single download. Per-hash interprocess coordination or a shared
   download service could remove this duplication if measurements justify it.
2. **Per-profile access reports.** The image adapter exposes offset and length;
   [frame audit](../../web/frame-audit.js) records read counts, bytes, timings and
   failures. This is not a durable per-profile chunk-access heatmap. Add bounded
   aggregation by manifest, hash and profile to compare common reads.
3. **Mutable-file accounting.** Instrument the writable filesystem separately:
   path, operation, offset, byte count and size change for writes, truncation,
   rename and deletion. Record metadata rather than file contents. Account for
   filesystem persistence and host-owned profile files as distinct operations.
   A lack of observed writes is not proof a writable file can safely be shared;
   keep mutable state private by default.
4. **Runtime costs.** Shared disk content does not merge each client's WASM
   heap, decoded assets, GPU resources or session state. Measure memory, CPU,
   GPU load and responsiveness with concurrent clients before choosing limits.
5. **Other duplicates.** Client program artifacts, derived modules and profile
   state remain separate. Shared game-image storage does not mean every byte
   used by an extra profile disappears.

## Recommended next experiment

Use two distinct named profiles with the common cache and their default
separate client roots. Existing documented launch pattern:

```sh
gwnative --profile main
gwnative --profile second --new-instance
```

First measure a warm-cache run through matching areas: cache allocation,
network fetches, frame times and memory for each process. Then exercise
different areas, profile setting changes, template saves, quit and restart;
verify state stays with its profile. Use a dedicated test cache for cold-start,
corruption, interrupted download and cleanup experiments. Check that one
profile's patch or cleanup does not invalidate another's running generation.

Success means reused hashes are stored once, warm reads reuse resident data,
mutable state remains separate, and concurrent play remains stable. No need to
build a general copy-on-write archive layer before these checks.

## Validation during this inspection

- `cargo test cache::`: 18 passed.
- `cargo test chunks::`: 24 passed.
- `cargo test profile`: 21 passed; overlaps some cache tests above.
- `node --test web/image-read-tracking.test.js web/frame-audit.test.js`:
  20 passed.
- All 10 source links in this note resolve. Repository-wide `scripts/check-docs`
  fails on five unrelated example links under `.agents/skills/`; it reports no
  failures in this note.

These tests check cache, profile and read-tracking behavior; they do not prove
two logged-in sessions, actual per-client disk growth, or memory savings.
