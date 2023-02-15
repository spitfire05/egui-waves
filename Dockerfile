FROM rust:1.67 as builder
RUN cargo install --locked trunk
ADD . ./
RUN trunk build --release



FROM pierrezemb/gostatic
COPY --from=builder dist/* srv/http/
CMD ["-port","8080","-https-promote", "-enable-logging"]