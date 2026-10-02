# syntax=docker/dockerfile:1.7
# One image, three commands: server | worker | migrate.

FROM rust:1.99-bookworm AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked --bin commerce-service \
 && cp target/release/commerce-service /commerce-service

# Distroless: no shell, no package manager, runs as non-root.
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /commerce-service /usr/local/bin/commerce-service
USER nonroot:nonroot
EXPOSE 8080 9090
ENTRYPOINT ["/usr/local/bin/commerce-service"]
CMD ["server"]
