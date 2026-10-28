# Development shell: `nix-shell` (then `cargo run -p koi-music`).
# The Rust toolchain comes from rustup via rust-toolchain.toml.
{ pkgs ? import <nixpkgs> { } }:

let
  # Libraries loaded at runtime: winit/glow (window, input, OpenGL) and
  # fontconfig (system font fallback for CJK and other scripts).
  runtimeLibs = with pkgs; [
    fontconfig
    libGL
    libxkbcommon
    wayland
    xorg.libX11
    xorg.libXcursor
    xorg.libXi
    xorg.libXrandr
  ];
in
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [ pkg-config rustup ];
  buildInputs = with pkgs; [ alsa-lib ] ++ runtimeLibs;
  # Runtime tools, normally installed by the user (koi-music never bundles them).
  packages = with pkgs; [ yt-dlp deno ffmpeg ];
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
}
