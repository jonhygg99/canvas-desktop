"""Espera el artefacto de una plataforma sin esperar a las demás."""
import os
from pathlib import Path
import sys
import time
from release_gate import api, command


def select(repo, sha, platform, timeout=1800):
    deadline = time.monotonic() + timeout
    while True:
        runs = api(repo, f"actions/workflows/prepare-release.yml/runs?head_sha={sha}&per_page=100")
        pending = False
        for run in sorted(runs["workflow_runs"], key=lambda r: r["id"], reverse=True):
            if (run["head_sha"] != sha or run["head_branch"] != "main"
                    or run["event"] not in ("push", "workflow_dispatch")):
                continue
            artifacts = api(repo, f"actions/runs/{run['id']}/artifacts?per_page=100")
            if any(a["name"] == f"canvas-desktop-{platform}" and not a["expired"]
                   for a in artifacts["artifacts"]):
                return str(run["id"])
            # Una plataforma fallida no debe esperar al resto ni bloquear el respaldo.
            jobs = api(repo, f"actions/runs/{run['id']}/jobs?per_page=100")
            own = [j for j in jobs["jobs"] if j["name"].startswith(f"{platform} /")]
            pending |= run["status"] != "completed" and (not own or any(j["status"] != "completed" for j in own))
        if not pending or time.monotonic() >= deadline:
            return ""
        print(f"Esperando artefacto preparado de {platform}...", flush=True)
        time.sleep(20)


def main():
    platform = sys.argv[1]
    sha = command("git", "rev-parse", "HEAD")
    run = select(os.environ["GITHUB_REPOSITORY"], sha, platform)
    with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as output:
        output.write(f"run_id={run}\n")


if __name__ == "__main__":
    main()
