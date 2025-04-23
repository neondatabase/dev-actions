# Neon Release PR

A GitHub Action and CLI tool for creating and managing release PRs for Neon components.

## Overview

The `neon-release-pr` tool automates the process of creating release pull requests for Neon components. It handles the creation of release branches, cherry-picking commits, crafting merge commits, and setting up GitHub PR options like auto-approval and auto-merge.

## GitHub Action Usage

### Example 1: Interactive Workflow with Inputs

Add the action to your workflow with user inputs:

```yaml
name: Create Release PR

on:
  workflow_dispatch:
    inputs:
      component:
        description: 'Component to release'
        required: true
        type: string
      hotfix:
        description: 'Create a hotfix release'
        required: false
        type: boolean
        default: false
      cherry-pick:
        description: 'Commits to cherry-pick (JSON array)'
        required: false
        type: string
        default: '[]'
      auto-merge:
        description: 'Enable auto-merge after approval'
        required: false
        type: boolean
        default: false
      auto-approve:
        description: 'Automatically approve the PR'
        required: false
        type: boolean
        default: false

jobs:
  create-release-pr:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Configure git
        run: |
          git config user.name "github-actions[bot]"
          git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
      
      - name: Create Release PR
        uses: neondatabase/dev-actions/release-pr@main
        with:
          component: ${{ inputs.component }}
          hotfix: ${{ inputs.hotfix }}
          cherry-pick: ${{ inputs.cherry-pick }}
          auto-merge: ${{ inputs.auto-merge }}
          auto-approve: ${{ inputs.auto-approve }}
        env:
          GH_TOKEN: ${{ secrets.PR_CREATION_TOKEN }}
          GH_TOKEN_APPROVE: ${{ secrets.PR_APPROVAL_TOKEN }}
```

### Example 2: Scheduled Automated Release

This example shows a workflow that runs on a schedule and automatically creates release PRs, with automatic approval and automatic merging:

```yaml
name: Foo release PR
on:
  schedule:
    - cron: "*/30 * * * *" # every 30 minutes
  workflow_dispatch:

jobs:
  release:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Configure git
        run: |
          git config user.name "github-actions[bot]"
          git config user.email "41898282+github-actions[bot]@users.noreply.github.com"

      - name: Run release action
        uses: neondatabase/dev-actions/release-pr@main
        with:
          component: foo
          auto-approve: true
          auto-merge: true
        env:
          GH_TOKEN: ${{ secrets.PR_CREATION_TOKEN }}
          GH_TOKEN_APPROVE: ${{ secrets.PR_APPROVAL_TOKEN }}
```

## Installation

You can install the tool directly from the repository:

```bash
pip install git+https://github.com/neondatabase/dev-actions.git#subdirectory=release-pr
```

## CLI Usage

The tool can also be used as a CLI:

```bash
# Create a new release PR
neon-release-pr new <component> [options]

# Amend an existing release PR
neon-release-pr amend start [--branch <branch>]
# Make your changes
neon-release-pr amend finish
```

For a complete list of options and commands, use the built-in help:

```bash
# General help
neon-release-pr --help

# Command-specific help
neon-release-pr new --help
neon-release-pr amend --help
neon-release-pr amend start --help
neon-release-pr amend finish --help
```
