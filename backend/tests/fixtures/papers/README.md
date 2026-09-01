# Research paper fixtures

These PDFs exercise GROBID against real publisher-generated research articles.
They are kept only as integration-test inputs.

- `plos-one-0215794.pdf`
  - Joan E. Ball-Damerow et al., "Research applications of primary biodiversity databases in the digital age"
  - DOI: <https://doi.org/10.1371/journal.pone.0215794>
  - Source: <https://journals.plos.org/plosone/article/file?id=10.1371/journal.pone.0215794&type=printable>
  - License: Creative Commons Attribution (CC BY)
- `zookeys-725-079.pdf`
  - Atsunobu Murase, Ryohei Miki, Hiroyuki Motomura, "Southern limits of distribution of the intertidal gobies..."
  - DOI: <https://doi.org/10.3897/zookeys.725.19952>
  - Source: <https://zookeys.pensoft.net/article/19952/download/pdf/>
  - License: Creative Commons Attribution 4.0 (CC BY 4.0)

The ignored test `real_grobid_extracts_metadata_from_research_paper_fixtures`
requires GROBID 0.9.1 and validates title, authors, DOI, and year. With
`consolidateHeader=0`, journal and page fields remain optional because GROBID's
real BibTeX responses omit them for these fixtures.
