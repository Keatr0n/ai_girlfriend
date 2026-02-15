{
  description = "Voice AI dev shell";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
  let
    system = "x86_64-linux";
    pkgs = import nixpkgs { inherit system; };
  in {
    devShells.${system}.default = pkgs.mkShell {
      buildInputs = [
        pkgs.rustc
        pkgs.cargo
        pkgs.clang
        pkgs.llvmPackages.libclang
        pkgs.cmake
        pkgs.pkg-config
        pkgs.clippy
        pkgs.rust-analyzer
        pkgs.stdenv.cc
        pkgs.alsa-lib
        pkgs.pipewire
        pkgs.libpulseaudio
        pkgs.python3
        pkgs.rocmPackages.clr
        pkgs.rocmPackages.rocblas
        pkgs.rocmPackages.hipblas
        pkgs.ffmpeg
        pkgs.piper-tts
      ];

      shellHook = ''
        export LLAMA_CUDA=0
        export LLAMA_METAL=0
        export LLAMA_ROCM=1
        export LLAMA_OPENBLAS=1
        export LLAMA_AVX=1
        export LLAMA_AVX2=1
        export LLAMA_FMA=1
        export OPENSSL_DIR=${pkgs.openssl.dev}
        export OPENSSL_LIB_DIR=${pkgs.openssl.out}/lib

        export LD_LIBRARY_PATH=${pkgs.alsa-lib}/lib:${pkgs.pipewire}/lib:${pkgs.rocmPackages.clr}/lib:${pkgs.rocmPackages.rocblas}/lib:${pkgs.rocmPackages.hipblas}/lib:$LD_LIBRARY_PATH
        export PKG_CONFIG_PATH=${pkgs.openssl.dev}/lib/pkgconfig:$PKG_CONFIG_PATH

        # Let bindgen & clang find headers
        export LIBCLANG_PATH=${pkgs.llvmPackages.libclang.lib}/lib

        # C/C++ headers for clang on NixOS
        export CPATH=${pkgs.stdenv.cc.cc.lib}/include
        export C_INCLUDE_PATH=${pkgs.stdenv.cc.cc.lib}/include
        export CPLUS_INCLUDE_PATH=${pkgs.stdenv.cc.cc.lib}/include
        export LIBRARY_PATH=${pkgs.stdenv.cc.cc.lib}/lib
      '';
    };
  };
}
