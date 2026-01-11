{
  lib,
  rustPlatform,
  makeWrapper,
  pkg-config,
  cmake,
  glib,
  openssl,
  lua5_4,
  libmysqlclient,
  zlib,
}:
rustPlatform.buildRustPackage {
  pname = "frippy";
  version = "0.5.1";
  cargoLock = {
    lockFile = ./Cargo.lock;
  };
  src = lib.cleanSource ./.;

  nativeBuildInputs = [pkg-config cmake];
  buildInputs = [
    glib
    openssl
    lua5_4
    libmysqlclient
    # zlib
  ];

  meta = {
    description = "IRC Bot";
    mainProgram = "frippy";
    maintainers = with lib.maintainers; [jokler];
  };
}
