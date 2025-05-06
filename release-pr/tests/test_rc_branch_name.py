import pytest
from datetime import datetime, timezone
from neon_release_pr import git
from neon_release_pr.context import ctx


@pytest.fixture
def example_component():
    return "storage"


@pytest.fixture
def example_timestamp():
    # datetime corresponding to 2025-05-06T10:36Z
    return datetime(2025, 5, 6, 10, 36, tzinfo=timezone.utc)


@pytest.fixture
def example_branch():
    return "rc/release-storage/2025-05-06T10-36Z"


def test_context_to_branch_name(example_component, example_timestamp, example_branch):
    ctx.component = example_component
    ctx.reference_time = example_timestamp
    assert git.rc_branch_name() == example_branch


def test_branch_name_to_context(example_component, example_timestamp, example_branch):
    git.parse_rc_branch_to_context(example_branch)
    assert ctx.component == example_component
    assert ctx.reference_time == example_timestamp


def test_round_trip_parse_then_generate(example_branch):
    git.parse_rc_branch_to_context(example_branch)
    assert git.rc_branch_name() == example_branch


def test_invalid_rc_branch_format():
    with pytest.raises(ValueError):
        git.parse_rc_branch_to_context("rc/release-console/not-a-time")
