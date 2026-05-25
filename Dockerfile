FROM rust:trixie AS builder
WORKDIR /usr/src/langtap
COPY . .
RUN cargo install --path ./server

FROM debian:bookworm-slim
RUN apt-get update -y
RUN apt-get install -y libssl3
RUN rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/langtap /usr/local/bin/langtap
EXPOSE 61948
EXPOSE 54879
ENTRYPOINT ["/usr/local/bin/langtap"]
