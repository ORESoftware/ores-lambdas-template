# syntax=docker/dockerfile:1
# Multi-stage; final image is the HTTP adapter used by Cloud Run, Azure (container plans), Scintilla and k8s.
FROM --platform=$BUILDPLATFORM rust:1.88-bookworm AS builder
ARG TARGETARCH
WORKDIR /src
COPY . .
RUN cargo build --release --features http,portable --bins
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime
COPY --from=builder /src/target/release/worker-http /usr/local/bin/worker-http
COPY --from=builder /src/target/release/worker-portable /usr/local/bin/worker-portable
COPY .cli-flags.toml /.cli-flags.toml
WORKDIR /
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/worker-http"]
