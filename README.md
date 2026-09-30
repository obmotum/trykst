# Trykst

**Collaborative Typst editor with native OIDC and seamless SSO.**

[![Typst Version](https://img.shields.io/badge/Typst-0.15.1-239dad?logo=typst&logoColor=white)](https://typst.app/)
[![Rust](https://img.shields.io/badge/Rust-1.82+-orange?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![SvelteKit](https://img.shields.io/badge/SvelteKit-5-ff3e00?logo=svelte)](https://kit.svelte.dev/)
[![OpenID Connect](https://img.shields.io/badge/OpenID_Connect-F78C40?logo=openid&logoColor=white)](https://openid.net/connect/)
[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

Trykst is a self-hosted web editor for [Typst](https://typst.app/) documents, built for organizations and home labs that already run an identity provider. There are no local accounts and no login page: people are signed in through your OpenID Provider (Keycloak, Authentik, Entra ID, …), so with an active IdP session opening Trykst just works.

Trykst is a fork of [TypstDrive](https://github.com/SirBlobby/TypstDrive) by SirBlobby, rebuilt around OIDC. See [Origin and license](#origin-and-license).

## Features

### Identity and access
- **OIDC only, seamless SSO**: authorization code flow with PKCE. Visitors without a session are sent straight to the IdP and come back to the page they asked for.
- **Just-in-time accounts**: users are created on first sign-in and keyed by `(issuer, subject)`; name and email are kept in sync with the IdP.
- **Roles from the IdP**: admin rights, access restriction and guest status come from a roles claim or from Keycloak Organizations membership.
- **Guests for external users**: guests only see what was shared with them and cannot create documents, spaces, packages or API keys, nor browse the directory.
- **Central logout**: signing out ends the IdP session too (RP-initiated logout), and back-channel logout ends Trykst sessions the moment the IdP revokes them.
- **Server-side sessions** with a maximum age; a re-login at the IdP is silent while its session lasts.

### Sharing
- **People picker**: find colleagues by name, email or domain in the IdP directory, including people who have never signed in; shows avatar, organization and email.
- **Invite before first login**: sharing with someone creates a placeholder account that is linked on their first sign-in.
- **Roles per document**: editor or viewer, plus link sharing for anyone with the link.

### Editing
- **Real-time collaboration**: Yjs and CodeMirror 6 with live cursors.
- **Instant preview**: Typst compiled to SVG on the fly, with zoom and a collapsible preview pane.
- **Spaces**: multi-file projects with their own `typst.toml`, `.typ`, `.bib` and asset files, a file tree and per-file collaboration.
- **Instance-local packages**: publish a Space as a versioned Typst package, importable everywhere as `@trykst/<name>:<version>`.
- **Comments and version history**.
- **Export** to PDF, PNG, SVG, HTML, Markdown, Word or LaTeX (Pandoc).
- **Presentation mode** with slide controls and an annotation overlay.
- **Custom fonts and images**, uploaded per account or per document.
- **Render API**: `POST /v1/render` with API keys, documented at `/api-docs`.
- **Themes**: Catppuccin, Arch Linux, Cerberus, light and dark.

## Quick start

Trykst needs an OpenID Provider. For a local test, the repository ships a Keycloak with a ready-made realm:

```bash
git clone <your-trykst-repo-url> trykst
cd trykst
git clone https://github.com/typst/typst.git typst
git -C typst checkout 9dfd3a08500b7896045f907433cf7b4b02434fad

docker compose -f docker-compose.dev.yml up -d   # Keycloak on http://localhost:8080
```

Then set the variables listed at the top of `docker-compose.dev.yml` and run the server (see [Local development](#local-development)). Test users: `alice`, `bob`, `carol`, `erik` and more, the password is the username.

## Self-hosting

A Docker image packages the Rust backend and the SvelteKit frontend into one container. Build it from the repository:

```bash
docker compose up -d --build
```

Edit `docker-compose.yml` first: it documents every variable. Data lives in SQLite by default (`/data/trykst.db` in the `appdata` volume); PostgreSQL is supported via `DATABASE_URL` and `DB_TYPE=postgres`.

### Required configuration

| Variable | Example | Description |
|---|---|---|
| `OIDC_ISSUER` | `https://sso.example.com/realms/example` | Issuer URL; endpoints are discovered from it. |
| `OIDC_CLIENT_ID` | `trykst` | Client id at the IdP. |
| `OIDC_CLIENT_SECRET` | | Client secret (confidential client). |
| `PUBLIC_URL` | `https://trykst.example.com` | Externally reachable URL of Trykst. |
| `COOKIE_SECRET` | `openssl rand -hex 64` | 64+ byte secret; without it sessions end on every restart. |

Register these URLs at the IdP:
- Redirect URI: `<PUBLIC_URL>/api/auth/oidc/callback`
- Post-logout redirect URI: `<PUBLIC_URL>/`
- Back-channel logout URL: `<PUBLIC_URL>/api/auth/oidc/backchannel-logout`

### Optional configuration

| Variable | Default | Description |
|---|---|---|
| `OIDC_SCOPES` | `openid profile email` | Requested scopes. Add `organization:*` for Keycloak Organizations. |
| `OIDC_ROLES_CLAIM` | | Dot path to the roles or groups claim, e.g. `realm_access.roles` or `groups`. |
| `OIDC_ADMIN_ROLE` | | Members of this role are admins. Without it, the first user becomes admin and admins are managed in Trykst. |
| `OIDC_REQUIRED_ROLE` | | Only members of this role (or guests, or home-organization members) may sign in. |
| `OIDC_GUEST_ROLE` | | Members of this role are guests, unless they also hold the required or admin role. |
| `OIDC_HOME_ORGANIZATIONS` | | Comma-separated organization aliases. Members are regular users; members of only other organizations are guests. |
| `OIDC_ORGANIZATIONS_CLAIM` | `organization` | Claim holding the user's organizations. |
| `OIDC_SUBJECT_CLAIM` | `sub` | Stable user id claim (`oid` for Entra ID). |
| `OIDC_USERNAME_CLAIM` | `preferred_username` | Claim used as display username. |
| `OIDC_DIRECTORY` | `none` | `keycloak` enables the people picker via the Keycloak Admin API. |
| `OIDC_DIRECTORY_ORG_SOURCE` | `attribute` | `attribute` or `organizations` (Keycloak Organizations). |
| `OIDC_DIRECTORY_ORG_ATTRIBUTE` | `organization` | User attribute shown as organization when the source is `attribute`. |
| `SESSION_MAX_AGE_HOURS` | `10` | Maximum session length before a (silent) re-login at the IdP. |
| `DATABASE_URL` | `sqlite:///data/trykst.db?mode=rwc` | SQLite or `postgres://user:pass@host:5432/trykst`. |
| `DB_TYPE` | auto | `sqlite` or `postgres`. |
| `PORT` | `3000` | HTTP port. |
| `RUST_LOG` | `server=debug,tower_http=debug` | Log filter; `info` for production. |

### Keycloak setup

1. **Client**: confidential, standard flow, PKCE `S256`, the three URLs above.
2. **Roles in the ID token**: in the `roles` client scope, enable *Add to ID token* on the realm roles mapper (or on the client roles mapper, depending on `OIDC_ROLES_CLAIM`).
3. **People picker** (`OIDC_DIRECTORY=keycloak`): enable *Service accounts* on the client and grant it the `realm-management` roles `view-users` and `query-users`.
4. **Organizations** (Keycloak 26+, optional): enable Organizations for the realm, make sure the `organization` client scope is assigned to the client (optional), and set `OIDC_SCOPES=openid profile email organization:*`, `OIDC_HOME_ORGANIZATIONS=<your alias>` and `OIDC_DIRECTORY_ORG_SOURCE=organizations`.

External users sign in through Keycloak like everyone else, via an organization of their own (for example with identity brokering to their company's IdP) or with an account in your realm.

> Recreating the realm instead of migrating it gives users new ids. Trykst then cannot match existing accounts. Export and import realms instead.

Other providers work through the same standard settings; see `OIDC_SUBJECT_CLAIM` for Entra ID.

## Fonts and images

Upload `.ttf` or `.otf` fonts from the dashboard or the editor toolbar. Trykst reads the family name from the file, registers all its variants and makes them available to the compiler and the `tinymist` language server immediately:

```typst
#set text(font: "JetBrains Mono")
```

For Google Fonts, extract the ZIP and upload all `.ttf` files at once; a variable font needs only its single file.

Uploaded images are referenced by filename, remote images by URL:

```typst
#image("logo.png", width: 50%)
#image("https://example.com/logo.png", width: 50%)
```

## Local development

1. Clone Typst into `typst/` at the commit pinned in the `Dockerfile` (see [Quick start](#quick-start)).
2. Start the dev Keycloak: `docker compose -f docker-compose.dev.yml up -d`.
3. Install `tinymist` and put it on your `PATH`; the backend uses it for LSP features.
4. Frontend: `npm install`, then `npm run build` (served by the backend) or `npm run dev` (proxies `/api` and `/yjs` to port 3000).
5. Backend: set the variables from the header of `docker-compose.dev.yml`, then `cd server && cargo run`.

Set `CARGO_TARGET_DIR` outside synced folders such as OneDrive; the build directory grows to several gigabytes.

## Migrating from TypstDrive

- Accounts are matched to IdP identities by verified email on the first sign-in.
- Packages move from the `typstdrive` to the `trykst` namespace automatically; `#import "@typstdrive/…"` keeps working.
- Password login, registration, the setup wizard and the desktop sync API are gone.
- The default database file is now `trykst.db`; point `DATABASE_URL` at your existing file.

## Origin and license

Trykst is a fork of [TypstDrive](https://github.com/SirBlobby/TypstDrive), Copyright 2026 SirBlobby, maintained by Aleksandar Trkulja. It is distributed under the [Apache License 2.0](LICENSE) like the original. See [NOTICE](NOTICE) for attribution and a summary of the changes.

Typst is a project of the Typst team; Trykst is not affiliated with or endorsed by it.

## Screenshots

<p align="center">
  <img src="preview/editor.png" alt="Editor view" width="100%">
</p>
<p align="center">
  <img src="preview/dashboard.png" alt="Dashboard view" width="100%">
</p>
