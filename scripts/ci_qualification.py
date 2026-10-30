"""Reuse a successful main build only for the identical tagged commit.

The branch always builds. Tags wait briefly for main, then reuse its qualified
artifact or run the complete matrix. No branch-name-only or PR qualification.
"""
import argparse
import json
import os
from pathlib import Path
import re
import time
from urllib.parse import urlencode
from urllib.request import Request, urlopen

import check_release

ARTIFACT = "Constellation-windows-x64"
REQUIRED = {"core (windows-latest)", "core (ubuntu-latest)"}


class GitHub:
    def __init__(self):
        self.repo = os.environ["GITHUB_REPOSITORY"]
        self.token = os.environ["GH_TOKEN"]

    def get(self, suffix):
        request = Request(
            f"https://api.github.com/repos/{self.repo}/actions/{suffix}",
            headers={"Authorization": f"Bearer {self.token}",
                     "Accept": "application/vnd.github+json",
                     "X-GitHub-Api-Version": "2022-11-28"},
        )
        with urlopen(request, timeout=20) as response:
            return json.load(response)


def matches(run, sha):
    return (run.get("head_sha") == sha and run.get("head_branch") == "main"
            and run.get("event") == "push"
            and run.get("path", "").split("@")[0] == ".github/workflows/ci.yml")


def qualified(api, run, sha):
    if not matches(run, sha) or run.get("conclusion") != "success":
        return False
    jobs = api.get(f"runs/{int(run['id'])}/jobs?per_page=100")["jobs"]
    passed = {j["name"] for j in jobs if j.get("conclusion") == "success"}
    if not REQUIRED <= passed:
        return False
    artifacts = api.get(f"runs/{int(run['id'])}/artifacts?per_page=100")["artifacts"]
    return any(a.get("name") == ARTIFACT and not a.get("expired", True)
               for a in artifacts)


def resolve(api, sha, seconds=240, clock=time.monotonic, pause=time.sleep):
    deadline = clock() + seconds
    discovery_deadline = clock() + min(30, seconds)
    query = urlencode({"head_sha": sha, "event": "push", "branch": "main", "per_page": 100})
    while True:
        runs = api.get(f"workflows/ci.yml/runs?{query}")["workflow_runs"]
        candidates = [r for r in runs if matches(r, sha)]
        for run in candidates:
            if qualified(api, run, sha):
                return int(run["id"])
        pending = (any(r.get("status") != "completed" for r in candidates)
                   or (not candidates and clock() < discovery_deadline))
        remaining = deadline - clock()
        if not pending or remaining <= 0:
            return None
        pause(min(10, remaining))


def verify(api, run_id, sha, folder):
    run = api.get(f"runs/{run_id}")
    assert qualified(api, run, sha), "Source run no longer qualifies this commit"
    version = check_release.check()
    archive = Path(folder) / f"Constellation-{version}-windows-x64.zip"
    digest = check_release.inspect_package(archive, version)
    print(f"Reused exact-commit qualification: run {run_id}; {archive.name}; sha256:{digest}")
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as stream:
            stream.write(f"Qualified exact commit `{sha}` using [main run {run_id}]"
                         f"(https://github.com/{api.repo}/actions/runs/{run_id}).\n\n"
                         f"Both Windows/Linux jobs passed. Package SHA-256: `{digest}`.\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["resolve", "verify"])
    parser.add_argument("--run-id", type=int)
    parser.add_argument("--folder", type=Path, default=Path("dist"))
    args = parser.parse_args()
    sha = os.environ["GITHUB_SHA"]
    assert re.fullmatch(r"[0-9a-f]{40}", sha), "Invalid commit SHA"
    if args.mode == "verify":
        assert args.run_id and args.run_id > 0, "Missing qualified source run"
        verify(GitHub(), args.run_id, sha, args.folder)
        return
    run_id = None
    if os.environ.get("GITHUB_REF", "").startswith("refs/tags/"):
        try:
            run_id = resolve(GitHub(), sha)
        except Exception as error:
            # API unavailability must cause full checks, never a false success.
            print(f"Qualification lookup unavailable ({type(error).__name__}); running full matrix.")
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        output.write(f"reuse={'true' if run_id else 'false'}\nrun_id={run_id or ''}\n")
    print(f"Reusing main run {run_id}." if run_id else "Full matrix required.")


if __name__ == "__main__":
    main()
