uta! Prism 1.0.0
================

This archive is a standard osu! skin (.osk), not a separate uta! package format.
It contains uta!-specific texture names beginning with "uta-" and a marker texture
named "uta-skin-marker".

Current repository limitation
-----------------------------
At baseline commit ce7fd7d0d571c7d1c52f89265209d1d0761cf449, UtaSkinTransformer
only customises global HUD composition. The uta-* assets will import with the skin,
but the current ruleset does not yet request them. Implement the lookup bridge in
the accompanying design/scaffold before expecting in-game changes.

Fallback behaviour
------------------
- Missing target notes, live curve or playhead: ruleset vector fallback remains.
- Missing grid, trail or particles: default optional layer is used; a skin can
  intentionally disable an optional drawable by returning Drawable.Empty().
- No font files are included. Lyrics use lazer-bundled fonts and framework glyph
  fallback.
- Colour and motion safety are enforced by the ruleset, not trusted to texture
  colours alone.

Asset naming is documented in design/contracts/uta-skin-lookups.json.
