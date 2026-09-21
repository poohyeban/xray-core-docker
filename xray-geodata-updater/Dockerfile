FROM alpine:3.22

RUN apk add --no-cache \
      bash \
      ca-certificates \
      coreutils \
      curl \
      findutils \
      logrotate \
      tzdata \
      util-linux

COPY entrypoint.sh /entrypoint.sh
COPY update-geodata.sh /usr/local/bin/update-geodata.sh
COPY bootstrap-geodata.sh /usr/local/bin/bootstrap-geodata.sh
COPY healthcheck.sh /usr/local/bin/healthcheck.sh
COPY root.cron /etc/crontabs/root
COPY logrotate.conf /etc/logrotate.d/xray-stack

RUN chmod 0755 /entrypoint.sh /usr/local/bin/update-geodata.sh /usr/local/bin/healthcheck.sh /usr/local/bin/bootstrap-geodata.sh \
    && chmod 0600 /etc/crontabs/root \
    && mkdir -p /geodata /var/log/xray /var/log/xray-geodata-updater /run/xray-control

ENTRYPOINT ["/entrypoint.sh"]
