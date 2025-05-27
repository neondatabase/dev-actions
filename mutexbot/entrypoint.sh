#!/usr/bin/env bash
set -euo pipefail

export MUTEXBOT_API_KEY="${INPUT_API_KEY}"

args=()
if [[ -n "${INPUT_ISOLATION_CHANNEL}" ]]; then
  args+=( "--isolation-channel" "${INPUT_ISOLATION_CHANNEL}" )
fi

case "${INPUT_MODE}" in
  "reserve")
    args+=(
      "reserve"
      "${INPUT_RESOURCE_NAME}"
      "Reserved by ${GITHUB_SERVER_URL}/${GITHUB_REPOSITORY}/actions/runs/${GITHUB_RUN_ID}"
    )
    if [[ -n "${INPUT_DURATION}" ]]; then
      args+=( "${INPUT_DURATION}" )
    fi
    mutexbot "${args[@]}"
    ;;
  *)
    args+=( "${INPUT_MODE}" "${INPUT_RESOURCE_NAME}" )
    mutexbot "${args[@]}"
    ;;
esac
