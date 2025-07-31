from neon_release_pr import git


def test_round_trip_parse_then_generate():
    for url in [
        "https://github.com/databricks-eng/hadron.git",
        "git@github.com:neondatabase/neon.git",
        "ssh://git@github.com:22/neondatabase/neon.git",
        "https://github.com/databricks-eng/hadron",
    ]:
        assert git.github_repo(url) is not None, f"No match for url {url}"
