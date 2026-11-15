#!/bin/sh
# Build only: never launch Flek or use the development Cargo runner.
set +x
set -eu

die() { printf 'error: %s\n' "$*" >&2; exit 1; }

if [ "$#" -gt 1 ] || [ "${1:-}" = "--help" ]; then
    printf 'Usage: %s [aarch64-apple-darwin|x86_64-apple-darwin|universal-apple-darwin]\n' "$0"
    printf 'See docs/build-macos.md for signing, notarization and dependency options.\n'
    [ "$#" -le 1 ] && exit 0
    exit 1
fi

[ "$(uname -s)" = Darwin ] || die 'macOS is required to build .app / DMG bundles'
repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_dir"
for tool in cargo rustc node npm xcrun codesign hdiutil plutil; do
    command -v "$tool" >/dev/null 2>&1 || die "missing tool: $tool"
done
xcrun --find clang >/dev/null
target=${1:-$(rustc -vV | sed -n 's/^host: //p')}
case "$target" in
    aarch64-apple-darwin) dmg_arch=aarch64 ;;
    x86_64-apple-darwin) dmg_arch=x64 ;;
    universal-apple-darwin) dmg_arch=universal ;;
    *) die 'unsupported target; see --help' ;;
esac

# Do not import certificates or create/unlock a Keychain. Developer ID builds
# use a certificate already provisioned by the owner or CI outside this script.
unset APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD
APPLE_SIGNING_IDENTITY=${APPLE_SIGNING_IDENTITY:--}
export APPLE_SIGNING_IDENTITY
case "$APPLE_SIGNING_IDENTITY" in
    -) printf 'Signing: ad-hoc (local testing)\n' ;;
    'Developer ID Application: '*) printf 'Signing: Developer ID Application\n' ;;
    *) die 'APPLE_SIGNING_IDENTITY must be - or a Developer ID Application identity' ;;
esac
case "${FLEK_NOTARIZE:-0}" in
    0)
        # Tauri notarizes automatically when credentials exist. Make it opt-in,
        # even if the caller has Apple credentials in their shell environment.
        unset APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID
        unset APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH
        printf 'Notarization: disabled\n'
        ;;
    1)
        [ "$APPLE_SIGNING_IDENTITY" != - ] || die 'notarization requires Developer ID signing'
        if [ -n "${APPLE_API_KEY:-}" ]; then
            [ -n "${APPLE_API_ISSUER:-}" ] && [ -n "${APPLE_API_KEY_PATH:-}" ] \
                || die 'set APPLE_API_KEY, APPLE_API_ISSUER and APPLE_API_KEY_PATH'
            [ -r "$APPLE_API_KEY_PATH" ] || die 'APPLE_API_KEY_PATH must be readable'
            unset APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID
        else
            [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ] \
                || die 'set APPLE_ID, APPLE_PASSWORD and APPLE_TEAM_ID (or API key variables)'
            unset APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH
        fi
        xcrun --find notarytool >/dev/null
        xcrun --find stapler >/dev/null
        printf 'Notarization: enabled (wait for acceptance and staple)\n'
        ;;
    *) die 'FLEK_NOTARIZE must be 0 or 1' ;;
esac

case "${FLEK_SKIP_NPM_CI:-0}" in
    0) npm ci --no-audit --no-fund ;;
    1) [ -x node_modules/.bin/tauri ] || die 'install locked dependencies with npm ci first' ;;
    *) die 'FLEK_SKIP_NPM_CI must be 0 or 1' ;;
esac

# CI mode skips Finder AppleScript layout, avoiding UI/Automation permission.
# Always pass an explicit target so artifact paths do not depend on Cargo config.
export CI=true
unset TAURI_BUNDLER_DMG_IGNORE_CI
npm run tauri -- build --ci --target "$target" --bundles app,dmg -- --locked

target_dir=$(cargo metadata --locked --no-deps --format-version 1 | node -e '
    let input = "";
    process.stdin.on("data", chunk => input += chunk);
    process.stdin.on("end", () => console.log(JSON.parse(input).target_directory));
')
version=$(node -p 'require("./crates/app/tauri.conf.json").version')
bundle_dir="$target_dir/$target/release/bundle"
app="$bundle_dir/macos/Flek.app"
dmg="$bundle_dir/dmg/Flek_${version}_${dmg_arch}.dmg"
[ -d "$app" ] && [ -f "$dmg" ] || die 'expected .app / DMG artifacts are missing'
plutil -lint "$app/Contents/Info.plist"
codesign --verify --deep --strict "$app"
if [ "${FLEK_NOTARIZE:-0}" = 1 ]; then
    xcrun stapler validate "$app"
fi
printf '\nBuilt artifacts (disk usage):\n'
du -sh "$app" "$dmg"
printf '\nDMG SHA-256:\n'
shasum -a 256 "$dmg"
