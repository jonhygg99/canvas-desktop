"""Reutiliza CI del PR solo cuando su árbol verificado es idéntico al de main."""
import json
import os
from pathlib import Path
import subprocess
import tempfile


def api(repo, endpoint):
    return json.loads(subprocess.check_output(["gh", "api", f"repos/{repo}/{endpoint}"], text=True))


def verified_run(repo, sha, tree):
    pulls = api(repo, f"commits/{sha}/pulls")
    for pull in pulls:
        if (not pull.get("merged_at") or pull["merge_commit_sha"] != sha
                or pull["base"]["ref"] != "main"
                or pull["head"]["repo"]["full_name"] != repo):
            continue
        runs = api(repo, f"actions/workflows/ci.yml/runs?event=pull_request&head_sha={pull['head']['sha']}&per_page=100")
        for run in sorted(runs["workflow_runs"], key=lambda r: r["id"], reverse=True):
            if run["conclusion"] != "success" or run["event"] != "pull_request":
                continue
            artifacts = api(repo, f"actions/runs/{run['id']}/artifacts?per_page=100")
            if not any(a["name"] == "verified-ci-tree" and not a["expired"] for a in artifacts["artifacts"]):
                continue
            with tempfile.TemporaryDirectory() as temp:
                subprocess.run(["gh", "run", "download", str(run["id"]), "--repo", repo,
                                "--name", "verified-ci-tree", "--dir", temp], check=True)
                if (Path(temp) / "tree.txt").read_text().strip() == tree:
                    return str(run["id"])
    return ""


def main():
    run = ""
    if os.environ["GITHUB_EVENT_NAME"] == "push" and os.environ["GITHUB_REF"] == "refs/heads/main":
        sha = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
        tree = subprocess.check_output(["git", "rev-parse", "HEAD^{tree}"], text=True).strip()
        try:
            run = verified_run(os.environ["GITHUB_REPOSITORY"], sha, tree)
        except (subprocess.CalledProcessError, KeyError, OSError, ValueError) as error:
            print(f"No se pudo reutilizar CI; se ejecutan todos los controles: {error}")
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        output.write(f"reuse={'true' if run else 'false'}\nsource_run={run}\n")
    print(f"CI verificada equivalente: {run or 'ninguna; ejecutar CI completa'}")


if __name__ == "__main__":
    main()
