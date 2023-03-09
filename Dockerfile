FROM rust:1.67 as build

WORKDIR /usr/src/breakcore-dog

COPY . .

RUN apt-get update && apt-get install -y libopus-dev ffmpeg && rm -rf /var/lib/apt/lists/*
RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y libopus-dev ffmpeg yt-dlp && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/src/breakcore-dog/target/release/breakcore-dog /usr/local/bin/breakcore-dog


WORKDIR /usr/local/bin
CMD ["breakcore-dog"]
