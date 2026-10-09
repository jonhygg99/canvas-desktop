"""Comprueba los paquetes y genera hashes independientes por plataforma."""
import hashlib
from pathlib import Path
import re
import sys


def checksums(directory, platform, version):
    if not re.fullmatch(r"[a-z0-9-]+", platform):
        raise ValueError("Plataforma inválida")
    extensions = {"windows-x64": ("-setup.exe",), "windows-arm64": ("-setup.exe",),
                  "macos": (".dmg",), "linux": (".deb", ".AppImage")}
    files = sorted(directory.iterdir())
    packages = [p for p in files if p.is_file() and p.name.endswith(extensions[platform])]
    # macOS publica 2 DMG (Intel x86_64 + Apple Silicon aarch64): exigir los
    # dos evita repetir el v0.7.3, que salió con uno solo en silencio.
    expected = 2 if platform in ("linux", "macos") else 1
    if len(packages) != expected:
        raise ValueError(f"Se esperaban {expected} paquetes de {platform}; hay {len(packages)}")
    lines = []
    for package in packages:
        # cargo-packager usa productName para DMG y el nombre binario en otros SO.
        prefix = r"Canvas[ .]Desktop" if platform == "macos" else "canvas-desktop"
        if not re.fullmatch(rf"{prefix}_{re.escape(version)}_[A-Za-z0-9_.-]+", package.name) or package.stat().st_size == 0:
            raise ValueError(f"Paquete vacío o versión incorrecta: {package.name}")
        if platform.startswith("windows-") and f"_{platform.removeprefix('windows-')}-setup.exe" not in package.name:
            raise ValueError(f"Arquitectura incorrecta: {package.name}")
        # GitHub transforma espacios en puntos. Normalizar antes del manifiesto
        # conserva los bytes atestados y permite descargar/verificar/reintentar.
        if " " in package.name:
            normalized = package.with_name(package.name.replace(" ", "."))
            if normalized.exists():
                raise ValueError(f"Nombre de paquete duplicado: {normalized.name}")
            package = package.rename(normalized)
        with package.open("rb") as content:
            digest = hashlib.file_digest(content, "sha256").hexdigest()
        lines.append(f"{digest}  {package.name}\n")
    manifest = directory / f"SHA256SUMS-{platform}.txt"
    manifest.write_text("".join(lines), encoding="utf-8")
    return [directory / p.name.replace(" ", ".") for p in packages] + [manifest]


if __name__ == "__main__":
    checksums(Path(sys.argv[1]), sys.argv[2], sys.argv[3])
