{
  description = "MonoCode desktop application";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          monocode = pkgs.callPackage ./nix/package.nix { };
        in
        {
          inherit monocode;
          default = monocode;
        }
      );

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.monocode}/bin/monocode";
          meta.description = "Launch MonoCode";
        };
      });

      checks = forAllSystems (system: {
        inherit (self.packages.${system}) monocode;
      });

      formatter = forAllSystems (system: nixpkgs.legacyPackages.${system}.nixfmt);

      overlays.default = final: prev: {
        monocode = final.callPackage ./nix/package.nix { };
      };
    };
}
