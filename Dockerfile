FROM ghcr.io/famedly/rust-container:nightly as builder
ARG PROJECT_NAME=famedly-sync-agent
ARG CARGO_NET_GIT_FETCH_WITH_CLI=true
ARG FAMEDLY_CRATES_REGISTRY
ARG CARGO_HOME
ARG CARGO_REGISTRIES_FAMEDLY_INDEX
ARG GIT_CRATE_INDEX_USER
ARG GIT_CRATE_INDEX_PASS
ARG RUSTC_WRAPPER
ARG CARGO_BUILD_RUSTFLAGS
ARG CI_SSH_PRIVATE_KEY

# Add CI key for git dependencies in Cargo.toml. This is only done in the builder stage, so the key
# is not available in the final container.
RUN mkdir -p ~/.ssh
RUN echo "${CI_SSH_PRIVATE_KEY}" > ~/.ssh/id_ed25519
RUN chmod 600 ~/.ssh/id_ed25519
RUN echo "Host *\n\tStrictHostKeyChecking no\n\n" > ~/.ssh/config

COPY . /app
WORKDIR /app
RUN cargo auditable build --release

FROM debian:bookworm-slim
RUN apt update && apt install ca-certificates -y
RUN mkdir -p /opt/${PROJECT_NAME}
WORKDIR /opt/${PROJECT_NAME}
COPY --from=builder /app/target/release/${PROJECT_NAME} /usr/local/bin/${PROJECT_NAME}
ENTRYPOINT ["/usr/local/bin/${PROJECT_NAME}"]
