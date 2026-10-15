# Built-in fonts

Embedded by the `default-fonts` feature (on by default). Each file is a
subset of the original: Latin, Latin-1 and Extended, Greek, Cyrillic, general
punctuation, euro, trade, arrows, minus, box drawing (U+2500-257F) and
geometric shapes (U+25A0-25FF). All layout features are kept.

| File | Source | Version | Licence | Original | Subset |
|------|--------|---------|---------|----------|--------|
| `Inter-Regular.ttf` | https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip, `extras/ttf/Inter-Regular.ttf` | Inter 4.1 | `OFL-Inter.txt` (SIL OFL 1.1) | 411,640 B | 204,820 B |
| `Inter-Bold.ttf` | same archive, `extras/ttf/Inter-Bold.ttf` | Inter 4.1 | `OFL-Inter.txt` | 420,428 B | 208,688 B |
| `JetBrainsMono-Regular.ttf` | https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip, `fonts/ttf/JetBrainsMono-Regular.ttf` | JetBrains Mono 2.304 | `OFL-JetBrainsMono.txt` (SIL OFL 1.1) | 273,900 B | 145,740 B |
| `m5x7.ttf` | https://commondatastorage.googleapis.com/leafo/fonts/m5x7-webfont.ttf (the webfont served by https://managore.itch.io/m5x7) | m5x7 (Daniel Linssen) | `m5x7-LICENSE.txt` (CC0 1.0) | 18,780 B | 17,508 B |

The m5x7 webfont is ASCII-only, so pixel text outside ASCII falls back to
Inter.

## Subset command

`hb-subset` from HarfBuzz (Homebrew `harfbuzz`), run on each original:

```bash
U="U+0020-007E,U+00A0-024F,U+0370-03FF,U+0400-04FF,U+2010-2027,U+2030-205E,U+20AC,U+2122,U+2190-2193,U+2212,U+2500-257F,U+25A0-25FF"
hb-subset Inter-Regular.ttf --unicodes="$U" --layout-features='*' -o Inter-Regular.ttf.sub
```
