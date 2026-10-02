# Runs Trykst locally against the dev Keycloak from docker-compose.dev.yml:
# sets the variables listed in that file's header, builds the frontend if it
# has not been built yet and starts the backend with `cargo run`.
#
#   pwsh -File dev/run-server.ps1 [-Database trykst-dev.db]
#
# Then open http://localhost:3000 and sign in as alice, bob or erik
# (password = username). The SQLite file is created in server/.
param([string]$Database = "trykst-dev.db")

$ErrorActionPreference = "Stop"
$repo = Split-Path $PSScriptRoot -Parent

try {
    Invoke-WebRequest "http://localhost:8080/realms/trykst/.well-known/openid-configuration" -TimeoutSec 5 | Out-Null
} catch {
    Write-Warning "The dev Keycloak is not reachable. Start it with: docker compose -f docker-compose.dev.yml up -d"
}

if (-not (Test-Path "$repo\build\index.html")) {
    Write-Host "Building the frontend..."
    Push-Location $repo
    try {
        if (-not (Test-Path "node_modules")) { npm install }
        npm run build
    } finally {
        Pop-Location
    }
}

$env:DATABASE_URL = "sqlite://$Database`?mode=rwc"
$env:DB_TYPE = "sqlite"
$env:STATIC_DIR = "$repo\build"
# Development only: a fixed secret keeps sessions across restarts.
$env:COOKIE_SECRET = "dev-cookie-secret-dev-cookie-secret-dev-cookie-secret-dev-cookie-secret"
$env:PUBLIC_URL = "http://localhost:3000"
$env:OIDC_ISSUER = "http://localhost:8080/realms/trykst"
$env:OIDC_CLIENT_ID = "trykst"
$env:OIDC_CLIENT_SECRET = "trykst-dev-secret"
$env:OIDC_ROLES_CLAIM = "realm_access.roles"
$env:OIDC_ADMIN_ROLE = "trykst-admin"
$env:OIDC_REQUIRED_ROLE = "trykst-user"
$env:OIDC_GUEST_ROLE = "trykst-external"
$env:OIDC_SCOPES = "openid profile email organization:*"
$env:OIDC_HOME_ORGANIZATIONS = "example"
$env:OIDC_DIRECTORY = "keycloak"
$env:OIDC_DIRECTORY_ORG_SOURCE = "organizations"
if (-not $env:RUST_LOG) { $env:RUST_LOG = "server=info" }

Write-Host "Trykst on http://localhost:3000 (database: server\$Database). Stop with Ctrl+C."
Set-Location "$repo\server"
cargo run
