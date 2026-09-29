# GitHub App setup for read-only product auth

Nzube connects to GitHub through a dedicated GitHub App that uses the device flow. The app requests read access to repository contents, issues, and pull requests, and nothing else. No step below has been performed yet. Until a real app is registered and the check at the end passes, product authentication is **unverified**.

Sources are GitHub's own docs, as of 2026-09-29: "Registering a GitHub App using URL parameters", "Registering a GitHub App", "Generating a user access token for a GitHub App", "Refreshing user access tokens", "Permissions required for GitHub Apps", and "Reviewing and revoking authorization of GitHub Apps".

## 1. Register the app

Open this link while signed in as the account that should own the app. It opens GitHub's registration form with the settings below already filled in. Nothing is created until you click **Create GitHub App**.

```text
https://github.com/settings/apps/new?description=Your+intent%2C+ready+for+your+agent.&url=https%3A%2F%2Fgithub.com%2FAojdevStudio%2Fnzube&public=true&contents=read&issues=read&pull_requests=read
```

For an organization-owned app, use `https://github.com/organizations/<ORGANIZATION>/settings/apps/new` with the same query string.

The link prefills these settings:

| Setting | Value |
| --- | --- |
| Description | Your intent, ready for your agent. |
| Homepage URL | `https://github.com/AojdevStudio/nzube` |
| Where it can be installed | Any account (`public=true`) |
| Repository permissions | Contents: Read-only; Issues: Read-only; Pull requests: Read-only. Metadata: Read-only is mandatory and added by GitHub |
| Webhook | Off. Webhooks are disabled by default for URL-parameter registration |

Set these by hand on the form, because URL parameters cannot set them or because they need a choice:

1. **GitHub App name.** "Nzube" is not available, because a GitHub user account already has that login, and app names cannot match another account's login. Choose the app name yourself.
2. **Enable Device Flow.** Check it. No URL parameter sets this option.
3. **Expire user authorization tokens.** Leave it checked. User tokens then last 8 hours, and a refresh token lasts 6 months.
4. **Callback URL.** Leave it empty. The device flow ignores it.
5. **Request user authorization (OAuth) during installation.** Leave it unchecked.
6. **Webhook: Active.** Confirm it is unchecked.
7. **Permissions.** Confirm that nothing beyond the four read permissions above is selected. Do not add any write permission.

After you create the app:

- Copy the **Client ID** from the app's settings page. It is public and is the only value Nzube needs.
- Do **not** generate a client secret or a private key. Nzube uses neither, and a desktop or mobile app cannot keep a secret confidential.

**Why not the manifest flow.** The manifest flow needs a server at `redirect_url` and a code exchange within one hour. That exchange returns a client secret, a private key, and a webhook secret. The manifest also has no field for enabling the device flow.

## 2. Give Nzube the client ID

Put the client ID in the application's configuration, and do not hardcode it in this library. The library accepts it through `ClientId::parse`. The example reads it from `NZUBE_GITHUB_APP_CLIENT_ID`.

## 3. Install the app on a test repository

Open `https://github.com/apps/<app-slug>/installations/new`, choose the account or organization, and select **Only select repositories** with one test repository. A GitHub App user token can reach only the repositories where the app is installed and the user has access. Other repositories return 404.

## 4. Prove product auth

Use a private test repository that has at least one issue with comments and one pull request.

```sh
NZUBE_GITHUB_APP_CLIENT_ID=<client id> \
  cargo run -p github-evidence --example device_login -- <owner>/<private-test-repo> main <issue> <pull>
```

The example prints a code. Enter it at `https://github.com/login/device`. The example then reads the repository with the `ghu_` user token, refreshes the token without a client secret, and reads again with both the new token and the old one.

Record these as passing:

- [ ] Device authorization completes, and the token prefix check (`ghu_`) passes.
- [ ] The resolve, instructions-file, history, issue, comments, pull request, files, and diff reads each return `ok` on the private repository.
- [ ] Refresh succeeds without a client secret, and the refreshed token reads successfully.
- [ ] A repository where the app is not installed returns `Unavailable` (404).
- [ ] After you revoke the authorization at `https://github.com/settings/apps/authorizations`, a read returns `InvalidCredentials` (401).

Record the old-token result after refresh as observed. GitHub's docs say the old token stops working, but this crate has not seen it happen.

## Disconnect versus revocation

A client without a secret can only delete its local tokens. That is a *disconnect*. GitHub describes the endpoints that delete an app token or grant as operations for the application owner, so Nzube should not rely on them. *Revocation* on GitHub's side is the user's action at `https://github.com/settings/apps/authorizations`. Organization owners can instead uninstall the app. The product must label these two outcomes differently.
