# Bundled fonts

The native interface uses IBM Plex Sans Regular and SemiBold for text/headings and Lucide for icons. IBM Plex retains the SIL Open Font License 1.1 (`IBM-Plex-OFL.txt`); Lucide retains the ISC license and the included MIT notice for icons derived from Feather (`Lucide-LICENSE.txt`).

| File | SHA-256 |
| --- | --- |
| `IBMPlexSans-Regular.ttf` | `975dcda37d80f038dcd143c22e33ca2d97a0cc5a929aace1c749153b0fe1afa5` |
| `IBMPlexSans-SemiBold.ttf` | `a20caf8286023a6a7a85e40b1d2a4ae9fc3e3b1f9eda8f4c542dd4986af67bb1` |
| `lucide.ttf` | `0b98954b05aba6a770306609847f2d2194395ab225288346c7841a4a50ce2300` |

## CJK fallback

The unmodified Noto Sans CJK Regular collection is embedded in GUI builds; face 2 (Noto Sans CJK SC) is used after the normal egui fonts in proportional and monospace families. Fontconfig and separately installed system fonts are not needed at runtime.

© 2014-2021 Adobe (http://www.adobe.com/). See OFL.txt for the SIL Open Font License 1.1. The font retains its own license.

Provenance:

```json
{
  "upstream": "https://github.com/notofonts/noto-cjk",
  "package": "noto-fonts-cjk 20240730-1",
  "sha256": "b76b0433203017ca80401b2ee0dd69350349871c4b19d504c34dbdd80541690a",
  "bytes": 19484784,
  "face_index": 2,
  "family": "Noto Sans CJK SC",
  "license": "SIL Open Font License 1.1",
  "changes": "None; original collection copied verbatim"
}
```
