"""Firma con Developer ID y notariza el DMG de macOS.

Sin secretos de Apple no hace nada (exit 0): el release sale sin firmar,
como hoy. Con secretos, deja el DMG firmado + notarizado con el MISMO nombre,
así `attest`, `upload-artifact` y `checksums` no cambian.

Secretos necesarios (repo settings, nunca en el chat ni en el código):
  APPLE_CERTIFICATE          .p12 del certificado "Developer ID Application" en base64
  APPLE_CERTIFICATE_PASSWORD contraseña del .p12
  APPLE_SIGNING_IDENTITY     ej. "Developer ID Application: Nombre (TEAMID)"
  APPLE_ID / APPLE_PASSWORD  Apple ID + contraseña de app (notarización)
  APPLE_TEAM_ID              Team ID de 10 caracteres

Requiere cuenta de pago de Apple Developer: sin ella no hay certificado y
Gatekeeper seguirá avisando (clic derecho → Abrir, o `xattr -cr`).

Flujo: el .app que deja `cargo packager` se borra tras crear el DMG, así que
se convierte a lectura-escritura, se firma FUERA de la imagen (codesign falla
sobre el HFS+ montado con "internal error", pero copiar sí funciona), se
reconvierte a comprimido, se firma el DMG, se notariza y se grapa el ticket.
Sin entitlements: hardened runtime (`--options runtime`) basta porque la app
no usa JIT, red en escucha ni cámara/micrófono.
"""

import base64
import glob
import os
import shutil
import subprocess
import sys
import tempfile
import uuid
from pathlib import Path


def run(*args, cwd=None):
    print("+", " ".join(str(a) for a in args), flush=True)
    subprocess.run(list(args), cwd=cwd, check=True)


def req(name):
    value = os.environ.get(name, "")
    if not value:
        raise ValueError(f"Falta el secreto {name}")
    return value


def main():
    # `cargo packager --target <triple>` deja el DMG en target/<triple>/release.
    target = sys.argv[1]
    if not os.environ.get("APPLE_CERTIFICATE"):
        print("Sin secretos de Apple: DMG sin firmar (clic derecho → Abrir).")
        return
    identity = req("APPLE_SIGNING_IDENTITY")
    cert_pw = req("APPLE_CERTIFICATE_PASSWORD")
    apple_id = req("APPLE_ID")
    app_pw = req("APPLE_PASSWORD")
    team = req("APPLE_TEAM_ID")

    dmg = glob.glob(f"target/{target}/release/*.dmg")
    if len(dmg) != 1:
        raise ValueError(f"Se esperaba 1 DMG en target/{target}/release; hay {len(dmg)}")
    dmg = Path(dmg[0]).resolve()

    work = Path(tempfile.mkdtemp(prefix="macos-sign-"))
    keychain = f"sign-{uuid.uuid4().hex}.keychain"
    try:
        p12 = work / "cert.p12"
        p12.write_bytes(base64.b64decode(os.environ["APPLE_CERTIFICATE"]))
        keychain_pw = uuid.uuid4().hex
        run("security", "create-keychain", "-p", keychain_pw, keychain)
        run("security", "list-keychains", "-s", keychain, "login.keychain")
        run("security", "unlock-keychain", "-p", keychain_pw, keychain)
        run("security", "import", str(p12), "-k", keychain, "-P", cert_pw,
            "-T", "/usr/bin/codesign")
        run("security", "set-key-partition-list", "-S", "apple-tool:,apple:,codesign:",
            "-s", "-k", keychain_pw, keychain)

        rw = work / "rw.dmg"
        run("hdiutil", "convert", str(dmg), "-format", "UDRW", "-o", str(rw))
        mount = run_output("hdiutil", "attach", str(rw), "-nobrowse", "-mountpoint",
                           str(work / "mnt"))
        try:
            apps = list((work / "mnt").glob("*.app"))
            if len(apps) != 1:
                raise ValueError(f"Se esperaba 1 .app en el DMG; hay {len(apps)}")
            staged = work / "signed.app"
            run("ditto", str(apps[0]), str(staged))
            # Fuera de la imagen: codesign falla SOBRE el HFS+ montado.
            run("codesign", "--deep", "--force", "--options", "runtime",
                "--timestamp", "-s", identity, str(staged))
            run("codesign", "--verify", "--deep", "--strict", str(staged))
            shutil.rmtree(apps[0])
            run("ditto", str(staged), str(apps[0]))
            run("codesign", "--verify", "--deep", "--strict", str(apps[0]))
        finally:
            run("hdiutil", "detach", str(work / "mnt"))

        final = work / "final.dmg"
        run("hdiutil", "convert", str(rw), "-format", "UDZO", "-o", str(final))
        run("codesign", "--force", "-s", identity, str(final))
        run("xcrun", "notarytool", "submit", str(final), "--apple-id", apple_id,
            "--password", app_pw, "--team-id", team, "--wait")
        run("xcrun", "stapler", "staple", str(final))
        run("xcrun", "stapler", "validate", str(final))
        shutil.move(str(final), str(dmg))
        print(f"Firmado y notarizado: {dmg.name}")
    finally:
        run("security", "delete-keychain", keychain)
        shutil.rmtree(work, ignore_errors=True)


def run_output(*args):
    print("+", " ".join(str(a) for a in args), flush=True)
    out = subprocess.run(list(args), check=True, text=True, capture_output=True)
    print(out.stdout, flush=True)
    return out.stdout


if __name__ == "__main__":
    main()
