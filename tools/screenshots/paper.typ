#let paper(title: "", authors: (), abstract: none, body) = {
  set document(author: authors.map(a => a.name), title: title)
  set page(paper: "a4", margin: (x: 1.8cm, y: 2cm), numbering: "1")
  set text(size: 10pt, lang: "en")
  set heading(numbering: "1.1")
  set par(justify: true, leading: 0.58em)

  align(center)[
    #block(text(weight: 700, 1.6em, title))
    #v(0.6em)
  ]

  grid(
    columns: (1fr,) * authors.len(),
    gutter: 1em,
    ..authors.map(author => align(center)[
      *#author.name* \
      #author.affiliation \
      #text(size: 0.85em, raw(author.email))
    ]),
  )

  if abstract != none {
    v(1em)
    pad(x: 2.5em)[
      #align(center, text(weight: 700)[Abstract])
      #abstract
    ]
  }

  v(1em)
  show: columns.with(2, gutter: 1.6em)
  body
}

#show: paper.with(
  title: "Seamless Single Sign-On for Collaborative Typesetting",
  authors: (
    (name: "Alice Admin", affiliation: "Example IT Solutions", email: "alice@example.test"),
    (name: "Erik Extern", affiliation: "Partner AG", email: "erik@partner.test"),
  ),
  abstract: [
    We describe a self-hosted editor for Typst documents that delegates
    every aspect of identity to an OpenID Provider. Accounts are created on
    first sign-in, roles and guest status follow the identity provider, and
    colleagues can be invited before they ever log in. #lorem(28)
  ],
)

= Introduction
Collaborative writing tools usually maintain their own user database.
In organizations that already operate an identity provider this creates
a second source of truth. #lorem(36)

== Design goals
- No local accounts or passwords
- Silent sign-in while an IdP session exists
- Roles, guests and organizations from token claims

= Session model
Each session stores a fingerprint of the rules that derive a profile from
the ID token. A session is renewed when the fingerprint changes:

$ f = H(v || c_"roles" || c_"admin" || c_"guest") $

#lorem(30)

= Evaluation
#figure(
  table(
    columns: 3,
    align: (left, right, right),
    table.header[*Database*][*Sign-in*][*Checks*],
    [PostgreSQL 16], [OIDC], [31 / 31],
    [SQLite], [OIDC], [31 / 31],
  ),
  caption: [End-to-end results per database.],
)

#lorem(40)

= Related work
Existing editors either ship their own account system or support single
sign-on as an optional add-on next to local passwords. #lorem(70)

= Limitations
Directory search currently targets Keycloak; other providers fall back to
email invitations until a directory backend exists. #lorem(50)

= Conclusion
Delegating identity entirely to the IdP removes a whole class of account
management features while making single sign-on effortless. #lorem(24)
