FROM mcr.microsoft.com/vscode/devcontainers/universal:1-linux

USER codespace

RUN    rustup toolchain add stable beta nightly \
    && rustup component add --toolchain stable  rustfmt clippy         \
    && rustup component add --toolchain beta    rustfmt clippy         \
    && rustup component add --toolchain nightly rustfmt clippy         \
    && rustup target    add wasm32-wasi wasm32-unknown-unknown

RUN cargo install cargo-expand cargo-udeps cargo-watch git-uncommit https ripgrep tokio-console
