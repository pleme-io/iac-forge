{
  description = "iac-forge — platform-independent IaC generator library";

  # substrate.rust.library dispatches over the committed Cargo.gen.lock (the
  # slim gen delta, reconstructed to a full BuildSpec in PURE NIX) rather than
  # through crate2nix.
  #
  # NOT `import "${substrate}/lib/rust-library.nix"`: that entry point routes
  # through crate2nix, which reads a derivation back during evaluation
  # (import-from-derivation) in two independent places -- its Cargo.nix
  # generate step, and `mkGitHash` (tools.nix:293) hashing a git dependency by
  # building a runCommand. Both are a DIFFERENT mechanism from the one
  # substrate's gen-pin bump removed, which lived in gen's git-source handling,
  # so they survived that fix untouched. The delta path reads what it needs out
  # of an artifact this repo already tracks, so there is no new generated file
  # to keep fresh.
  inputs.substrate.url = "github:pleme-io/substrate";

  outputs =
    { substrate, ... }:
    substrate.rust.library {
      src = ./.;
    };
}
