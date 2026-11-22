# Builds a portable Linux binary of Veloce: Ubuntu 22.04 (glibc 2.35) as the baseline so it runs on
# any distribution released since 2022, with FFmpeg, dav1d and OpenSSL linked statically.
FROM ubuntu:22.04

ARG FFMPEG_VERSION=7.1.1
ARG DAV1D_VERSION=1.5.1
ARG OPENSSL_VERSION=3.0.16
ARG NV_HEADERS_VERSION=n12.2.72.0

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl git pkg-config nasm meson ninja-build perl \
        clang libclang-dev libasound2-dev zlib1g-dev xz-utils \
    && rm -rf /var/lib/apt/lists/*

ENV PREFIX=/opt/media
ENV PKG_CONFIG_PATH=/opt/media/lib/pkgconfig
WORKDIR /src

RUN curl -fsSL https://www.openssl.org/source/openssl-${OPENSSL_VERSION}.tar.gz | tar xz \
    && cd openssl-${OPENSSL_VERSION} \
    && ./Configure linux-x86_64 no-shared no-tests --prefix=$PREFIX --libdir=lib \
    && make -j"$(nproc)" && make install_sw

RUN curl -fsSL https://code.videolan.org/videolan/dav1d/-/archive/${DAV1D_VERSION}/dav1d-${DAV1D_VERSION}.tar.gz | tar xz \
    && cd dav1d-${DAV1D_VERSION} \
    && meson setup build --prefix=$PREFIX --libdir=lib --buildtype=release \
        --default-library=static -Denable_tools=false -Denable_tests=false \
    && ninja -C build install

RUN git clone --depth 1 --branch ${NV_HEADERS_VERSION} https://github.com/FFmpeg/nv-codec-headers.git \
    && make -C nv-codec-headers PREFIX=$PREFIX install

# Only what a player needs: every decoder, demuxer and parser, network protocols with TLS, and
# NVDEC (loaded at run time, so it needs nothing installed on machines without an NVIDIA GPU).
RUN curl -fsSL https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz | tar xJ \
    && cd ffmpeg-${FFMPEG_VERSION} \
    && ./configure --prefix=$PREFIX --pkg-config-flags=--static \
        --enable-static --disable-shared --enable-pic \
        --disable-programs --disable-doc --disable-autodetect \
        --disable-encoders --disable-muxers --disable-filters --disable-indevs --disable-outdevs \
        --enable-filter=aresample,scale,format,null,anull \
        --enable-zlib --enable-openssl --enable-libdav1d \
        --enable-ffnvcodec --enable-nvdec --enable-cuvid \
        --extra-cflags="-I$PREFIX/include" --extra-ldflags="-L$PREFIX/lib" \
    && make -j"$(nproc)" && make install

RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
ENV PATH=/root/.cargo/bin:$PATH
