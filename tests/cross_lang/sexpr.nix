#
# Pure-Nix reference implementation of iac-forge's canonical sexpr
# emission + BLAKE3 content hashing.
#
# Third language proof (after Rust and Ruby) that the canonical form
# is portable: any implementation that emits according to these rules
# and hashes with BLAKE3 agrees on every frozen vector.
#
# Usage:
#
#   # Emit the canonical form of a value
#   nix-instantiate --eval --json tests/cross_lang/sexpr.nix \
#       --arg value '{ kind = "list"; items = [
#           { kind = "symbol"; value = "list"; }
#           { kind = "symbol"; value = "integer"; }
#       ]; }' \
#       --argstr want "emit"
#   → "(list integer)"
#
#   # Hash raw canonical text (requires Nix ≥ 2.19 with blake3-hashes flag)
#   nix-instantiate --extra-experimental-features blake3-hashes \
#       --eval --json tests/cross_lang/sexpr.nix \
#       --argstr input "integer" --argstr want hash
#   → "bf0b731c90564bc8c1a8b8078964f3fb4e20636f1beb54ff1cfecb06a7ca2ac8"
#
# The SExpr Nix encoding mirrors the Rust enum:
#   symbol  → { kind = "symbol";  value = "name";  }
#   string  → { kind = "string";  value = "hello"; }
#   integer → { kind = "integer"; value = 42;      }
#   float   → { kind = "float";   value = 3.14;    }
#   bool    → { kind = "bool";    value = true;    }
#   nil     → { kind = "nil"; }
#   list    → { kind = "list";    items = [ ... ]; }

{ value ? null
, want ? "emit"     # "emit" | "hash"
, input ? null      # alternative: raw canonical text for hashing
}:

let
  # ── Emission ─────────────────────────────────────────────────

  # Standard-library-free string escape — no nixpkgs dependency.
  escapeString = s:
    let
      chars = builtins.filter (c: true)
        (builtins.genList (i: builtins.substring i 1 s) (builtins.stringLength s));
      escapeChar = c:
        if c == "\"" then "\\\""
        else if c == "\\" then "\\\\"
        else if c == "\n" then "\\n"
        else if c == "\t" then "\\t"
        else c;
    in builtins.concatStringsSep "" (map escapeChar chars);

  hasInfix = needle: haystack:
    let
      nLen = builtins.stringLength needle;
      hLen = builtins.stringLength haystack;
      check = i:
        if i + nLen > hLen then false
        else if builtins.substring i nLen haystack == needle then true
        else check (i + 1);
    in check 0;

  concatMapSep = sep: f: xs:
    builtins.concatStringsSep sep (map f xs);

  emit = v:
    if v.kind == "symbol" then v.value
    else if v.kind == "string" then
      "\"" + (escapeString v.value) + "\""
    else if v.kind == "integer" then toString v.value
    else if v.kind == "float" then
      let s = toString v.value;
      in if (hasInfix "." s) || (hasInfix "e" s) || (hasInfix "E" s)
         then s
         else s + ".0"
    else if v.kind == "bool" then
      if v.value then "true" else "false"
    else if v.kind == "nil" then "nil"
    else if v.kind == "list" then
      "(" + (concatMapSep " " emit v.items) + ")"
    else throw "unknown SExpr kind: ${v.kind}";

  # ── Hash (BLAKE3 via builtins.hashString) ────────────────────
  #
  # Requires Nix ≥ 2.19 with --extra-experimental-features blake3-hashes.
  # The flag can go on the system-wide nix.conf
  # (`experimental-features = blake3-hashes`) or be passed per-invocation.

  hashText = text: builtins.hashString "blake3" text;

in
  if want == "emit" then
    if value == null then throw "emit needs --arg value"
    else emit value
  else if want == "hash" then
    let text = if input != null then input
               else if value != null then emit value
               else throw "hash needs --arg value or --argstr input";
    in hashText text
  else throw "want must be \"emit\" or \"hash\""
