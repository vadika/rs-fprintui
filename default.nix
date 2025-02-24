{ wrapGAppsHook, fetchFromGitHub, lib, rustPlatform, pkg-config, wireguard-tools, glib, gtk4, libadwaita, polkit }:

rustPlatform.buildRustPackage rec {
  pname = "fprintui";
  version = "0.1.0";

  src = ./.;

  nativeBuildInputs = [
    pkg-config
    wrapGAppsHook
  ];

  buildInputs = [
    wireguard-tools
    glib.dev
    gtk4.dev
    libadwaita.dev
    polkit
  ];

  postFixup = ''
    wrapProgram $out/bin/${pname} \
       --set LIBGL_ALWAYS_SOFTWARE true \
       --set G_MESSAGES_DEBUG all
  '';

  cargoHash = "sha256-46gwxYqJEjKQY42oYLFtKng0PFVqDJDDkiOrHtwa5rg=";
}
