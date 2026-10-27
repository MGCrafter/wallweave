FROM debian:12
ENV DEBIAN_FRONTEND=noninteractive
RUN sed -i 's/Types: deb$/Types: deb deb-src/' /etc/apt/sources.list.d/debian.sources \
    && apt-get update && apt-get install -y --no-install-recommends \
    git ca-certificates curl build-essential cmake ninja-build pkg-config patchelf python3 binutils \
    libxrandr-dev libxinerama-dev libxcursor-dev libxi-dev libgl-dev libegl-dev libglew-dev \
    freeglut3-dev libsdl2-dev liblz4-dev libavcodec-dev libavformat-dev libavutil-dev \
    libswscale-dev libswresample-dev libxxf86vm-dev libglm-dev libglfw3-dev libmpv-dev \
    libpulse-dev libfftw3-dev libfreetype-dev libharfbuzz-dev libdbus-1-dev libnode-dev \
    libwayland-dev wayland-protocols libgmp-dev libnss3 libnspr4 libcups2 libatk1.0-0 \
    libatk-bridge2.0-0 libxcomposite1 libxdamage1 libgbm1 libxkbcommon0 libasound2 \
    libpango-1.0-0 libcairo2 libdrm2 libxshmfence1 libatspi2.0-0
WORKDIR /work
