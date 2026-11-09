#!/bin/sh
# Cargo runner on macOS (see .cargo/config.toml).
#
# Keychain remembers "Always Allow" per signing identity, but for code
# without an Apple team id it pins the exact binary (its cdhash): every
# rebuild asks again. Signing with an "Apple Development" certificate (free
# with any Apple ID in Xcode → Settings → Accounts → Manage Certificates)
# puts a team id in the signature, and the permission then survives rebuilds.
#
# Identity: $FLEK_CODESIGN_IDENTITY if set; else the first valid "Apple
# Development" certificate; else "Flek Dev" (self-signed: works, but asks
# after each rebuild). Without any, the binary runs unsigned.
bin="$1"
case "$(basename "$bin")" in
  flek|flek-app)
    id="$FLEK_CODESIGN_IDENTITY"
    if [ -z "$id" ]; then
      id=$(security find-identity -v -p codesigning 2>/dev/null \
        | awk '/"Apple Development: /{print $2; exit}')
    fi
    if [ -z "$id" ] && security find-identity -p codesigning 2>/dev/null | grep -q '"Flek Dev"'; then
      id="Flek Dev"
    fi
    if [ -n "$id" ]; then
      codesign --force --sign "$id" --identifier "dev.flek.cli" "$bin" 2>/dev/null \
        || echo "warning: codesign with \"$id\" failed; Keychain may ask again" >&2
    fi
    ;;
esac
exec "$@"
