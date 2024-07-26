#!/bin/sh

set -eu

# CI does not add /usr/bin to $PATH for some reason, which means we
# lack docker
export PATH="${PATH}:/usr/bin"

# Shut down any still running test-setup first
docker compose --project-directory ./tests/environment down -v test-setup || true
docker compose --project-directory ./tests/environment up --wait
