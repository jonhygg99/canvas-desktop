#!/usr/bin/env bash
# Empaqueta el binario release de Canvas Desktop en dist/Canvas Desktop.app
# y lo firma ad-hoc con un Designated Requirement ESTABLE (solo el bundle id).
#
# ¿Por qué importa? macOS identifica a una app ante los permisos (TCC: Full
# Disk Access, carpetas de ~/Library/CloudStorage…) por su firma. Un binario
# sin firmar cambia de identidad con cada recompilación (y nunca recibe el
# diálogo de consentimiento), y una firma ad-hoc normal fija el cdhash en el
# requisito, así que también caduca al reconstruir. Fijando el DR al
# identificador del bundle, TODAS las reconstrucciones son "la misma app":
# los permisos concedidos sobreviven.
#
# Uso:  ./packaging/macos/make_app.sh
# Sale: dist/Canvas Desktop.app   →  open "dist/Canvas Desktop.app"
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

APP_NAME="Canvas Desktop"
BUNDLE_ID="com.canvas-desktop.Canvas-Desktop"
EXE_NAME="CanvasDesktop"
DIST="$ROOT/dist"
APP="$DIST/$APP_NAME.app"

echo "==> cargo build --release -p canvas-app"
cargo build --release -p canvas-app
BIN="$ROOT/target/release/canvas-desktop"
[ -x "$BIN" ] || { echo "error: no se encontró $BIN" >&2; exit 1; }

echo "==> ensamblo $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundleDisplayName</key>
    <string>$APP_NAME</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_ID</string>
    <key>CFBundleExecutable</key>
    <string>$EXE_NAME</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
</dict>
</plist>
PLIST

cp "$BIN" "$APP/Contents/MacOS/$EXE_NAME"
chmod 755 "$APP/Contents/MacOS/$EXE_NAME"

if [ -f "$ROOT/assets/macos/icon.icns" ]; then
    cp "$ROOT/assets/macos/icon.icns" "$APP/Contents/Resources/AppIcon.icns"
fi

echo "==> codesign ad-hoc con DR estable ($BUNDLE_ID)"
DR="designated => identifier \"$BUNDLE_ID\""
codesign --force --sign - --requirements "=$DR" "$APP"

echo "==> verificación"
codesign --verify --strict "$APP"
codesign -dv "$APP" 2>&1 | grep -E 'Identifier=|Signature' || true
codesign -d -r- "$APP" 2>&1 | grep '^designated' || true

echo ""
echo "Listo: $APP"
echo "Lánzalo con:  open \"$APP\""
