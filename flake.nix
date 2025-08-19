{
  description = "iroh-raft - Distributed consensus library using Raft, Iroh P2P, and Redb storage";

  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    nixpkgs.url = "nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ rust-overlay.overlays.default ];
        pkgs = import nixpkgs { inherit system overlays; };

        rust = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        inputs = with pkgs; [
          # Rust toolchain
          rust
          cargo-nextest
          cargo-watch
          cargo-edit

          # Build dependencies
          pkg-config
          openssl.dev
          protobuf

          # Development tools
          rust-analyzer
          ripgrep
          fd

          # Testing
          cargo-tarpaulin
        ];
      in
      {
        devShell = pkgs.mkShell {
          packages = inputs;

          shellHook = ''
            echo "iroh-raft Development Environment"
            echo "================================"
            echo "Rust: $(rustc --version)"
            echo "Cargo: $(cargo --version)"
            echo ""
            echo "Available commands:"
            echo "  cargo build           - Build the library"
            echo "  cargo test            - Run tests"
            echo "  cargo doc --open      - Build and open documentation"
            echo "  cargo bench           - Run benchmarks"
            echo ""
            echo "Development shortcuts:"
            echo "  cb                    - cargo build"
            echo "  ct                    - cargo test"
            echo "  cr                    - cargo build --release"
            echo "  cw                    - cargo watch -x test"
            echo ""
            
            # Aliases for development
            alias cb='cargo build'
            alias ct='cargo test'
            alias cr='cargo build --release'
            alias cw='cargo watch -x test'
            alias cdoc='cargo doc --open'
            
            # Run all tests with different features
            test_all() {
              echo "Running unit tests..."
              cargo test
              
              echo "Running tests with test-helpers feature..."
              cargo test --features test-helpers
              
              echo "Running tests with metrics-otel feature..."
              cargo test --features metrics-otel
              
              echo "Running doc tests..."
              cargo test --doc
            }
          '';

          RUST_LOG = "iroh_raft=debug,raft=info";
          RUST_BACKTRACE = "1";
        };

        defaultPackage = pkgs.rustPlatform.buildRustPackage {
          pname = "iroh-raft";
          version = "0.1.0";

          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          nativeBuildInputs = with pkgs; [
            pkg-config
            protobuf
          ];

          buildInputs = with pkgs; [
            openssl
          ];

          doCheck = true;
        };
      }
    );
}
