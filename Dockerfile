# toro-mcp: mcp.torosaba.net (AI コネクタ)
#
#   docker build -t toro-mcp .
#
# musl で静的にリンクして scratch に載せる。CA 証明書はバイナリに埋め込んである (webpki-roots)

FROM rust:1.100-alpine@sha256:8d0e82abfdf9a38071e07cabc5afba4ddbe077631a80e4e742a760c38beadc33 AS compile
RUN apk add --no-cache gcc musl-dev
WORKDIR /src
# 依存だけを先にビルドして、ソースだけを変えたときのビルドを速くする
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs && touch src/lib.rs \
 && cargo build --release --locked \
 && rm -rf src
COPY src ./src
COPY assets ./assets
RUN touch src/main.rs src/lib.rs && cargo build --release --locked

FROM scratch
COPY --from=compile /src/target/release/toro-mcp /toro-mcp
USER 65532:65532
ENV PORT=8080
EXPOSE 8080
HEALTHCHECK --interval=15s --timeout=4s --start-period=5s --retries=3 CMD ["/toro-mcp", "healthcheck"]
ENTRYPOINT ["/toro-mcp"]
