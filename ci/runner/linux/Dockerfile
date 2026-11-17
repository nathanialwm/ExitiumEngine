# Self-hosted GitHub Actions runner for Exitium's Linux CI jobs (INTERVIEW.md R1).
# Runs on the owner's PC through docker compose; see ci/runner/README.md.
FROM ubuntu:24.04

ARG RUNNER_VERSION=2.338.2
ARG RUST_VERSION=1.100.0

ENV DEBIAN_FRONTEND=noninteractive
# mesa-vulkan-drivers provides lavapipe (software Vulkan), so wgpu tests run headless.
# The X11/Wayland/ALSA/udev headers are for winit, audio and gamepad crates.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl git jq gh sudo unzip zstd python3 \
        build-essential pkg-config libssl-dev clang lld \
        mesa-vulkan-drivers libvulkan1 \
        libx11-dev libxcursor-dev libxrandr-dev libxi-dev libxkbcommon-dev \
        libwayland-dev libasound2-dev libudev-dev \
    && rm -rf /var/lib/apt/lists/*

# Volume mount points must exist and belong to runner, or Docker creates them as root.
RUN useradd --create-home --shell /bin/bash runner \
    && echo "runner ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/runner \
    && mkdir -p /state && chown runner:runner /state

WORKDIR /home/runner/actions-runner
RUN curl -fsSL "https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz" \
        | tar -xz \
    && ./bin/installdependencies.sh \
    && mkdir -p _work \
    && chown -R runner:runner /home/runner

USER runner
ENV PATH=/home/runner/.cargo/bin:$PATH
# Outside the checkout: actions/checkout runs `git clean -ffdx`, which would wipe
# an in-tree target/ and force a full rebuild on every job.
ENV CARGO_TARGET_DIR=/home/runner/target
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain "${RUST_VERSION}" \
    && rustup component add clippy rustfmt \
    && rustup target add wasm32-unknown-unknown \
    && rustup toolchain install nightly --profile minimal --component miri,rust-src \
    && cargo +nightly miri setup \
    && cargo install --locked cargo-nextest cargo-deny \
    && rm -rf /home/runner/.cargo/registry \
    && mkdir -p /home/runner/.cargo/registry /home/runner/target

COPY --chown=runner:runner entrypoint.sh /home/runner/entrypoint.sh
ENTRYPOINT ["/home/runner/entrypoint.sh"]
