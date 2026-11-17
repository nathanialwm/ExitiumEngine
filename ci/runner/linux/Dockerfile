# Self-hosted GitHub Actions runner for Exitium's Linux CI jobs (INTERVIEW.md R1).
# Runs on the owner's PC through docker compose; see ci/runner/README.md.
FROM ubuntu:24.04

ARG RUNNER_VERSION=2.338.2
ARG RUST_VERSION=1.100.0

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl git jq gh sudo unzip zstd python3 \
        build-essential pkg-config libssl-dev clang lld \
        # lavapipe: software Vulkan so wgpu tests run headless without a GPU
        mesa-vulkan-drivers libvulkan1 \
        # winit / audio / gamepad system libraries
        libx11-dev libxcursor-dev libxrandr-dev libxi-dev libxkbcommon-dev \
        libwayland-dev libasound2-dev libudev-dev \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /bin/bash runner \
    && echo "runner ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/runner

WORKDIR /home/runner/actions-runner
RUN curl -fsSL "https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz" \
        | tar -xz \
    && ./bin/installdependencies.sh \
    && chown -R runner:runner /home/runner

USER runner
ENV PATH=/home/runner/.cargo/bin:$PATH
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain "${RUST_VERSION}" \
    && rustup component add clippy rustfmt \
    && rustup target add wasm32-unknown-unknown \
    && rustup toolchain install nightly --profile minimal --component miri,rust-src \
    && cargo +nightly miri setup \
    && cargo install --locked cargo-nextest cargo-deny \
    && rm -rf /home/runner/.cargo/registry     && mkdir -p /home/runner/.cargo/registry /home/runner/actions-runner/_work

COPY --chown=runner:runner entrypoint.sh /home/runner/entrypoint.sh
ENTRYPOINT ["/home/runner/entrypoint.sh"]
