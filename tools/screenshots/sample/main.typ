#import "template.typ": paper

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
    colleagues can be added to a project before they ever log in. #lorem(24)
  ],
)

#include "sections/introduction.typ"
#include "sections/session-model.typ"
#include "sections/evaluation.typ"

= Related work
Existing editors either ship their own account system or support single
sign-on as an optional add-on next to local passwords. #lorem(60)

= Conclusion
Delegating identity entirely to the IdP removes a whole class of account
management features while making single sign-on effortless. #lorem(24)

#bibliography("refs.bib")
