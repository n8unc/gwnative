# Launcher manages saved credentials

Accounts receive saved-credential changes through the launcher; game instances may consume those credentials for login but game-entered credentials are not read or synced back. The user explicitly rejected game-to-launcher credential synchronisation. Existing game-side save/clear callbacks therefore must not silently mutate launcher-owned credential records or Account identity.
