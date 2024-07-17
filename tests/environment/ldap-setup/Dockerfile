FROM alpine:3.19

RUN wget -O /usr/local/bin/lldap-cli 'https://github.com/Zepmann/lldap-cli/raw/6d9efd5d83492878886a556159a3be90472c5d84/lldap-cli'; \
		chmod +x /usr/local/bin/lldap-cli; \
		apk add --no-cache bash curl jq
ADD ldap-setup.sh /ldap-setup.sh
CMD /ldap-setup.sh
HEALTHCHECK CMD ["test" "-f" "/ready"]
