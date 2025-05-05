from datetime import datetime, timezone
from neon_release_pr.context import ctx
from shutil import which
from subprocess import run, PIPE, CalledProcessError
from typing import Optional
import re


def ready():
    if which("git") is None:
        raise RuntimeError("git CLI is not installed or not found in PATH")

    try:
        run_git(
            ["config", "--get", "user.name"],
            capture_output=True,
            dry_run=False,
            silent=True,
        )
    except CalledProcessError:
        raise RuntimeError(
            "git user.name is not configured. Please run: git config --global user.name 'Your Name'"
        )

    try:
        run_git(
            ["config", "--get", "user.email"],
            capture_output=True,
            dry_run=False,
            silent=True,
        )
    except CalledProcessError:
        raise RuntimeError(
            "git user.email is not configured. Please run: git config --global user.email 'you@example.com'"
        )


def run_git(
    args: list[str],
    *,
    check: bool = True,
    capture_output: bool = False,
    dry_run: Optional[bool] = None,
    silent: bool = False,
) -> Optional[str]:
    """Run a git command and return stdout as string (trimmed)."""
    if dry_run is None:
        dry_run = ctx.dry_run
    if not silent:
        print(f"[{'dry-run' if dry_run else 'running'}] git {' '.join(args)}")
    if dry_run:
        return ""
    result = run(
        ["git", *args],
        check=check,
        stdout=PIPE if capture_output else None,
        text=True,
    )
    return result.stdout.strip() if capture_output else None


def fetch_all():
    run_git(["fetch", "--all"])


def create_from_remote(base_branch: str, new_branch: str):
    # Assume the base_branch is a remote branch by default, fall back to local
    # for branches not pushed to remote or commit hashes
    try:
        run_git(["switch", "-c", new_branch, f"origin/{base_branch}"])
    except Exception:
        run_git(["switch", "-c", new_branch, base_branch])


def switch_to_branch(branch: str):
    run_git(["switch", branch])


def discard_head():
    run_git(["reset", "--hard", "HEAD~"])


def current_branch() -> str:
    current_branch = run_git(
        ["rev-parse", "--abbrev-ref", "HEAD"], capture_output=True, dry_run=False
    )
    assert isinstance(current_branch, str)
    return current_branch


def apply_commits(commits: list[str]):
    for commit in commits:
        run_git(["cherry-pick", commit])


def get_tree_sha(commit: str) -> str:
    tree_sha = run_git(
        ["rev-parse", f"{commit}^{{tree}}"], capture_output=True, dry_run=False
    )
    assert isinstance(tree_sha, str)
    return tree_sha


def get_commit_sha(ref: str) -> str:
    commit_sha = run_git(["rev-parse", ref], capture_output=True, dry_run=False)
    assert isinstance(commit_sha, str)
    return commit_sha


def verify_commit(commit: str) -> str:
    commit_sha = run_git(
        ["rev-parse", "--verify", f"{commit}^{{commit}}"],
        capture_output=True,
        dry_run=False,
    )
    assert isinstance(commit_sha, str)
    return commit_sha


def create_release_merge_commit():
    """
    Craft a merge commit for a component's release PR.

    - Uses tree of current HEAD
    - Uses current HEAD as the first parent
    - Uses origin/<release_branch> as the second parent
    - Message is "<Component> release YYYY-MM-DD"
    """
    head = get_commit_sha("HEAD")
    release_head = get_commit_sha(f"origin/{release_branch_name()}")
    tree = get_tree_sha(head)
    message = merge_message()
    merge_commit = create_merge_commit_from_tree(tree, head, release_head, message)
    fast_forward_branch_to(merge_commit)


def create_merge_commit_from_tree(
    tree: str, first_parent: str, second_parent: str, message: str
) -> str:
    commit_sha = run_git(
        ["commit-tree", tree, "-p", first_parent, "-p", second_parent, "-m", message],
        capture_output=True,
        dry_run=False,
    )
    assert isinstance(commit_sha, str)
    return commit_sha


def fast_forward_branch_to(commit: str):
    run_git(["merge", "--ff-only", commit])


def push_current_branch_to_origin(branch: str):
    run_git(["push", "-u", "origin", branch])


def rc_branch_name() -> str:
    return (
        f"rc/release-{ctx.component}/{ctx.reference_time.strftime('%Y-%m-%dT%H-%MZ')}"
    )


def release_branch_name() -> str:
    if ctx.component == "storage":
        return "release"
    else:
        return f"release-{ctx.component}"


def merge_message() -> str:
    return f"{ctx.component.capitalize()} release {ctx.reference_time.strftime('%Y-%m-%d %H:%M UTC')}"


def parse_rc_branch_to_context(branch: str):
    """
    Parse a branch like: rc/release-<component>/<timestamp>
    where timestamp is nearly ISO 8601 formatted (e.g. 2024-08-26T16-34Z)
    We use a dash for separating hours and minutes because git disallows colons in ref names

    Returns:
        (component, datetime with UTC timezone)
    """
    pattern = r"^rc/release-(?P<component>[^/]+)/(?P<timestamp>\d{4}-\d{2}-\d{2}T\d{2}-\d{2}Z)$"
    match = re.match(pattern, branch)
    if not match:
        raise ValueError(f"Invalid RC branch name format: {branch}")

    ctx.component = match.group("component")
    ctx.reference_time = datetime.strptime(
        match.group("timestamp"), "%Y-%m-%dT%H:%MZ"
    ).replace(tzinfo=timezone.utc)
