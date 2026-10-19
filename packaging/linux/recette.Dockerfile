FROM debian:bookworm
# Index apt conservés : le conteneur démarré installe le paquet sans « apt-get update », dont la
# vérification GPG échoue par intermittence pendant le démarrage de systemd.
RUN apt-get update && apt-get install -y systemd systemd-sysv curl
STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
