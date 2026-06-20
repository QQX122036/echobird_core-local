# `echobird_core` — clean-room backend for EchoBird

> Clean-room Rust reimplementation of EchoBird's private backend
> crate. The full derivation contract lives in [`src/lib.rs`](src/lib.rs);
> in short: every line of code in this repository was derived from
> the public thin shell's IPC contract and the TypeScript types in
> the public frontend. No upstream source was read.

## Why this exists

The original `edison7009/EchoBird-secret-` private repository that
ships the closed-source backend is not publicly accessible. We
needed a way to:

1. Build and run the public EchoBird frontend (`QQX122036/EchoBird`)
   without depending on a private repo we don't have read access to.
2. Make the per-model `maxContextTokens` /
   `maxInputTokens` / `maxOutputTokens` fields actually take effect
   at the upstream-request layer. The upstream crate drops them at
   serde because its structs predate the fields — see
   `services/agent::send_message` and
   `services::context_window` for the cap-enforcement path.

## What works

| Surface | Status | Notes |
|---|---|---|
| `models.*` (CRUD + ping + test) | ✅ Real | SQLite-backed. Token limits are validated, persisted, and enforced on `test_model`. |
| `agent_send_message` | ✅ Real | Streams OpenAI and Anthropic SSE. Applies `max_input_tokens` (input trim) and `max_output_tokens` (`max_tokens` clamp) per the user-configured caps. Emits a `state` event with the post-trim token estimate so the Mother Agent's context-usage bar can show a real percentage. |
| `apply_model_to_tool` | ✅ Real | Writes Claude Code, Codex, and generic tool configs. Auto-flips `oneMContext` when `maxContextTokens >= 1_000_000` (user override wins). |
| `bundled_assets` | ✅ Real | Reads the install manifest registered by the thin shell. |
| `settings` (get/save) | ✅ Real | SQLite-backed single-row. |
| `scan_tools` | ✅ Real | Walks the registered install manifest and probes the host filesystem. |
| `ssh.*` (load/save/remove) | ✅ Real | JSON file at `~/.echobird/ssh.json`. `test_connection` returns `not_implemented` because the clean-room build doesn't link `ssh2`. |
| `secret.*` (encrypt/decrypt) | ✅ Real | AES-GCM with a passphrase-derived key. Wire format matches the proprietary build (`enc:v1:<hex>`) so the frontend can use either implementation transparently. |
| `app.*` (lifecycle) | ✅ Real | Settings, log tail, open-folder, avatar, project / game launchers. |
| `parasite.*` | ⚠️ Stub | Returns `not_implemented`. The proprietary build runs Claude Code as a child process. |
| `ai_career.*` | ⚠️ Stub | Returns empty data with the shape the frontend expects. |
| `local_server.*` | ⚠️ Stub | Returns `not_implemented` for download / engine install. The proprietary build shells out to `llama-server` / `ollama`. |
| `get_model_directory` | ⚠️ Stub | Returns `None`. The frontend already ships `src/data/modelDirectory.json` as the offline fallback. |
| `get_store_models` | ⚠️ Stub | Returns `[]`. |
| `download_and_install_update` | ⚠️ Stub | Returns `not_implemented`. The proprietary build is Windows-only. |

**Stubs deliberately return a `not_implemented:` error prefix** so
the frontend can show a clear "feature is on the roadmap" toast
instead of silently hanging or rendering a "success" with no result.

## Token-limit integration

The user-configured `maxContextTokens` / `maxInputTokens` /
`maxOutputTokens` flow through three layers:

1. **Persistence** — `storage::sqlite::SqliteStore` stores them as
   nullable INTEGER columns. The `InMemoryStore` used by tests has
   the same shape.
2. **Validation** — `services::models::NewModelDto::validate`
   enforces `> 0`, `<= 100M`, and `maxInput <= maxContext`. The
   cross-field check is the kind of thing the proprietary build
   would have to add to its serde layer; doing it in the service
   layer means a single test covers both the SQLite and the
   in-memory paths.
3. **Enforcement** — `services::agent::send_message`:
   - Calls `trim_to_input_cap` on the message list before sending
     so the input fits under `maxInputTokens` (with a 5% safety
     margin to absorb estimation error).
   - Clamps the per-request `max_tokens` via `clamp_max_tokens` to
     `maxOutputTokens`.
   - Emits a `state` event of shape
     `{ kind: "contextUsage", usedTokens, totalTokens }` so the
     Mother Agent's progress bar can show the post-trim usage
     against the configured cap.

4. **Tool adapter** — `services::model_proxy::apply_model_to_tool`
   auto-flips the Claude `[1m]` variant when the user-configured
   `maxContextTokens` is at or above 1M. The user can pass an
   explicit `oneMContext: false` to opt out.

## Architecture

```
src/
├── lib.rs                  # Thin public surface: run() + register()
├── error/                  # Typed error enum (NotFound/Validation/...)
├── storage/                # Store trait + Sqlite impl + InMemory impl
│   ├── mod.rs              # Public re-exports
│   ├── model.rs            # Model / NewModel / ModelPatch
│   ├── settings.rs         # AppSettings / SettingsPatch
│   ├── sqlite.rs           # Production persistence
│   ├── memory.rs           # Test-only persistence
│   ├── global.rs           # OnceLock<Arc<dyn Store>>
│   └── schema.sql          # CREATE TABLE / CREATE INDEX
├── services/               # Business logic (no Tauri imports)
│   ├── models.rs           # CRUD + validation + ping + test
│   ├── agent.rs            # SSE streaming forward
│   ├── context_window.rs   # Token cap enforcement (pure logic)
│   ├── model_proxy.rs      # apply_model_to_tool + Claude 1M
│   ├── bundled_assets.rs   # Static install-manifest table
│   ├── settings.rs         # App settings pass-through
│   └── tool_installer.rs   # scan_tools
└── commands/               # Tauri IPC shims (one per public domain)
    ├── models.rs
    ├── agent.rs
    ├── tools.rs
    ├── app.rs
    ├── bundled.rs
    ├── ssh.rs
    ├── secret.rs
    ├── parasite.rs
    ├── ai_career.rs
    └── local_server.rs
```

Design rules followed throughout:

- **No `anyhow` in the public API.** Every function returns
  `CoreResult<T>` = `Result<T, Error>` with a typed `Error` enum.
  The IPC layer maps to a string the frontend parses as
  `<code>:<message>`.
- **No `serde_json::Value` in the service or storage layer.** Every
  read returns a typed value or `Error::NotFound`. The IPC layer is
  the only place that touches untyped JSON.
- **No SQL outside `storage::sqlite`.** The `Store` trait uses
  domain types only.
- **No `tokio` in services that don't need it.** The
  `services::models` CRUD is sync; only the I/O-bound operations
  (`agent::send_message`, `models::ping_model`,
  `models::test_model`) are `async`.

## Building

```sh
cd /Users/ayden/echobird_core-local
cargo build        # ~5s on M-series Mac, no network needed
cargo test         # 18 unit tests
```

To wire this into the public EchoBird frontend, set the
`ECHOBIRD_CORE_PATH` env var to this directory and run
`npm run apply-core-path` from the public repo's root. The
public repo's `scripts/apply-core-path.sh` then appends a
`[patch."https://github.com/edison7009/EchoBird-secret-.git"]`
block to `src-tauri/Cargo.toml` redirecting to this directory,
and writes a `src-tauri/.cargo/config.toml` that maps the git
URL to a local source name. The next `cargo tauri build` builds
against this crate directly.

## License

BUSL-1.1, matching the upstream project. See `LICENSE` for the
full text.
