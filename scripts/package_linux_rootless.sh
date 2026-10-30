#!/bin/bash
# Builds the Linux packages in an Ubuntu 22.04 userland, with no root (ADR 0121).
#
#   scripts/package_linux_rootless.sh [--format deb|appimage|all]
#
# A program needs the glibc of the machine that built it, and `package_linux.py` refuses more than
# Ubuntu 22.04's. This script makes that machine: it unpacks the Ubuntu 22.04 base image into
# `$SOYEHT_PACKAGE_BASE` (default `~/.cache/soyeht-package-base`, one per architecture of the
# host), installs the build libraries in it with `apt` inside a user namespace (`bwrap`), copies
# the committed sources (`git archive HEAD`, never local changes) and runs `package_linux.py`
# there. The packages land in `dist/`. The first run downloads about 1.5 GB and builds from
# scratch; later runs reuse the toolchain and the build folder.
#
# Needs `bwrap` (bubblewrap), `curl`, `git`, `tar` and `sha256sum` on the host. Build on the
# architecture you want a package for. Set `SOYEHT_PACKAGE_JOBS` to bound the build jobs.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
base="${SOYEHT_PACKAGE_BASE:-$HOME/.cache/soyeht-package-base}"
jobs="${SOYEHT_PACKAGE_JOBS:-8}"
case "$(uname -m)" in
x86_64) arch=amd64 ;;
aarch64 | arm64) arch=arm64 ;;
*) echo "package_linux_rootless: unsupported architecture $(uname -m)" >&2; exit 2 ;;
esac
release_url="https://cdimage.ubuntu.com/ubuntu-base/releases/22.04/release"
tarball="ubuntu-base-22.04.5-base-$arch.tar.gz"
rootfs="$base/$arch/root"
mkdir -p "$base/$arch"

packages="build-essential curl ca-certificates git pkg-config cmake clang libclang-dev python3
 squashfs-tools binutils xz-utils libwayland-dev libx11-dev libxcb1-dev libxkbcommon-dev
 libxkbcommon-x11-dev libxcursor-dev libxrandr-dev libxi-dev libfontconfig-dev libfreetype-dev
 libssl-dev libasound2-dev"

# Runs a command as the user namespace's root inside the Ubuntu userland.
inside() {
    bwrap --unshare-user --uid 0 --gid 0 --bind "$rootfs" / --dev /dev --proc /proc --tmpfs /tmp \
        --ro-bind /etc/resolv.conf /etc/resolv.conf --setenv HOME /root \
        --setenv PATH /root/.cargo/bin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
        --chdir / "$@"
}

if [[ ! -f "$rootfs/etc/os-release" ]]; then
    curl -fsSL -o "$base/$arch/$tarball" "$release_url/$tarball"
    curl -fsSL -o "$base/$arch/SHA256SUMS" "$release_url/SHA256SUMS"
    (cd "$base/$arch" && grep " \*\?$tarball\$" SHA256SUMS | sha256sum -c -)
    mkdir -p "$rootfs"
    tar -xzf "$base/$arch/$tarball" -C "$rootfs"
    mkdir -p "$rootfs/root"
    # apt drops to a user the namespace does not have; one root is all it maps.
    inside /bin/sh -c "echo 'APT::Sandbox::User \"root\";' > /etc/apt/apt.conf.d/99sandbox
        apt-get update -qq
        DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends $packages
        dpkg --configure -a"
fi

# The committed sources, in a folder of their own for every run.
sources="$rootfs/src"
rm -rf "${sources:?}"
mkdir -p "$sources"
git -C "$root" archive HEAD | tar -x -C "$sources"
epoch="$(git -C "$root" log -1 --format=%ct)"
commit="$(git -C "$root" rev-parse --short=9 HEAD)"

cat >"$rootfs/package-in-root.sh" <<'INSIDE'
#!/bin/sh
set -eu
if [ ! -x /root/.cargo/bin/rustup ]; then
    curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none >/dev/null
fi
mkdir -p /build
cd /src
exec python3 scripts/package_linux.py --engine-target /build/engine --desktop-target /build/desktop \
    --out /src/dist "$@"
INSIDE
inside --setenv SOURCE_DATE_EPOCH "$epoch" --setenv SOYEHT_BUILD_COMMIT "$commit" \
    --setenv CARGO_BUILD_JOBS "$jobs" /bin/sh /package-in-root.sh "$@"

mkdir -p "$root/dist"
cp -f "$sources"/dist/soyeht-* "$root/dist/"
ls -l "$root/dist"
