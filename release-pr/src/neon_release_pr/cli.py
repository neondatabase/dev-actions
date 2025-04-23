from datetime import datetime, timezone
from neon_release_pr import git, gh
from neon_release_pr.context import ctx
from typing import Optional
from typing_extensions import Annotated
import typer

app = typer.Typer()
amend_app = typer.Typer()


@app.callback()
def main(
    dry_run: Annotated[
        bool,
        typer.Option(
            "--dry-run", help="Only print what commands to run instead of running them"
        ),
    ] = False,
):
    """neon-release is a CLI tool for releasing components at neon"""
    ctx.dry_run = dry_run


@app.command()
def new(
    component: Annotated[str, typer.Argument(envvar="NEON_RELEASE_PR_COMPONENT")],
    base: Annotated[
        Optional[str],
        typer.Option(metavar="REF", help="Base to release from.", show_default="main"),
    ] = None,
    hotfix: Annotated[
        bool,
        typer.Option(
            "--hotfix",
            help="Shortcut: set base to <release-branch>.",
            envvar="NEON_RELEASE_PR_HOTFIX",
        ),
    ] = False,
    cherry_pick: Annotated[
        list[str],
        typer.Option(
            "--cherry-pick",
            metavar="COMMIT",
            help="Cherry-pick commits (can be used multiple times).",
            envvar="NEON_RELEASE_PR_CHERRY_PICK",
        ),
    ] = [],
    auto_merge: Annotated[
        bool,
        typer.Option(
            "--auto-merge",
            help="Enable auto-merge after PR approval.",
            envvar="NEON_RELEASE_PR_AUTO_MERGE",
        ),
    ] = False,
    auto_approve: Annotated[
        bool,
        typer.Option(
            "--auto-approve",
            help="Automatically approve the PR.",
            envvar="NEON_RELEASE_PR_AUTO_APPROVE",
        ),
    ] = False,
):
    if hotfix and base is not None:
        raise typer.BadParameter("--hotfix cannot be used together with --base")
    elif hotfix:
        base = f"release-{component}"
    elif base is None:
        base = "main"

    ctx.component = component
    ctx.reference_time = datetime.now(timezone.utc)

    ctx.validate()

    branch_name = git.rc_branch_name()

    typer.echo(f"[info] Creating release branch for component '{component}'")
    typer.echo(f"[info] Base branch: {base}")
    typer.echo(f"[info] RC branch: {branch_name}")
    typer.echo(f"[info] Time: {ctx.reference_time.isoformat()}")

    try:
        git.fetch_all()
        git.create_from_remote(base, branch_name)
        if cherry_pick:
            typer.echo(f"[info] Applying {len(cherry_pick)} cherry-pick(s)...")
            git.apply_commits(list(cherry_pick))
        git.create_release_merge_commit()
        git.push_current_branch_to_origin(branch_name)
        typer.echo("[success] RC branch pushed successfully")

    except Exception as e:
        typer.echo(f"[error] {type(e).__name__}: {e}", err=True)
        raise typer.Exit(code=1)

    pr_title = git.merge_message()
    labels = ["release/hotfix"] if hotfix else []

    try:
        pr_url = gh.create_pr(branch_name, git.release_branch_name(), pr_title, labels)
        typer.echo(f"[success] Created PR: {pr_url}")

        if auto_approve:
            gh.approve_pr(pr_url)
            typer.echo("[success] Approved PR")

        if auto_merge:
            gh.enable_auto_merge(branch_name)
            typer.echo("[success] Enabled auto-merge")
    except Exception as e:
        typer.echo(f"[error] GitHub PR actions failed: {e}", err=True)
        raise typer.Exit(code=1)


@amend_app.command("start")
def amend_start(
    branch: Annotated[
        Optional[str],
        typer.Option(
            metavar="REF",
            help="RC branch to amend",
        ),
    ] = None,
):
    if branch is not None:
        git.fetch_all()
        git.switch_to_branch(branch)

    git.parse_rc_branch_to_context(git.current_branch())

    ctx.validate()

    git.discard_head()


@amend_app.command("finish")
def amend_finish():
    git.parse_rc_branch_to_context(git.current_branch())

    ctx.validate()

    git.create_release_merge_commit()


app.add_typer(amend_app, name="amend")
