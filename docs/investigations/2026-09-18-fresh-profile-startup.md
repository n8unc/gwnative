# Fresh-profile startup failure

Observed: the first launcher account starts; a newer account stalls after filesystem readiness and runtime initialization in both client runtimes. Running it alone does not help.

The working profile contains `/app:/app:/Gw.dat`; the failing profile contains only the template directories. Only IndexedDB paths and record sizes were inspected, not account-file contents.

The host changes the working directory to `/app:`. Its path wrapper makes the generated client mount check for `app:` resolve to `/app:`, so the client skips its former nested-directory creation. However, generated `SYSCALLS.calculateAt` prefixes the working directory to `app:/Gw.dat` before `FS.open` reaches the wrapper. That produces `/app:/app:/Gw.dat`. Fresh profiles lack its parent; older profiles retain it.

Fix: include `app:/app:` in the directories created and persisted before main. Keep existing file paths and data; do not introduce another mount or copy data between accounts.

Validation: regression exercises the generated path calculation against fresh and existing filesystem fixtures. It failed with `missing parent` before the fix and passes afterward. The extracted fixture matches both installed official glues; all 291 browser tests pass. This isolates the missing-parent failure; live game startup after the fix still requires confirmation.
