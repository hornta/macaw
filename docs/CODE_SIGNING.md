# Code signing policy

Macaw's Windows releases are signed so that Windows can verify they come from this project and were not modified.
Signing is being set up with [SignPath Foundation](https://signpath.org). Once it is active, signed releases say:

> Free code signing provided by [SignPath.io](https://signpath.io), certificate by [SignPath Foundation](https://signpath.org).

The certificate's publisher is "SignPath Foundation".

## What gets signed

Only builds made by GitHub Actions from this repository's source code, by the release workflow
([`.github/workflows/release.yml`](../.github/workflows/release.yml)) when a version tag is pushed:

- `macaw.exe` for x64 and for ARM64
- the installer `Macaw-<version>-setup.exe`

Nothing built elsewhere is signed, and every signing request is approved by hand.

## Team roles

| Role | Who |
|---|---|
| Authors: may change code without further review | [@hornta](https://github.com/hornta) |
| Reviewers: review every change from anyone else | [@hornta](https://github.com/hornta) |
| Approvers: approve every signing request | [@hornta](https://github.com/hornta) |

Everyone in these roles uses multi-factor authentication for GitHub and SignPath.

## Privacy

This program will not transfer any information to other networked systems unless specifically requested by the user
or the person installing or operating it.

Macaw has no network access at all. See [Privacy](../README.md#privacy) in the README.

## Setting up signing (maintainers)

SignPath Foundation only signs projects that already have a release, so the first release (0.1.0) is unsigned.

1. Turn on two-factor authentication on GitHub.
2. Apply for free open-source code signing at [signpath.org](https://signpath.org), with a link to this repository and
   to this policy.
3. Once approved, in SignPath:
   - create the project `macaw` with GitHub (github.com) as its trusted build system for this repository;
   - add the artifact configurations `programs` and `installer` from
     [`.signpath/artifact-configurations/`](../.signpath/artifact-configurations);
   - create the signing policy `release-signing` with yourself as approver;
   - create an API token for a CI user that may submit signing requests.
4. In this repository's settings, under **Secrets and variables → Actions**, add:
   - secret `SIGNPATH_API_TOKEN`: the API token;
   - variable `SIGNPATH_ORGANIZATION_ID`: the SignPath organization ID;
   - optionally the variables `SIGNPATH_PROJECT_SLUG` and `SIGNPATH_SIGNING_POLICY_SLUG`, if you chose names other
     than `macaw` and `release-signing`.
5. Push a version tag. The release workflow pauses until you approve its two signing requests in SignPath, then
   creates a draft release with the signed files.
