#!/bin/sh

set -eu

# docker compose --project-directory ./tests/environment down --remove-orphans --volumes || true
docker compose --project-directory ./tests/environment up --wait
