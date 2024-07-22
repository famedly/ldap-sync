#!/bin/sh

set -eu

# Shut down any still running ldap-setup first
docker compose --project-directory ./tests/environment down -v ldap-setup || true
# docker compose --project-directory ./tests/environment down --remove-orphans --volumes || true
docker compose --project-directory ./tests/environment up --wait
