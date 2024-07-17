#!/bin/sh

# Extend lldap with the extra attributes we need
lldap-cli -H ldap:17170 -D admin -w password schema attribute user add telephonenumber string -v -e
lldap-cli -H ldap:17170 -D admin -w password schema attribute user add useraccountcontrol integer -v -e

# Signify that the script has completed
echo "ready" > /ready

# Sleep long enough for docker to pick up the health file
sleep 60
