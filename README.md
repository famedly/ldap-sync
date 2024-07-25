# LDAP-sync

Sync LDAP/AD users with Zitadel.

## Quirks & Edge Cases

- Changing a user's LDAP id (the attribute from the `user_id` setting)
  is unsupported, as this is used to identify the user on the Zitadel
  end.
- Disabling a user on the LDAP side (with `status`) results in the
  user being deleted from Zitadel.
- Providing multiple values for an LDAP attribute is not supported.
- Zitadel's API is not fully atomic; if a request fails, a user may
  not be fully created and still not be functional even if the tool is
  re-used.
  - In particular, the matrix localpart, the preferred user name, and
    whether the user has permissions to use Famedly may not be synced.
- If a user's email or phone number changes, they will only be
  prompted to verify it if the tool is configured to make users verify
  them.
- Changing a user's email also immediately results in a new
  login/username.
- If SSO is turned on later, existing users will not be linked.

[![rust workflow status][badge-rust-workflow-img]][badge-rust-workflow-url]
[![docker workflow status][badge-docker-workflow-img]][badge-docker-workflow-url]

[badge-rust-workflow-img]: https://github.com/famedly/rust-project-template/actions/workflows/rust.yml/badge.svg
[badge-rust-workflow-url]: https://github.com/famedly/rust-project-template/commits/main
[badge-docker-workflow-img]: https://github.com/famedly/rust-project-template/actions/workflows/docker.yml/badge.svg
[badge-docker-workflow-url]: https://github.com/famedly/rust-project-template/commits/main

Short description of the project.

## Getting Started

Instructions of how to get the project running.

## Pre-commit usage

1. If not installed, install with your package manager, or `pip install --user pre-commit`
2. Run `pre-commit autoupdate` to update the pre-commit config to use the newest template
3. Run `pre-commit install` to install the pre-commit hooks to your local environment

---

# Famedly

**This project is part of the source code of Famedly.**

We think that software for healthcare should be open source, so we publish most
parts of our source code at [github.com/famedly](https://github.com/famedly).

Please read [CONTRIBUTING.md](CONTRIBUTING.md) for details on our code of
conduct, and the process for submitting pull requests to us.

For licensing information of this project, have a look at the [LICENSE](LICENSE.md)
file within the repository.

If you compile the open source software that we make available to develop your
own mobile, desktop or embeddable application, and cause that application to
connect to our servers for any purposes, you have to agree to our Terms of
Service. In short, if you choose to connect to our servers, certain restrictions
apply as follows:

- You agree not to change the way the open source software connects and
  interacts with our servers
- You agree not to weaken any of the security features of the open source software
- You agree not to use the open source software to gather data
- You agree not to use our servers to store data for purposes other than
  the intended and original functionality of the Software
- You acknowledge that you are solely responsible for any and all updates to
  your software

No license is granted to the Famedly trademark and its associated logos, all of
which will continue to be owned exclusively by Famedly GmbH. Any use of the
Famedly trademark and/or its associated logos is expressly prohibited without
the express prior written consent of Famedly GmbH.

For more
information take a look at [Famedly.com](https://famedly.com) or contact
us by [info@famedly.com](mailto:info@famedly.com?subject=[GitLab]%20More%20Information%20)
