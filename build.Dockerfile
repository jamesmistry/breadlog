# An Alpine build environment for Breadlog. When run, builds exclusively
# statically-linked binaries using musl libc for compatibility with a 
# wide range of Linux distributions and distribution versions.

ARG ALPINE_TAG=latest

FROM alpine:${ALPINE_TAG}

# Install the Rust toolchain and its prerequisites.
RUN apk add --no-cache curl gcc musl-dev
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs > /tmp/install_rust.sh
RUN sh /tmp/install_rust.sh -y

# Update the PATH environment variable so that the Rust tools are available.
ENV PATH="${PATH}:root/.cargo/bin"

# Confirm the toolchain installation was successful.
RUN rustc --version && rustfmt --version && rustdoc --version && cargo --version

# Build release and debug builds.
# Expect the repository in /repo, and the Cargo.toml file in the repo root.

CMD cargo build --manifest-path=/repo/Cargo.toml --release && \
    LLVM_PROFILE_FILE="profiler_output.profraw" RUSTFLAGS="-C instrument-coverage" cargo build --manifest-path=/repo/Cargo.toml
