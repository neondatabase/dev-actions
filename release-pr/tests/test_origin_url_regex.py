from neon_release_pr import git


def test_round_trip_parse_then_generate():
    test_cases = [
        ("https://github.com/databricks-eng/hadron.git", "databricks-eng/hadron"),
        ("git@github.com:neondatabase/neon.git", "neondatabase/neon"),
        ("ssh://git@github.com:22/neondatabase/neon.git", "neondatabase/neon"),
        ("https://github.com/databricks-eng/hadron", "databricks-eng/hadron"),
        (
            "git@github-neon.com:neondatabase/dev-actions.git",
            "neondatabase/dev-actions",
        ),
        ("git@github.com-emu:databricks-eng/hadron.git", "databricks-eng/hadron"),
        (
            "ssh://git@github-neon.com:22/neondatabase/dev-actions.git",
            "neondatabase/dev-actions",
        ),
        ("git@github.com-emu:databricks-eng/hadron.git", "databricks-eng/hadron"),
    ]
    for url, expected_repo in test_cases:
        assert git.github_repo(url) == expected_repo, (
            f"Expected {expected_repo} for url {url}"
        )
