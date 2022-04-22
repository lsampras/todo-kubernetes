FROM rust:latest
RUN USER=root cargo new --bin  api_server
COPY Cargo.toml Cargo.lock api_server/
WORKDIR /api_server
RUN cargo build --target-dir ../OUTPUT_DIR