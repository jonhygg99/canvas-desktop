"""Publica Windows una sola vez; un fallo previo conserva un draft recuperable."""
import json
import os
from pathlib import Path
import subprocess
from release_assets import checksums


def main():
    tag = os.environ["RELEASE_TAG"]
    repo = os.environ["GITHUB_REPOSITORY"]
    sha = os.environ["RELEASE_SHA"]
    assets = checksums(Path("dist"), "windows-x64", tag.removeprefix("v"))
    subprocess.run(["gh", "attestation", "verify", str(assets[0]), "--repo", repo,
                    "--source-digest", sha, "--deny-self-hosted-runners",
                    "--signer-workflow", f"{repo}/.github/workflows/build-windows.yml"], check=True)
    existing = subprocess.run(["gh", "release", "view", tag, "--repo", repo,
                               "--json", "isDraft"], capture_output=True, text=True)
    if existing.returncode == 0:
        if not json.loads(existing.stdout)["isDraft"]:
            raise ValueError("Este release ya está publicado; no se sobrescribe. Usa una versión nueva.")
        subprocess.run(["gh", "release", "upload", tag, *map(str, assets),
                        "--repo", repo, "--clobber"], check=True)
    else:
        subprocess.run(["gh", "release", "create", tag, *map(str, assets), "--repo", repo,
                        "--draft", "--verify-tag", "--title", f"Canvas Desktop {tag}",
                        "--notes-file", ".github/release-body.md", "--generate-notes"], check=True)
    # La promoción ocurre solo después de comprobar procedencia y subir ambos assets.
    subprocess.run(["gh", "release", "edit", tag, "--repo", repo,
                    "--draft=false", "--latest"], check=True)


if __name__ == "__main__":
    main()
