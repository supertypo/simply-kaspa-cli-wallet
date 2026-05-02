##
# builder image
##
ARG RUST_VERSION=1.88.0
FROM rust:${RUST_VERSION}-alpine AS builder

ARG REPO_DIR=.

RUN apk --no-cache add \
  musl-dev \
  g++ \
  clang18-dev \
  linux-headers

WORKDIR /work

ENV RUSTFLAGS="-C target-feature=-crt-static -C link-args=-s" \
  CARGO_REGISTRIES_CRATES_IO_PROTOCOL="sparse"

# Cache dependencies
COPY "$REPO_DIR"/Cargo.toml "$REPO_DIR"/Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs

RUN cargo fetch
RUN cargo build --release

COPY "$REPO_DIR" .
RUN find . -type f -name main.rs -exec touch {} +
RUN cargo build --release


##
# runtime image
##
FROM alpine

RUN apk --no-cache add \
  libstdc++

COPY --from=builder /work/target/release/kaspa-wallet /usr/local/bin/

USER 13337:13337

ARG VERSION
ENV VERSION=$VERSION

ENTRYPOINT ["kaspa-wallet"]
CMD ["--help"]
