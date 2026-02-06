from datetime import datetime, timezone
from neon_release_pr import git, gh
from neon_release_pr.context import ctx
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
    git.ready()


@app.command()
def new(
    component: Annotated[str, typer.Argument(metavar="COMPONENT", show_default=False)],
    cherry_pick: Annotated[
        list[str] | None,
        typer.Argument(
            metavar="COMMIT...",
            show_default=False,
            help="Commit(s) to cherry-pick onto the release branch. Implies --base set to the previous release.",
        ),
    ] = None,
    base: Annotated[
        str | None,
        typer.Option(
            "--base",
            "--from-commit-sha",
            metavar="REF",
            help="Base to release from.",
            show_default="main",
        ),
    ] = None,
    auto_merge: Annotated[
        bool,
        typer.Option(
            "--auto-merge",
            "--automatic",
            help="Enable auto-merge after PR approval.",
        ),
    ] = False,
    auto_approve: Annotated[
        bool,
        typer.Option(
            "--approve",
            help="Automatically approve the PR.",
        ),
    ] = False,
):
    ctx.component = component
    ctx.reference_time = datetime.now(timezone.utc)

    ctx.validate()
    gh.ready()

    if cherry_pick and base is None:
        base = git.release_branch_name()
    elif base is None:
        base = git.base_branch_name()

    branch_name = git.rc_branch_name()

    typer.echo(f"[info] Creating release branch for component '{component}'")
    typer.echo(f"[info] Base branch: {base}")
    typer.echo(f"[info] RC branch: {branch_name}")
    typer.echo(f"[info] Time: {ctx.reference_time.isoformat()}")

    try:
        git.fetch_all()
        git.create_from_remote(base, branch_name)
        if cherry_pick:
            verified_commits = []
            for commit in cherry_pick:
                try:
                    verified_commits.append(git.verify_commit(commit))
                except Exception:
                    raise typer.BadParameter(
                        f'Invalid commit "{commit}" passed to cherry_pick!'
                    )
            typer.echo(f"[info] Applying {len(verified_commits)} cherry-pick(s)...")
            git.apply_commits(verified_commits)
        if component == "compute":
            git.update_compute_tag_in_manifest(git.current_branch())
        git.create_release_merge_commit()
        git.push_current_branch_to_origin(branch_name)
        typer.echo("[success] RC branch pushed successfully")

    except Exception as e:
        typer.echo(f"[error] {type(e).__name__}: {e}", err=True)
        raise typer.Exit(code=1)

    pr_title = git.merge_message()
    labels = ["release/hotfix"] if cherry_pick else []

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
        str | None,
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
