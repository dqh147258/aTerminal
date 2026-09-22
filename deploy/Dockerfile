ARG RUST_IMAGE=rust:1.94.1-bookworm
ARG RUNTIME_IMAGE=debian:bookworm-slim
FROM ${RUST_IMAGE} AS build
ARG RUST_TOOLCHAIN=1.94.1
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
RUN cargo +${RUST_TOOLCHAIN} build --locked --release -p ai-terminal-server

FROM ${RUNTIME_IMAGE}
RUN mkdir /data && chown 10001:10001 /data
COPY --from=build /src/target/release/ai-terminal-server /usr/local/bin/ai-terminal-server
USER 10001:10001
ENV AI_TERMINAL_BIND=0.0.0.0:8787 AI_TERMINAL_DB=/data/terminal.sqlite3
EXPOSE 8787
ENTRYPOINT ["/usr/local/bin/ai-terminal-server"]
