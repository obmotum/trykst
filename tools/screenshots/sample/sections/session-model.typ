= Session model
Each session stores a fingerprint of the rules that derive a profile from
the ID token. A session is renewed when the fingerprint changes:

$ f = H(v || c_"roles" || c_"admin" || c_"guest") $

#figure(
  image("../figures/sign-in.svg", width: 90%),
  caption: [Sign-in with an active IdP session.],
)

#lorem(24)
