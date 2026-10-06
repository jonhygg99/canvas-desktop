"""Valida el tag y la CI exacta; encuentra un instalador preparado en main."""
import json
import os
from pathlib import Path
import re
import subprocess
import time
import tomllib


def command(*args):
    return subprocess.check_output(args, text=True).strip()


def api(repo, endpoint):
    return json.loads(command("gh", "api", f"repos/{repo}/{endpoint}"))


def validate_tag(tag, version):
    if not re.fullmatch(r"v\d+\.\d+\.\d+", tag) or tag != f"v{version}":
        raise ValueError(f"El tag {tag!r} debe coincidir con v{version} de Cargo.toml")


def latest_main_run(runs, sha):
    candidates = [r for r in runs if r["head_sha"] == sha
                  and r["head_branch"] == "main" and r["event"] == "push"]
    return max(candidates, key=lambda r: r["id"], default=None)


def wait_for_ci(repo, sha, timeout=1800):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        runs = api(repo, f"actions/workflows/ci.yml/runs?head_sha={sha}&per_page=100")
        run = latest_main_run(runs["workflow_runs"], sha)
        if run and run["status"] == "completed":
            if run["conclusion"] != "success":
                raise ValueError(f"CI no aprobada: {run['html_url']}")
            return run["id"]
        print("Esperando CI de este commit en main...", flush=True)
        time.sleep(20)
    raise ValueError("CI no terminó en 30 minutos. Revisa Actions y reintenta el release.")



def main():
    tag = os.environ["RELEASE_TAG"]
    version = tomllib.loads(Path("Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
    validate_tag(tag, version)
    sha = command("git", "rev-parse", "HEAD")
    command("git", "fetch", "origin", "main")
    subprocess.run(["git", "merge-base", "--is-ancestor", sha, "origin/main"], check=True)
    # Comprueba el tag remoto: workflow_dispatch no puede publicar un ref arbitrario.
    tag_sha = command("git", "rev-parse", f"refs/tags/{tag}^{{commit}}")
    if tag_sha != sha:
        raise ValueError("El checkout no coincide con el commit del tag")
    if os.environ.get("GITHUB_SHA", sha) != sha:
        raise ValueError("Ejecuta el workflow manual sobre el tag, no sobre main")
    repo = os.environ["GITHUB_REPOSITORY"]
    ci_run = wait_for_ci(repo, sha)
    from select_prepared import select
    prepared = select(repo, sha, "windows-x64")
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        output.write(f"sha={sha}\ntag={tag}\nprepared_run={prepared}\nci_run={ci_run}\n")
    print(f"Validado {tag} ({sha}); build preparado: {prepared or 'se compilará'}")


if __name__ == "__main__":
    main()
