#!/bin/sh

set -eu

# Shut down any still running test-setup first
docker compose --project-directory ./tests/environment down -v test-setup || true
docker compose --project-directory ./tests/environment up --wait
