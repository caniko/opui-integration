{
  description = "freetype/fontconfig for OpenPencil raster-native link";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs =
    { nixpkgs, ... }:
    let
      pkgs = nixpkgs.legacyPackages.x86_64-linux;
    in
    {
      devShells.x86_64-linux.raster = pkgs.mkShell {
        packages = [
          pkgs.pkg-config
          pkgs.freetype
          pkgs.fontconfig
        ];
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
          pkgs.freetype
          pkgs.fontconfig
        ];
      };
      devShells.x86_64-linux.graphical = pkgs.mkShell {
        packages = [
          pkgs.cargo-audit
          pkgs.cargo-deny
          pkgs.cargo-public-api
          pkgs.cargo-semver-checks
          pkgs.coreutils
          pkgs.just
          pkgs.jq
          pkgs.libinput
          pkgs.mesa
          pkgs.pkg-config
          pkgs.rustup
          pkgs.vulkan-loader
          pkgs.wayland
          pkgs.weston
        ];
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
          pkgs.libxkbcommon
          pkgs.mesa
          pkgs.vulkan-loader
          pkgs.wayland
        ];
        VK_DRIVER_FILES = "${pkgs.mesa}/share/vulkan/icd.d/lvp_icd.x86_64.json";
        WGPU_BACKEND = "vulkan";
        WINIT_UNIX_BACKEND = "wayland";
      };
    };
}
