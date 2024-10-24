#!/bin/sh

set -eu

# CI does not add /usr/bin to $PATH for some reason, which means we
# lack docker
export PATH="${PATH}:/usr/bin"

# Make sure the containers can write some files that need to be shared
touch tests/environment/zitadel/service-user.json
chmod a+rw tests/environment/zitadel/service-user.json

# Shut down any still running test-setup first
docker compose --project-directory ./tests/environment down -v test-setup -v ldap || true
docker compose --project-directory ./tests/environment up --wait
