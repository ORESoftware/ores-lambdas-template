# ores-lambdas-template repository instructions

This repository follows the canonical engineering and Git governance rules in `ORESoftware/my-ai/AGENTS.md`.

Repository-specific rules:

- `schema-authority/main.tsp` and `schema-authority/authored.schema.json` are independently human-authored peer authorities. `ORESoftware/typespec-json-schema-validator` (TJSV) must admit them fail-closed before generated contract evidence is trusted.
- `.cli-flags.toml` is the sole argv/alias/type/default command-line authority. Executables use the official `flags-2-env` binding; concern-specific TOML files must not create another argv parser.
- Repository-root ORES TOML files contain non-secret policy, environment-variable names, and bounded defaults only. Secret values belong in the runtime environment or an approved secret store; encrypted Git material follows `ORESoftware/ores-sops` and lives only under `env/enc/`.
- Never commit `env/dec/`, plaintext dotenv, private age identities, tokens, database URLs containing credentials, or decrypted values. Do not decrypt during container builds.
- Lambda cold-start paths initialize only the capabilities used by that function. The presence of a concern config file or optional dependency is not permission to eagerly initialize unrelated auth, ORM, chat, forms, cache, rate-limit, or middleware graphs.
- Preserve provider-neutral runtime behavior and thin provider adapters. AWS, portable HTTP/OCI, Vercel, and future adapters must map into the same validated invocation contract rather than fork business rules.
- Root concern TOMLs must use their owning repository's peer-authority schema and immutable reviewed TJSV gate. Generated schemas, normalized JSON, and receipts are evidence only.
- Git conflicts are resolved semantically. Preserve compatible intent from both sides, salvage still-valid work from stale branches onto current `main`, and never weaken a required exact-head CI gate merely to make a PR mergeable.
