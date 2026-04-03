{
  description = "Extract: EX(periment) TRACK(er) T(ui)";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  };
  outputs =
    { self, nixpkgs, ... }@inputs:

    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        system = system;
        config.allowUnfree = true;
      };
      shell_name = "base"; # edit this if you want
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        name = shell_name;
        # you can load env vars here or in the .envrc file
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
          pkgs.stdenv.cc.cc
          pkgs.libGL
          pkgs.glib.out
          pkgs.libxcrypt-legacy
          "/run/opengl-driver"
        ];

        venvDir = ".venv";
        packages = with pkgs; [
          uv
          rustc
          cargo
          pkg-config
          sqlite
          rust-analyzer
        ];

        shellHook = "";
      };
    };
}
