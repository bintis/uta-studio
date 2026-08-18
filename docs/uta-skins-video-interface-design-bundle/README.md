# uta! Skins, Video and Interface Polish - delivery bundle

This bundle was designed against `bintis/uta-ruleset` commit
`ce7fd7d0d571c7d1c52f89265209d1d0761cf449`.

## Start here

- `uta-skins-video-interface-design.zh-CN.pdf` - review-ready design document.
- `uta-skins-video-interface-design.zh-CN.md` - editable source.
- `design/index.html` - visual design board.
- `design/mockups/*.svg` - editable artboards.
- `skin/Uta-Prism/` - exact contents used to build `Uta-Prism.osk`.
- `implementation-scaffold/` - lookup and data-contract starter types.

## Important compatibility note

The current ruleset does not yet request uta!-specific skin elements. The `.osk` is a valid
standard osu! skin archive and will import, but its `uta-*` assets become active only after the
native lookup bridge in this design is implemented.

No font files are included or required.
