# Functional / immutable runtime audit

Use this checklist when refactoring Lambda runtime and provider adapters toward fresh-value transforms without forcing immutability into measured I/O hot paths.

## Prefer fresh values

Apply owned/fresh-value construction to:

- normalized provider envelope -> semantic invocation conversion;
- config/profile merges;
- receipt/result assembly;
- execution/build evidence construction;
- provider capability normalization;
- test-fixture transformations.

Construct the complete result through constructors/builders or iterator composition rather than creating a partial value and mutating fields later. Caller-owned inputs should remain observably unchanged.

## Mutation that remains appropriate

Do not mechanically remove mutation from:

- streaming buffers and codec/frame assembly;
- network read/write buffers;
- bounded reusable allocation pools;
- provider SDK structures that require in-place filling;
- other paths where profiling demonstrates material allocation/copy cost.

When mutable state crosses an abstraction boundary, keep its ownership local and do not expose aliases that let callers mutate runtime authority after admission.

## Review inventory

For each changed path, record:

1. input ownership (owned / borrowed / shared);
2. whether nested data can still alias the caller after conversion;
3. mutation sites and why they are required;
4. whether failure can leave a partially authoritative value visible;
5. whether retries reuse or mutate a prior receipt/config value;
6. tests proving caller-owned inputs are unchanged;
7. any measured hot-path reason for retaining mutation.

## High-priority areas

### Provider envelopes

Normalize AWS/GCP/local/provider input into a new provider-neutral request. Never mutate caller/provider payloads to smuggle trusted identity, policy decisions, or operation selection into the semantic dispatcher.

### Config merge

Build a new effective config from immutable source layers. Secret-bearing values must not be copied into diagnostics/evidence, and one merge must not mutate a reusable base config used by another invocation.

### Receipts/results

Construct terminal receipts once from admitted inputs/outcome. Avoid mutate-after-publish patterns where telemetry, response serialization, and provenance could observe different states.

### Tests

For each refactored transform, retain an original deep value, execute the transform, and assert the original remains semantically identical. Include nested collections so a shallow-copy-only conversion cannot satisfy the test accidentally.

## Admission rule

This is a refactoring policy, not a license to change provider/runtime semantics. Existing contract, middleware, identity, codec, timeout, and receipt behavior must remain equivalent unless a separate reviewed contract change explicitly says otherwise.
