# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0] - 2024-08-13

### Bug Fixes

- Set required ldap attributes
- Don't exit when a single user fails to sync
- Don't sync disabled users
- Make the main function log issues in the config file
- Implement `PartialEq` to do deep byte comparison
- Correctly handle ldap_poller errors
- Print error context when errors make it to the main function
- Don't set passwordless registration for users

### Continuous Integration Pipeline

- Update docker workflow
- Fix missing entry to `PATH`
- Don't run everything in a container so we can use docker
- Print docker logs on failure
- Remove coverage-related actions

### Documentation

- Add basic doc comments across the project
- Document edge cases
- Add documentation for testing
- Document LDAPS testing architecture
- Document usage for end users

### Features

- Creation
- Implement Zitadel user creation
- Implement LDAP sync cache
- Add preferred username to user metadata
- Add user grants
- Add UUID to synced users
- Delete disabled users
- Implement propagating LDAP user deletion
- Implement user change sync
- Log successful outcomes better
- [**breaking**] Make LDAPS connections work correctly
- Make phone numbers optional
- Properly handle binary attributes
- Make tls config optional
- [**breaking**] Make using attribute filters optional
- [**breaking**] Implement bitflag for status attribute with multiple disable values
- [**breaking**] Make SSO setup mandatory and assert SSO works properly

### Miscellaneous Tasks

- Fix yaml editorconfig
- Update Dockerfile
- Update to new zitadel-rust-client Zitadel::new()
- Switch famedly dependency URLs from ssh to https
- Remove no longer relevant TODO comment

### Refactor

- Stop using the ldap cache for now
- Clean up user conversion to more easily persist metadata
- Properly represent user fields that aren't static values
- Implement display for our user struct
- Factor the user struct out into its own module

### Styling

- Remove unnecessary imports in the config module
- Give methods proper names

### Testing

- Implement infrastructure for e2e testing
- Switch to openldap for testing
- Clean up Zitadel org before running the tests
- Assert that the Zitadel user is actually created
- Clean up tests a bit by making a struct for ldap
- Implement further e2e test cases
- Improve test setup logging
- Assert that email changes are handled correctly
- Move test template config to allow splitting docs and tests
- Allow `change_user` to take binary values
- Fix missing `.success()` calls on ldap functions
- Add test for syncing binary values

<!-- generated by git-cliff -->