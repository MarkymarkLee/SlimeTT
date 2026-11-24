#!/bin/sh
# Build target/release/SlimeTT.app: binary + sherpa-onnx dylibs in Contents/Frameworks + Info.plist.
#   ./bundle.sh            build and sign the app
#   ./bundle.sh --install  also quit the running copy, copy the app to /Applications and launch it
set -e
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release
APP=target/release/SlimeTT.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Frameworks"
cp target/release/stt "$APP/Contents/MacOS/stt"
cp vendor/sherpa-onnx/lib/libsherpa-onnx-c-api.dylib vendor/sherpa-onnx/lib/libonnxruntime.dylib "$APP/Contents/Frameworks/"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>SlimeTT</string>
  <key>CFBundleDisplayName</key><string>SlimeTT</string>
  <key>CFBundleIdentifier</key><string>dev.mark.stt</string>
  <key>CFBundleExecutable</key><string>stt</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>SlimeTT listens to your microphone while you dictate.</string>
</dict></plist>
PLIST
# Sign with a stable self-signed identity so macOS keeps the Microphone / Accessibility grants across rebuilds
# (an ad-hoc signature changes every build, which voids them). The identity lives in its own keychain with a
# known password so signing never prompts; it is created once, here.
KC="$HOME/Library/Keychains/stt-signing.keychain-db"
KC_PW=stt-local   # guards only this local signing key
ID="stt local signing"
if ! security find-certificate -c "$ID" "$KC" >/dev/null 2>&1; then
  echo "creating signing identity \"$ID\" in $KC (one time)"
  tmp=$(mktemp -d)
  /usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj "/CN=$ID" \
    -addext "keyUsage=critical,digitalSignature" -addext "extendedKeyUsage=critical,codeSigning" \
    -keyout "$tmp/key.pem" -out "$tmp/cert.pem" 2>/dev/null
  /usr/bin/openssl pkcs12 -export -inkey "$tmp/key.pem" -in "$tmp/cert.pem" -out "$tmp/id.p12" -passout pass:"$KC_PW"
  [ -f "$KC" ] || security create-keychain -p "$KC_PW" "$KC"
  security set-keychain-settings "$KC"   # no auto-lock
  security unlock-keychain -p "$KC_PW" "$KC"
  security import "$tmp/id.p12" -k "$KC" -P "$KC_PW" -T /usr/bin/codesign >/dev/null
  security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$KC_PW" "$KC" >/dev/null
  rm -rf "$tmp"
fi
security unlock-keychain -p "$KC_PW" "$KC"
codesign --force --deep --keychain "$KC" --sign "$ID" "$APP"
echo "built $APP"

[ "$1" = "--install" ] || exit 0

# quit the running copy, install, launch (permission grants survive: same signing identity)
pkill -x stt || true
pkill -f 'fm serve --host 127.0.0.1 --port 11976' || true  # the app's Apple FM server (SIGTERM skips the app's cleanup)
rm -rf /Applications/SlimeTT.app
cp -R "$APP" /Applications/
open /Applications/SlimeTT.app
echo "installed /Applications/SlimeTT.app"
