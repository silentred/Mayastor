{ sources ? import ../sources.nix }:
let
  pkgs = import sources.nixpkgs { overlays = [ (import sources.nixpkgs-mozilla) ]; };
in
rec {
  nightly = pkgs.rustChannelOf {
    channel = "nightly";
    data = "2020-06-26";
  };

  stable = pkgs.rustChannelOf {
    channel = "stable";
  };
}
