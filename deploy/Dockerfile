ARG RUST_IMAGE=rust:1.94.1-bookworm
ARG RUNTIME_IMAGE=debian:bookworm-slim
FROM ${RUST_IMAGE} AS build
ARG RUST_TOOLCHAIN=1.94.1
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
RUN cargo +${RUST_TOOLCHAIN} test --locked -p ai-terminal-server
RUN cargo +${RUST_TOOLCHAIN} build --locked --release -p ai-terminal-server

FROM ${RUNTIME_IMAGE}
ARG VERSION=development
ARG REVISION=unknown
ARG CREATED
LABEL org.opencontainers.image.title="aTerminal Server" \
      org.opencontainers.image.description="aTerminal coordination and encrypted relay server" \
      org.opencontainers.image.source="https://github.com/dqh147258/aTerminal" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.created="${CREATED}"
RUN mkdir /data && chown 10001:10001 /data
COPY --from=build /src/target/release/ai-terminal-server /usr/local/bin/ai-terminal-server
USER 10001:10001
ENV AI_TERMINAL_BIND=0.0.0.0:8787 AI_TERMINAL_DB=/data/terminal.sqlite3
EXPOSE 8787
HEALTHCHECK --interval=15s --timeout=3s --start-period=10s --retries=3 CMD ["/usr/local/bin/ai-terminal-server", "--healthcheck"]
ENTRYPOINT ["/usr/local/bin/ai-terminal-server"]
