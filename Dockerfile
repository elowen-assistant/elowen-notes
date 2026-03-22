FROM rust:1.88-bookworm AS build
WORKDIR /app

COPY Cargo.toml Cargo.toml
COPY Cargo.lock Cargo.lock
COPY src src

RUN cargo build --release

FROM debian:bookworm-slim
WORKDIR /app

COPY --from=build /app/target/release/elowen-notes /usr/local/bin/elowen-notes

EXPOSE 8080

CMD ["elowen-notes"]
