# min-theme

The min*/QCView family's design tokens as **data** — canon `docs/rust-wgpu-port.md` §25 (rows 160–169, ruled
2026-11-07). The first real shared design system: today three Qt styles and four token files share 27–35 names; this
crate is the one source they will all read from.

**Shape:** Spectrum 2's three tiers, in Spectrum's own JSON token form so the structure is borrowed and the values are ours.

| File | Tier | What |
|---|---|---|
| `tokens/color-palette.json` | 1 — global ramp | 13 grays + the hue ramps (blue, green, orange, red, yellow, cyan, cinnamon, purple), dark values |
| `tokens/color-aliases.json` | 2 — roles | background base / layer-1 / layer-2 / elevated; content / subdued / disabled; neutral, hover, selection; border, divider, track, focus; accent; positive / notice / negative / informative; side A / B; note marker; shadow colours |
| `tokens/layout.json` | 1 + aliases | corner radii (4 / 8 / 10 / 16 / full), component heights (20 / 24 / 32 / 40), spacing (2 … 40), icon sizes (toolbar 20), hairlines, drop shadows, motion and thresholds |
| `tokens/typography.json` | 1 + aliases | Geist / Geist Mono / Phosphor; sizes 11 … 22 with 12 as the live-text floor; line heights 1.3 / 1.5; detail = tracked capitals |
| `tokens/component.json` | 3 — components | action button, pill button, chip, tooltip, popover, dialog, field, slider, scrollbar, rail, bands, timecode well, the scrubber, the clip bars, the hover thumbnail |

A token is `"name": { "value": X }` where `X` is a colour (`#rrggbb[aa]`, `rgb()`, `rgba()`), a length (`"8px"`), a
duration (`"100ms"`), an em value, a bare number, a string, a shadow layer list, or a reference `"{other-name}"`. The one
extension over Spectrum's format is `"alpha": f` on a colour reference — the referenced colour with its alpha replaced
(the family's hover-ramp rule: the idle end is the hover colour at alpha 0, never "transparent").

`build.rs` resolves every reference (a dangling or cyclic one fails the build by name) and generates one typed
`pub const` per token in `min_theme::tokens` (`corner-radius-100` → `CORNER_RADIUS_100: f32`), plus `tokens::ALL`
and `tokens::get(name)` for tooling. `min-theme-dump [--json] [prefix]` prints the resolved set.

**Also here:** the Phosphor slug → codepoint table (`icons/phosphor.json`, QCView's `PhosphorIcons.js`, 1 512 names;
`min_theme::icons::glyph("play")`) and the six bundled faces under `assets/fonts/` behind the default `fonts`
feature (Geist Sans / Geist Mono under the SIL Open Font License 1.1, Phosphor under MIT — licences beside the files).

**Dark only**, by product direction. The ramp keeps a light theme possible as a value flip; nothing is built for it.

**Attribution:** the gray and hue ramp values, the radius / height / spacing / type scales and the drop-shadow recipes
are Adobe Spectrum 2's published desktop tokens (`adobe/spectrum-design-data`, Apache-2.0), adopted as data; the alias
choices, the component tokens, the motion and threshold values are the family's own (canon §16.4, §25).
