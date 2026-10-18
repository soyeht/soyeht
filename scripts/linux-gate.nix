# The shell the Linux gate host runs the merge gate in (scripts/remote_gate.sh, docs/platforms.md).
#
#   nix-shell scripts/linux-gate.nix --run 'bash scripts/check_all.sh --only engine:fmt'
#
# It gives a host that has only Nix (NixOS, where a rustup toolchain cannot run) what the engine,
# the desktop client (GPUI) and the end-to-end suites need: the pinned Rust toolchain
# (rust-toolchain.toml), a C toolchain, the GPUI build libraries, git, uv, Python, sccache and
# Node. Nothing is installed outside the Nix store, and nothing needs administrator rights.
# Raise the two pins together, with the hashes `nix-prefetch-url --unpack URL` prints.
let
  nixpkgs = builtins.fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/26f4107e47e3ad0ffd0db220469ba83b6f6d96e8.tar.gz";
    sha256 = "0k498brrpk3ckn1ldzyg3hzd4inrz9pzy4vbcci38kncsc13191r";
  };
  rustOverlay = builtins.fetchTarball {
    url = "https://github.com/oxalica/rust-overlay/archive/ec07af44be2e0ce4e1a17e00a97d44232607c1ec.tar.gz";
    sha256 = "1pg1lm9y1mkvx9ys1hwb3cxlhyfswf2nlp8yfld5yrx4qbpx6arf";
  };
  pkgs = import nixpkgs { overlays = [ (import rustOverlay) ]; };

  # The channel, components and (here) the Windows target for the engine's Windows lint.
  rust = (pkgs.rust-bin.fromRustupToolchainFile ../rust-toolchain.toml).override {
    targets = [ "x86_64-pc-windows-msvc" ];
  };

  # What the GPUI build links against and what it loads when a window opens.
  gpuiLibraries = with pkgs; [
    alsa-lib
    fontconfig
    freetype
    libxkbcommon
    openssl
    vulkan-loader
    wayland
    libxcb
    libx11
    libxcursor
    libxi
    libxrandr
    libxkbcommon
    zlib
    zstd
  ];

  # A Mac's zsh has no first-run wizard; this package's would take over every terminal pane
  # that starts in a directory without startup files, as the engine tests' panes do.
  zsh = pkgs.zsh.overrideAttrs (old: {
    postInstall = (old.postInstall or "") + ''
      rm -f $out/share/zsh/*/functions/zsh-newuser-install $out/share/zsh/*/functions/Newuser/* $out/share/zsh/*/scripts/newuser
    '';
  });

  tools = with pkgs; [
    bash
    clang
    cmake
    coreutils
    curl
    findutils
    gcc
    git
    gnugrep
    gnused
    lsof
    openssh
    perl
    pkg-config
    procps
    python3
    sccache
    util-linux
    uv
  ] ++ [ zsh ];

  # Several tests and the end-to-end scripts name programs by their usual Linux paths
  # (/bin/bash, /usr/bin/git, coreutils in /usr/bin), which NixOS does not have. This runs a
  # command in a user-namespace root that has them, with every tool above installed. Without
  # Nix-specific paths in the way, the gate runs the same commands as on any distribution.
  fhs = pkgs.buildFHSEnv {
    name = "soyeht-gate-fhs";
    targetPkgs = _: [ rust ] ++ tools ++ gpuiLibraries;
    extraOutputsToInstall = [ "dev" ];
    runScript = "bash";
  };
in
pkgs.mkShell {
  packages = [ fhs rust ] ++ tools ++ gpuiLibraries;

  # The end-to-end suites configure the shell prompt through a zsh startup file, and the engine
  # starts the person's $SHELL for a terminal pane, as it does on a Mac.
  shellHook = "export SHELL=${zsh}/bin/zsh";

  # `uv run` uses this interpreter and never downloads one, which could not run on NixOS.
  UV_PYTHON = "${pkgs.python3}/bin/python3";
  UV_PYTHON_DOWNLOADS = "never";
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath gpuiLibraries;
  PKG_CONFIG_PATH = pkgs.lib.makeSearchPathOutput "dev" "lib/pkgconfig" gpuiLibraries;
  # bindgen-based build scripts find libclang here.
  LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
}
