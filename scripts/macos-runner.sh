#!/bin/sh
# Cargo runner on macOS (see .cargo/config.toml).
#
# Keychain ties "Always Allow" to a binary's code signature. Every cargo
# build is ad-hoc signed with a new hash, so macOS asks again after each
# rebuild. Signing the `flek` binary with a stable certificate keeps one
# identity across rebuilds: allow once, never asked again.
#
# Identity: $FLEK_CODESIGN_IDENTITY, default "Flek Dev" (a self-signed code
# signing certificate, see README). Without it the binary runs unsigned.
bin="$1"
id="${FLEK_CODESIGN_IDENTITY:-Flek Dev}"
case "$(basename "$bin")" in
  flek|flek-app)
    # No -v: a self-signed certificate is "not trusted", which codesign and
    # Keychain do not care about; the designated requirement (identifier +
    # certificate leaf) is what stays stable across rebuilds.
    if security find-identity -p codesigning 2>/dev/null | grep -q "\"$id\""; then
      codesign --force --sign "$id" --identifier "dev.flek.cli" "$bin" 2>/dev/null \
        || echo "warning: codesign with \"$id\" failed; Keychain may ask again" >&2
    fi
    ;;
esac
exec "$@"
