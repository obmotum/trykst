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
