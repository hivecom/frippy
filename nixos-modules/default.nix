{ overlays }:
{
  frippy = import ./frippy-service.nix;

  overlayNixpkgsForThisInstance =
    { pkgs, ... }:
    {
      nixpkgs = {
        inherit overlays;
      };
    };
}
