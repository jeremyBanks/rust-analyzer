FROM mcr.microsoft.com/vscode/devcontainers/universal:1-linux

USER codespace

RUN rustup toolchain add stable nightly \
 && rustup component add --toolchain stable  rustfmt clippy \
 && rustup component add --toolchain nightly rustfmt clippy

RUN cargo install cargo-expand cargo-udeps cargo-watch git-uncommit https ripgrep tokio-console
