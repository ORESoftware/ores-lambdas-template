# provider-neutral Lambda middleware invocation

Driver: `ORESoftware/ores-lambdas-template#41`

This document captures one bounded review contract for the driver issue. It is intentionally independent of the remaining implementation work.

## Invariants

- Every provider/trigger crosses one provider-neutral middleware invocation helper before domain dispatch.
- Middleware initialization is cold-start scoped and fail-closed when required manifests are absent/invalid.
- Queue/schedule/direct invocations cannot bypass auth, rate-limit, context, or telemetry policy.
- Rejected invocations must not call customer/domain handlers.

## Verification

- Verify against the exact PR head with normal repository gates.
- Add negative tests before expanding authority or accepting new input classes.
- Keep cross-runtime/public semantics aligned with their canonical authority.
- Treat skipped/zero-step CI as missing evidence.

## Non-goals

This slice does not add credentials, weaken isolation, or declare full fleet rollout complete.
