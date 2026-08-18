# Implementation scaffold

These files define the proposed public contracts and naming, but they are not a complete commit.
They are intentionally small enough to review before wiring them into `UtaSkinTransformer`,
`UtaPitchGuide`, `UtaPitchCurveGraph`, `UtaLyricsDisplay`, the settings pages and the importer.

Recommended implementation order:

1. Add lookups and `UtaVisualStyle` resolution with built-in defaults.
2. Add the legacy `.osk` adapter and verify `Uta-Prism.osk` assets.
3. Replace hard-coded UI colours/metrics with the resolved style snapshot.
4. Add responsive settings descriptors/localisation.
5. Add the video surface abstraction and reference-pitch Auto path.
6. Add structured import diagnostics and regression tests.

Do not load fonts from the skin archive. Use bundled lazer fonts and glyph fallback.
