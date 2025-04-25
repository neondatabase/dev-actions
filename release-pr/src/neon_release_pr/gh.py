from neon_release_pr.context import ctx
from os import environ
from shutil import which
from subprocess import run, PIPE, CalledProcessError, DEVNULL
from typing import Optional


def ready():
    """Ensure GitHub CLI is installed and authenticated."""
    if which("gh") is None:
        raise RuntimeError(
            "GitHub CLI (gh) is not installed. Please install it: https://cli.github.com/"
        )

    try:
        run(["gh", "auth", "status"], check=True, stdout=DEVNULL, stderr=DEVNULL)
    except CalledProcessError:
        print("[info] GitHub CLI not authenticated, running 'gh auth login'...")
        run(["gh", "auth", "login", "--hostname", "github.com"], check=True)


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
        "--body",
        "",
    ]
    for label in labels:
        args += ["--label", label]
    pr_url = run_gh(args, capture_output=True)
    if ctx.dry_run:
        pr_url = "<pr-url-placeholder>"
    assert isinstance(pr_url, str)
    return pr_url


def enable_auto_merge(pr: str):
    run_gh(["pr", "merge", "--merge", "--auto", pr])


def approve_pr(pr: str):
    token = environ.get("GH_TOKEN_APPROVE")
    if not token:
        raise RuntimeError("GH_TOKEN_APPROVE is not set")
    run_gh(["pr", "review", "--approve", pr], token=token)
