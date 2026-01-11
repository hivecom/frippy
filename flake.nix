{
  description = "IRC Bot";
  inputs = {
    nixpkgs.url = "nixpkgs/nixos-25.11";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      fenix,
    }:
    let
      supportedSystems = [ "x86_64-linux" ];
      forEachSystem = nixpkgs.lib.genAttrs supportedSystems;
      overlayList = [ self.overlays.default ];
      pkgsBySystem = forEachSystem (
        system:
        import nixpkgs {
          inherit system;
          overlays = overlayList;
        }
      );
    in
    {
      overlays.default = final: prev: { frippy = final.callPackage ./package.nix { }; };

      packages = forEachSystem (system: {
        frippy =
          let
            inherit (fenix.packages.${system}.minimal) toolchain;
          in
          pkgsBySystem.callPackage ./package.nix {
            rustPlatform = pkgsBySystem.makeRustPlatform {
              cargo = toolchain;
              rustc = toolchain;
            };
          };
        default = self.packages.${system}.frippy;
      });

      devShells = forEachSystem (system: {
        default = pkgsBySystem.${system}.callPackage ./shell.nix { };
      });

      nixosModules = import ./nixos-modules { overlays = overlayList; };
    };
}
