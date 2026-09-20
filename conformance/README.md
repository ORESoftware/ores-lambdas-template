# Conformance

`conformance/check.sh` is the canonical repository-level lifecycle entry point for the generated Lambda template. It fail-closes on symlinked authority roots, requires the committed Cargo lock, checks Docker/OCI build-definition identity, executes the existing entrypoint/concern/lock/ores-cli audit contract suites, and runs the locked Rust adapter tests for `http`, `portable`, and `aws`.

Independent TypeSpec and JSON Schema peer-authority validation remains owned by the existing TJSV workflow and `schema-authority/`; this directory does not replace or regenerate either authored authority.
