# syntax=docker/dockerfile:1.7
# One multi-stage definition produces Docker-compatible images or OCI layouts.
# BUILDPLATFORM/TARGETPLATFORM are BuildKit automatic global args; do not redeclare them empty.
FROM --platform=$BUILDPLATFORM rust:1.88-bookworm AS builder
ARG TARGETARCH
ARG BINARY=worker-http
ARG CARGO_FEATURES=http
WORKDIR /src
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates gcc-aarch64-linux-gnu gcc-x86-64-linux-gnu \
    && rm -rf /var/lib/apt/lists/*
COPY . .
RUN set -eux; \
    case "$TARGETARCH" in \
      amd64) target=x86_64-unknown-linux-gnu; linker=x86_64-linux-gnu-gcc ;; \
      arm64) target=aarch64-unknown-linux-gnu; linker=aarch64-linux-gnu-gcc ;; \
      *) echo "unsupported target architecture: $TARGETARCH" >&2; exit 2 ;; \
    esac; \
    rustup target add "$target"; \
    linker_var="CARGO_TARGET_$(printf '%s' "$target" | tr '[:lower:]-' '[:upper:]_')_LINKER"; \
    if [ -n "$CARGO_FEATURES" ]; then \
      env "$linker_var=$linker" cargo build --release --target "$target" --bin "$BINARY" --features "$CARGO_FEATURES"; \
    else \
      env "$linker_var=$linker" cargo build --release --target "$target" --bin "$BINARY"; \
    fi; \
    install -Dm755 "target/$target/release/$BINARY" /out/lambda

FROM --platform=$TARGETPLATFORM debian:bookworm-slim AS runtime
ARG SOURCE_REPOSITORY=https://github.com/ORESoftware/ores-lambdas-template
ARG VCS_REF=unknown
LABEL org.opencontainers.image.source="$SOURCE_REPOSITORY" \
      org.opencontainers.image.revision="$VCS_REF" \
      org.opencontainers.image.title="ores-lambdas" \
      org.opencontainers.image.description="Provider-neutral lambda OCI runtime"
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tini \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system lambda \
    && useradd --system --gid lambda --home-dir /nonexistent --no-create-home lambda
COPY --from=builder /out/lambda /usr/local/bin/lambda
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod 0555 /usr/local/bin/lambda /usr/local/bin/entrypoint.sh
USER lambda
ENV PORT=8080 \
    LAMBDA_SIDECAR_MODE=combined \
    LAMBDA_SIDECAR_FAIL_MODE=open
EXPOSE 8080
ENTRYPOINT ["/usr/bin/tini", "-g", "--", "/usr/local/bin/entrypoint.sh"]
CMD ["/usr/local/bin/lambda"]
