# Multi-stage Dockerfile for truenas-master-mcp (linux/amd64)
FROM rust:1.85-alpine AS builder

RUN apk add --no-cache musl-dev perl make pkgconfig openssl-dev

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/

RUN cargo build --release

# Runtime stage
FROM alpine:3.20 AS runtime

RUN apk add --no-cache ca-certificates libc6-compat

# Create non-root user
RUN addgroup -g 1000 app && adduser -u 1000 -G app -s /bin/sh -D app

COPY --from=builder /app/target/release/truenas-master-mcp /usr/local/bin/truenas-master-mcp

RUN chmod +x /usr/local/bin/truenas-master-mcp

USER app

EXPOSE 8080 3000

ENV TRUENAS_SERVER_URL=https://10.10.10.50
ENV TRUENAS_TIMEOUT=30
ENV TRUENAS_VERSION=scale
ENV TRUENAS_VERIFY_SSL=false

ENTRYPOINT ["truenas-master-mcp"]
CMD ["--transport=stdio"]
