#!/bin/sh

set -eu

# Script to wait for an ldap server to be up, clean up any existing
# data and then to do some basic initialization.
#
# This is intended for test suite setup, don't use this in production.

LDAP_HOST='ldap://ldap:1389'

# Obviously only use this for testing
LDAP_BASE='dc=example,dc=org'
LDAP_ADMIN='cn=admin,dc=example,dc=org'
LDAP_PASSWORD='adminpassword'

# Wait for ldap to be ready
retries=5

while [ $retries -gt 0 ]; do
	sleep 5
	retries=$((retries - 1))

	if ldapsearch -D "${LDAP_ADMIN}" -w "${LDAP_PASSWORD}" -H "${LDAP_HOST}" -b "${LDAP_BASE}" 'objectclass=*'; then
		break
	fi
done

# Delete the previous testorg recursively
ldapdelete -D "${LDAP_ADMIN}" -w "${LDAP_PASSWORD}" -H "${LDAP_HOST}" -r 'ou=testorg,dc=example,dc=org' || true

# Add the test org
ldapadd -D "${LDAP_ADMIN}" -w "${LDAP_PASSWORD}" -H "${LDAP_HOST}" -f /ldap-setup/testorg.ldif

# Signify that the script has completed
echo "ready" > /tmp/ready

sleep 5
