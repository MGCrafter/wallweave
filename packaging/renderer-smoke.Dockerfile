FROM ubuntu:22.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates python3 binutils libgtk-3-0 libnss3 libasound2 libcups2 libpipewire-0.3-0 \
    libgl1 libegl1 libgbm1 libvulkan1 libva2 libva-drm2 libva-x11-2 libva-wayland2 \
    libxkbcommon0 libwayland-egl1 libxxf86vm1 liblz4-1 libgnutls30 libgcrypt20 \
    libgl1-mesa-dri xvfb ffmpeg dbus-x11 pipewire pipewire-pulse wireplumber pulseaudio-utils
WORKDIR /work
