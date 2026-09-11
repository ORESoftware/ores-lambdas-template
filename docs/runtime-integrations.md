# Lambda runtime integration matrix

This template is a provider-neutral function shell. Generated repositories should use the shared ORE concerns directly instead of recreating their configuration, authorization, telemetry, rate-limit, caching, synchronization, legal, chat, forms, or WASM policy locally.

## Always-on repository contracts

- `.cli-flags.toml` is the sole argv/alias/type/default command-line contract and is consumed through the official `flags-2-env` binding.
- `.ores-otel.toml` is the repository-level observability policy. Its cross-runtime contract is owned by `ores-otel/ores-interfaces`; exporter credentials are environment-only and never receive plaintext defaults.
- `schema-authority/main.tsp` and `schema-authority/authored.schema.json` remain independent peer authorities. `ORESoftware/typespec-json-schema-validator` (TJSV) must admit them at an immutable reviewed revision.
- `.zpkg.toml` describes zed-pkg package/build/test metadata and must stay semantically aligned with `Cargo.toml`, bins, features, and the real CI commands.
- `ORESoftware/ores-sops` owns Git-at-rest dotenv conventions. Plaintext is local-only under `env/dec/`; Git may contain only approved `env/enc/*.env.enc` ciphertext paths. Do not decrypt in `docker build`, upload decrypted values as artifacts, or copy them into root TOMLs.

## Capability-owned root TOMLs

Only render a concern file when the generated lambda actually uses that concern. The file selects/configures an admitted implementation; it is not permission to duplicate its parser or business rules.

| Root file | Owner | Lambda use |
| --- | --- | --- |
| `.ores-mw.toml` | `ORESoftware/ores-middleware` | HTTP/RPC middleware ordering, propagation, target/role selection. |
| `.ores-rl.toml` | `ores-rate-limit` | Rate-limit algorithm/backend policy. Secret Redis/HMAC values remain environment-only. |
| `.ores-lru.toml` | `ores-redis-lru-cache` | Local/Redis cache namespace, reconciliation and Pub/Sub policy. Cache credentials remain environment-only. |
| `.auth-shared.toml` | `shared-auth` compatibility surface | Authentication/authorization bindings. Never coexist with another Shared Auth root filename. |
| `.ores-chat.toml` | `ores-chat` | Chat-specific function configuration and environment-key bindings. |
| `.ores-forms.toml` | `ores-forms` | Forms-specific function configuration and environment-key bindings. |
| `.opto-sync.toml` | `opto-sync` | Offline/synchronization policy used by functions that participate in sync flows. |
| `.ores-legal.toml` | `ores-legal` | Legal/document-signing domain policy and environment-key bindings. |
| `.ores-wasm.toml` | `ores-wasm-loaders` | WASM loader/runtime policy where a function intentionally loads admitted modules. |
| `.ores-rpc.toml` | `ORESoftware/api-docs` RPC surface | RPC target, transport/framing and contract references. |
| `.fanwaave-cfg.toml` | `fanwaave` | Fanwaave domain/runtime policy when used. |

A generated lambda must not add a second argv parser inside any concern. Startup order is:

1. audit and parse `.cli-flags.toml` with `flags-2-env`;
2. merge approved CLI overrides into an immutable environment map;
3. parse and independently validate each enabled concern TOML with its owning package/peer-authority contract;
4. resolve symbolic environment-key bindings without printing values;
5. initialize only the capabilities needed by the selected function binary.

Unknown flags, malformed typed values, undeclared environment references, secret-as-argv declarations, plaintext secret defaults, conflicting auth filenames, and cross-authority TJSV discrepancies fail closed.

## Lambda cold-start boundary

Do not maximize reuse by eagerly initializing every shared package. Maximize reuse by delegating each enabled concern to the canonical library while keeping the function's dependency and startup graph narrow. The `health` binary in particular must remain dependency-light and must not initialize auth, ORM, Redis, chat, forms, or legal clients merely because the repository can support them.

Provider adapters remain thin. AWS Lambda, portable HTTP/OCI, Vercel and future providers map provider envelopes into the same provider-neutral invocation/result vocabulary in `src/runtime.rs`.

## Secrets and `env/enc`

The template carries the safe dev/prod ignore boundary but no public age recipients and no fake ciphertext. A generated repository initializes `ORESoftware/ores-sops` with reviewed public recipients, then stores only:

```text
env/enc/dev.env.enc
env/enc/prod.env.enc
```

Stage is opt-in under the exact ores-sops v0.4 rule and must add `env/enc/stage.env.enc` and its Git allowlist together. `env/dec/` and the managed `.env` symlink are never committed.

`ores-cli` repository/environment audits are expected to reconcile root TOML environment declarations with `.cli-flags.toml`. The encrypted-environment audit may decrypt ciphertext only in an explicitly authorized runtime, retain key names rather than values, bound subprocess/output sizes, suppress secret-bearing child stderr, and compare key-name sets without creating plaintext files.

## Promotion gate

A config/TJSV/package change is merge-ready only when the exact current head has stepful green formatter/compiler/tests and applicable contract checks. `mergeable=true`, a clean TOML parse, or a zero-step GitHub Actions failure is not equivalent to tested source.
