FROM bitnami/openldap:latest

USER root

RUN apt-get update && apt-get upgrade -y && \
	apt-get install --yes curl golang-go jq && \
	apt-get clean && rm -rf /var/lib/apt/lists /var/cache/apt/archives

RUN GOPATH=/ go install github.com/zitadel/zitadel-tools@latest
