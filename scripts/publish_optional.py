"""Añade paquetes opcionales; los reintentos no cambian assets publicados."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
from release_assets import checksums


def digest(path):
    with path.open("rb") as content:
        return hashlib.file_digest(content, "sha256").hexdigest()


def upload_missing(tag, repo, assets):
    existing = json.loads(subprocess.check_output(
        ["gh", "release", "view", tag, "--repo", repo, "--json", "assets"], text=True))
    names = {a["name"] for a in existing["assets"]}
    for asset in assets:
        if asset.name in names:
            with tempfile.TemporaryDirectory() as temp:
                subprocess.run(["gh", "release", "download", tag, "--repo", repo,
                                "--pattern", asset.name, "--dir", temp], check=True)
                if digest(Path(temp) / asset.name) != digest(asset):
                    raise ValueError(f"No se sobrescribe el asset publicado {asset.name}")
        else:
            subprocess.run(["gh", "release", "upload", tag, str(asset), "--repo", repo], check=True)


def main():
    tag = os.environ["RELEASE_TAG"]
    repo = os.environ["GITHUB_REPOSITORY"]
    sha = os.environ["RELEASE_SHA"]
    platform = os.environ["PLATFORM"]
    signer = os.environ.get("SIGNER_WORKFLOW", "release.yml")
    if signer not in ("release.yml", f"build-{platform}.yml"):
        raise ValueError("Workflow firmante no permitido")
    assets = checksums(Path("dist"), platform, tag.removeprefix("v"))
    for asset in assets[:-1]:
        subprocess.run(["gh", "attestation", "verify", str(asset), "--repo", repo,
                        "--source-digest", sha, "--deny-self-hosted-runners",
                        "--signer-workflow", f"{repo}/.github/workflows/{signer}"], check=True)
    if os.environ.get("DRY_RUN") == "true":
        print("Ensayo: paquetes, hashes y procedencia verificados; no se publica.")
        return
    upload_missing(tag, repo, assets)


if __name__ == "__main__":
    main()
