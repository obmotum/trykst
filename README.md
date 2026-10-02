# Trykst

**Collaborative Typst editor with native OIDC and seamless SSO.**

[![Typst Version](https://img.shields.io/badge/Typst-0.15.1-239dad?logo=typst&logoColor=white)](https://typst.app/)
[![Rust](https://img.shields.io/badge/Rust-1.82+-orange?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![SvelteKit](https://img.shields.io/badge/SvelteKit-5-ff3e00?logo=svelte)](https://kit.svelte.dev/)
[![OpenID Connect](https://img.shields.io/badge/OpenID_Connect-F78C40?logo=openid&logoColor=white)](https://openid.net/connect/)
[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![CI](https://github.com/obmotum/trykst/actions/workflows/ci.yml/badge.svg)](https://github.com/obmotum/trykst/actions/workflows/ci.yml)
[![Docker image](https://github.com/obmotum/trykst/actions/workflows/docker-publish.yml/badge.svg)](https://github.com/obmotum/trykst/pkgs/container/trykst)

Trykst is a self-hosted web editor for [Typst](https://typst.app/) documents, built for organizations and home labs that already run an identity provider. There are no local accounts and no login page: people are signed in through your OpenID Provider (Keycloak, Authentik, Entra ID, …), so with an active IdP session opening Trykst just works.

Trykst is a fork of [TypstDrive](https://github.com/SirBlobby/TypstDrive) by SirBlobby, rebuilt around OIDC. See [Origin and license](#origin-and-license).

## Features

### Identity and access
- **OIDC only, seamless SSO**: authorization code flow with PKCE. Visitors without a session are sent straight to the IdP and come back to the page they asked for.
- **Just-in-time accounts**: users are created on first sign-in and keyed by `(issuer, subject)`; name and email are kept in sync with the IdP.
- **Roles from the IdP**: admin rights, access restriction and guest status come from a roles claim or from Keycloak Organizations membership.
- **Guests for external users**: guests work in the projects they were added to. They cannot create projects or API keys, nor browse the directory.
- **Central logout**: signing out ends the IdP session too (RP-initiated logout), and back-channel logout ends Trykst sessions the moment the IdP revokes them.
- **Server-side sessions** with a maximum age; a re-login at the IdP is silent while its session lasts.
- **Always-current profiles**: name, email, picture and roles come from the IdP at sign-in. When an update or a change to the role settings affects them, existing sessions are renewed automatically, silently while the IdP session lasts.

### Projects, documents and files
- **Projects** are the unit of collaboration: a project holds documents and has members. Everything in Trykst lives in a project.
- **Documents** compile to one output. Each document has its own files: Typst sources, images, fonts, bibliographies, organized in folders that can be nested freely.
- **File tree** with drag and drop: move files and folders, drop files from the desktop into a folder to upload them, rename in place. One file is the main file the document is compiled from.

### Sharing
- **Project members** with three roles: owners manage members and the project, editors create and edit documents, viewers read. A project can have several owners and always keeps at least one.
- **People picker**: find colleagues by name, email or domain in the IdP directory, including people who have never signed in; shows avatar, organization and email.
- **Add people before their first login**: adding someone creates a placeholder account that is linked on their first sign-in.
- **Link sharing per document**: off, viewer or editor for anyone with the link, without signing in.

### Editing
- **Real-time collaboration**: Yjs and CodeMirror 6 with live cursors, per file.
- **Instant preview**: Typst compiled to SVG on the fly, with zoom and a collapsible preview pane. Errors name the file they occur in.
- **Packages**: publish a document with a `typst.toml` as a versioned Typst package. Project packages are importable by the documents of the same project as `@project/<name>:<version>`; admins can publish instance-wide packages as `@trykst/<name>:<version>`.
- **Comments** on documents and files, and a **version history** that snapshots the whole document with all its files.
- **Export** to PDF, PNG, SVG, HTML, Markdown, Word or LaTeX (Pandoc).
- **Presentation mode** with slide controls and an annotation overlay.
- **Custom fonts and images**, uploaded into the document that uses them.
- **Render API**: `POST /v1/render` with API keys, documented at `/api-docs`.
- **Themes**: Catppuccin, Arch Linux, Cerberus, light and dark.

## Quick start

Trykst needs an OpenID Provider. For a local test, the repository ships a Keycloak with a ready-made realm:

```bash
git clone --recursive --shallow-submodules https://github.com/obmotum/trykst.git
cd trykst

docker compose -f docker-compose.dev.yml up -d   # Keycloak on http://localhost:8080
```

Then set the variables listed at the top of `docker-compose.dev.yml` and run the server (see [Local development](#local-development)). Test users: `alice`, `bob`, `carol`, `erik` and more, the password is the username.

## Self-hosting

A Docker image packages the Rust backend and the SvelteKit frontend into one container. It is published to GitHub Container Registry for `linux/amd64` and `linux/arm64` (e.g. Raspberry Pi 4/5, Ampere, Apple Silicon hosts); Docker picks the matching architecture automatically:

| Tag | Content |
|---|---|
| `ghcr.io/obmotum/trykst:latest` | Latest build of `main` |
| `ghcr.io/obmotum/trykst:1.2.3`, `:1.2` | Releases (git tags `v1.2.3`) |
| `ghcr.io/obmotum/trykst:sha-<commit>` | A specific commit |

In `docker-compose.yml`, replace `build: .` with `image: ghcr.io/obmotum/trykst:latest`, or build the image yourself:

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

## Files, fonts and images

Fonts and images belong to the document that uses them: upload them into its file tree, by dropping them onto a folder or with the upload button. There are no account-wide or project-wide assets.

Upload `.ttf` or `.otf` fonts anywhere in the document. Trykst reads the family name from the file, registers all its variants and makes them available to the compiler and the `tinymist` language server:

```typst
#set text(font: "JetBrains Mono")
```

For Google Fonts, extract the ZIP and upload all `.ttf` files at once; a variable font needs only its single file.

Files are referenced by their path, relative to the file that uses them or, with a leading `/`, from the document root. Remote images work by URL:

```typst
#include "chapters/introduction.typ"
#image("figures/logo.png", width: 50%)
#image("/figures/logo.png", width: 50%)
#image("https://example.com/logo.png", width: 50%)
```

To reuse code or assets across documents, publish them as a package (File → Publish as package) and import it with `#import "@project/<name>:<version>": *`.

## Local development

1. Fetch the Typst compiler submodule if you cloned without `--recursive`: `git submodule update --init --depth 1`. Its pinned commit must match the one in the `Dockerfile`.
2. Start the dev Keycloak: `docker compose -f docker-compose.dev.yml up -d`.
3. Install `tinymist` and put it on your `PATH`; the backend uses it for LSP features.
4. Frontend: `npm install`, then `npm run build` (served by the backend) or `npm run dev` (proxies `/api` and `/yjs` to port 3000).
5. Backend: set the variables from the header of `docker-compose.dev.yml`, then `cd server && cargo run`. On Windows, `pwsh -File dev/run-server.ps1` does both (and builds the frontend if needed).

Set `CARGO_TARGET_DIR` outside synced folders such as OneDrive; the build directory grows to several gigabytes.

The README screenshots are generated by [`tools/screenshots`](tools/screenshots/README.md) against the dev setup.

## Upgrading from 0.1.x

Trykst 0.2 introduced projects and is a **fresh start: data from 0.1.x (or TypstDrive) is not migrated**. Accounts are recreated on the next sign-in; documents, spaces, files and packages are not carried over.

1. Export what you want to keep while still running 0.1.x (File → Download in the editor).
2. Either point `DATABASE_URL` at a new, empty database, or set `TRYKST_DROP_LEGACY_DATA=true` for one start to **delete all existing data** in the current one. Without either, 0.2 refuses to start on a 0.1.x database and leaves it untouched.
3. Remove `TRYKST_DROP_LEGACY_DATA` again, sign in, create a project and upload your files.

What changed for users:

- Documents and spaces became documents inside projects; every document can now have several files and folders.
- Sharing with individual people moved from the document to the project. Link sharing stays per document.
- Fonts and images are uploaded into a document instead of the account.
- `#import "@trykst/…"` packages are now published by admins; everyone else publishes `@project/…` packages inside a project. `#import "@typstdrive/…"` still resolves to instance-wide packages.

Compared to TypstDrive, password login, registration, the setup wizard and the desktop sync API are gone as well.

## Origin and license

Trykst is a fork of [TypstDrive](https://github.com/SirBlobby/TypstDrive), Copyright 2026 SirBlobby, maintained by Aleksandar Trkulja. It is distributed under the [Apache License 2.0](LICENSE) like the original. See [NOTICE](NOTICE) for attribution and a summary of the changes.

Typst is a project of the Typst team; Trykst is not affiliated with or endorsed by it.

## Screenshots

<p align="center">
  <img src="preview/editor.png" alt="Document editor with file tree and preview" width="100%">
</p>
<p align="center">
  <img src="preview/project.png" alt="A project with its documents" width="100%">
</p>
<p align="center">
  <img src="preview/dashboard.png" alt="Project list" width="100%">
</p>
