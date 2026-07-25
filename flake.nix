{
  description = "cull — jq for HTML: select with CSS selectors, shape into JSON, CSV, Markdown, or text";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        packages = rec {
          cull = pkgs.rustPlatform.buildRustPackage {
            pname = "cull";
            version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;

            src = self;
            cargoLock.lockFile = ./Cargo.lock;

            # Networked integration tests are skipped automatically; the rest
            # of the suite runs offline.
            doCheck = true;

            meta = with pkgs.lib; {
              description = "jq for HTML: select with CSS selectors, shape into JSON, CSV, Markdown, or text";
              homepage = "https://github.com/rashida-thorne/cull";
              license = licenses.mit;
              mainProgram = "cull";
            };
          };
          default = cull;
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.cull ];
          packages = with pkgs; [ rustfmt clippy rust-analyzer ];
        };
      });
}
