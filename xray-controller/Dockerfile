FROM rust:1.88-alpine AS build

RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
ENV CARGO_BUILD_JOBS=1
RUN cargo build --release --locked
RUN cp /src/target/release/xray-controller /out

FROM scratch
COPY --from=build /out /xray-controller
USER 0:0
ENTRYPOINT ["/xray-controller"]
