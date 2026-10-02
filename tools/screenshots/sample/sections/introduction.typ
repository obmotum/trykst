= Introduction
Collaborative writing tools usually maintain their own user database.
In organizations that already operate an identity provider this creates
a second source of truth. OpenID Connect @oidc-core lets the editor rely
on that provider instead. #lorem(30)

== Design goals
- No local accounts or passwords
- Silent sign-in while an IdP session exists
- Roles, guests and organizations from token claims
