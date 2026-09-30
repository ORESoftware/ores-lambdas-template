# Provider `run_api` boundary

Tracking: #43.

The AWS/GCP provider host API is added only after the normalized adapter authority lands at an immutable reviewed revision; exact Rust signatures derive from those landed types rather than a draft.

Provider hosts normalize transport input and lifecycle only. Semantic operation selection remains in the generated server's single guarded dispatcher; `run_api` must not introduce a second operation-key switch. HTTP and direct-invoke carriers remain separate trust classes, caller payloads cannot manufacture provider identity, and GCP IAP identity requires verified signed assertion policy.

Middleware executes at the invocation boundary before semantic dispatch. HEAD body suppression, bounded request IDs, provider lifecycle/env identifiers, and API Lambda eligibility rules remain preserved. Async provider connectors terminate at the established NATS JetStream boundary rather than adding a second semantic adapter family.
