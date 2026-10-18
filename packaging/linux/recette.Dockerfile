FROM debian:bookworm
RUN apt-get update && apt-get install -y systemd systemd-sysv curl && rm -rf /var/lib/apt/lists/*
STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
