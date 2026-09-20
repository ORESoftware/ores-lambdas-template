# Contracts

This directory is the repository-local contract boundary.

When this repository owns structured interfaces, keep independently authored TypeSpec and JSON Schema Draft 2020-12 authorities here. Neither authority may silently overwrite or generate the other. Reuse shared ORES contracts instead of copying them into a competing local schema.

Lambda descriptor, provider-neutral invocation, configuration, and artifact shapes that become contractual should converge here deliberately while preserving deployed wire compatibility.
