from neon_release.context import ctx
from os import environ
from subprocess import run, PIPE
from typing import Optional


def run_gh(
    args: list[str],
    *,
    check: bool = True,
    capture_output: bool = False,
    token: Optional[str] = None,
) -> Optional[str]:
    print(f"[{'dry-run' if ctx.dry_run else 'running'}] gh {' '.join(args)}")
    if ctx.dry_run:
        return ""
    result = run(
        ["gh", *args],
        check=check,
        stdout=PIPE if capture_output else None,
        text=True,
        env={**environ, "GH_TOKEN": token or environ.get("GH_TOKEN", "")},
    )
    return result.stdout.strip() if capture_output else None


def create_pr(
    branch: str,
    base: str,
    title: str,
    labels: list[str] = [],
) -> str:
    args = [
        "pr",
        "create",
        "--head",
        branch,
        "--base",
        base,
        "--title",
        title,
    ]
    for label in labels:
        args += ["--label", label]
    return run_gh(args, capture_output=True)


def enable_auto_merge(pr: str):
    run_gh(["pr", "merge", "--merge", "--auto", pr])


def approve_pr(pr: str):
    token = environ.get("GH_TOKEN_APPROVE")
    if not token:
        raise RuntimeError("GH_TOKEN_APPROVE is not set")
    run_gh(["pr", "review", "--approve", pr], token=token)
