FROM rust:1.67 as build

WORKDIR /usr/src/breakcore-dog

COPY . .

RUN apt-get update && apt-get install -y libopus-dev ffmpeg && rm -rf /var/lib/apt/lists/*
RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y libopus-dev ffmpeg yt-dlp && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/src/breakcore-dog/target/release/breakcore-dog /usr/local/bin/breakcore-dog

ENV DISCORD_TOKEN=NDIyMDgxOTY3MTc0MTg5MDU2.WqQUdg.IT7zm4BJ9JC-3Cws3KeKCE8n5VM
WORKDIR /usr/local/bin
CMD ["breakcore-dog"]
