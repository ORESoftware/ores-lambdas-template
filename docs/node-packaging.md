# Node, Bun, and Deno lambda packaging

The canonical runtime is provider-neutral. JavaScript/TypeScript functions must keep their domain logic separate from provider adapters and produce a deployable artifact per function.

## Node.js

Bundle each Node.js entrypoint to one JavaScript file wherever its dependency graph allows it. Prefer a maintained fast bundler such as esbuild; Rollup or webpack are acceptable where an existing repository already standardizes on them.

Example:

```sh
npx esbuild src/handler.ts \
  --bundle \
  --platform=node \
  --target=node22 \
  --format=esm \
  --outfile=dist/handler.mjs
```

Type checking is a separate gate; bundling is not a substitute for `tsc --noEmit`. Native addons, runtime-loaded assets, source maps, licenses, and intentionally externalized dependencies must be declared and tested from the packaged artifact with no access to the source checkout.

## Bun and Deno

Bun and Deno may additionally produce standalone executables when a provider accepts arbitrary binaries or OCI images. Treat those binaries as OS/architecture-specific outputs and build/test them separately for linux/amd64 and linux/arm64. Do not treat a Bun/Deno executable as a drop-in replacement for a managed Node.js runtime without provider-level compatibility tests.

## Provider adapters

AWS Lambda, Google Cloud/Cloud Run, Azure Functions/Container Apps, Vercel, Cloudflare Workers, and generic OCI runtimes have different bootstrap contracts. Reuse the provider-neutral command/response contract and generate or maintain thin adapters; do not force every target through a shell or container model that the provider does not support.
