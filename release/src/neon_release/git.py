from datetime import datetime, timezone
from neon_release.context import ctx
from re import match
from subprocess import run, PIPE
from typing import Optional


def run_git(
    args: list[str],
    *,
    check: bool = True,
    capture_output: bool = False,
    dry_run: Optional[bool] = None,
) -> Optional[str]:
    """Run a git command and return stdout as string (trimmed)."""
    if dry_run is None:
        dry_run = ctx.dry_run
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
    run_git(["switch", "-c", new_branch, f"origin/{base_branch}"])


def switch_to_branch(branch: str):
    run_git(["switch", branch])


def discard_head():
    run_git(["reset", "--hard", "HEAD~"])


def current_branch() -> str:
    return run_git(
        ["rev-parse", "--abbrev-ref", "HEAD"], capture_output=True, dry_run=False
    )


def apply_commits(commits: list[str]):
    for commit in commits:
        run_git(["cherry-pick", commit])


def get_tree_sha(commit: str) -> str:
    return run_git(
        ["rev-parse", f"{commit}^{{tree}}"], capture_output=True, dry_run=False
    )


def get_commit_sha(ref: str) -> str:
    return run_git(["rev-parse", ref], capture_output=True, dry_run=False)


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
    return run_git(
        ["commit-tree", tree, "-p", first_parent, "-p", second_parent, "-m", message],
        capture_output=True,
        dry_run=False,
    )


def fast_forward_branch_to(commit: str):
    run_git(["merge", "--ff-only", commit])


def push_current_branch_to_origin(branch: str):
    run_git(["push", "-u", "origin", branch])


def rc_branch_name() -> str:
    return (
        f"rc/release-{ctx.component}/{ctx.reference_time.strftime('%Y-%m-%dT%H:%MZ')}"
    )


def release_branch_name() -> str:
    if ctx.component == "storage":
        return "release"
    else:
        return f"release-{ctx.component}"


def merge_message() -> str:
    return f"{ctx.component.capitalize()} release {ctx.reference_time.strftime('%Y/%m/%d %H:%M UTC')}"


def parse_rc_branch_to_context(branch: str) -> tuple[str, datetime]:
    """
    Parse a branch like: rc/release-<component>/<timestamp>
    where timestamp is ISO 8601 formatted (e.g. 2024-08-26T16:34Z)

    Returns:
        (component, datetime with UTC timezone)
    """
    pattern = r"^rc/release-(?P<component>[^/]+)/(?P<timestamp>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}Z)$"
    match = match(pattern, branch)
    if not match:
        raise ValueError(f"Invalid RC branch name format: {branch}")

    ctx.component = match.group("component")
    ctx.reference_time = datetime.strptime(
        match.group("timestamp"), "%Y-%m-%dT%H:%MZ"
    ).replace(tzinfo=timezone.utc)
