# Kobo toolchain image plus a pinned Rust toolchain, for building libkaia.so
# against the same sysroot as the rest of the Kobo build.
FROM koreader/kokobo@sha256:e89d36fabb0650fc02f1c7b2cf9555e78485ba463569b6b13f9d063d6ecbb5b6
ARG RUST_VERSION=1.99.0
ENV RUSTUP_HOME=/home/ko/.rustup CARGO_HOME=/home/ko/.cargo
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain "${RUST_VERSION}" \
    && /home/ko/.cargo/bin/rustup target add armv7-unknown-linux-gnueabihf
ENV PATH=/home/ko/.cargo/bin:$PATH
# libarchive comes from the target KOReader staging volume mounted by
# build-rust-kobo.sh. Do not link a host distribution libarchive.
# Firmware 4.x toolchain by default; build-libkaia.sh switches to kobov5.
ENV CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=arm-kobo-linux-gnueabihf-gcc \
    CC_armv7_unknown_linux_gnueabihf=arm-kobo-linux-gnueabihf-gcc \
    AR_armv7_unknown_linux_gnueabihf=arm-kobo-linux-gnueabihf-ar
