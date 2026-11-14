# ---- Build ----
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY static ./static
RUN cargo build --release --locked

# ---- Laufzeit ----
FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tzdata \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 1000 --home /data ecoflower \
 && mkdir -p /data && chown ecoflower /data
COPY --from=build /src/target/release/ecoflower-home /usr/local/bin/ecoflower-home
USER ecoflower
ENV DATABASE_PATH=/data/ecoflower.db \
    BIND=0.0.0.0:8080 \
    TZ=Europe/Berlin
VOLUME ["/data"]
EXPOSE 8080
CMD ["ecoflower-home"]
