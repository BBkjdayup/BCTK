# TKup local patch

Based on the four source files in crates.io `tex2word-math` 1.0.6, upstream
<https://github.com/yfyang86/rstex2word>. Copyright and license: [LICENSE](LICENSE).
Original crates.io archive SHA-256:
`03ee46945f5a099874cef47c387768e9d806586110a493918e91dc5690f6f237`.

TKup uses `try_to_omath` for all DOCX exports. Unlike the upstream permissive
API, it reports unsupported commands, malformed structures and bounded-input
failures instead of silently dropping content. This patch also supports the
no-bar `\genfrac{}{}{0pt}{}{a}{b}` emitted by TKup's OMML importer.

Keep these changes covered by the desktop export tests when updating upstream.
The legacy `to_omath` API is retained only for upstream compatibility.
