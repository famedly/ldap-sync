#!/bin/sh
set -eux

openssl req -x509 -new -nodes -sha256 -newkey rsa:2048 \
		-keyout server.key \
		-out server.crt \
		-subj "/C=DE/CN=example.org" \
		-addext "subjectAltName = DNS:zitadel, DNS:localhost"

# These keys are not actually secret, and when passed into the docker
# container the server key needs to be readable by the container user
chmod go+r server.key

openssl x509 -outform pem -in server.crt -out ca.crt
openssl req -x509 -nodes -days 3650 -sha256 -newkey rsa:2048 \
		-CAkey server.key \
		-CA ca.crt \
		-keyout client.key \
		-out client.crt \
		-subj "/CN=admin.example.org"
