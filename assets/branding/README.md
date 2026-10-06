# aTerminal app icon

Selected concept **01 · A / Command Gate**: cyan A and white terminal prompt on navy.
`app-icon.svg` is the editable, font-independent full-bleed master. Geometry and
colors preserve the selected concept; platform masking replaces its preview-only
rounded background.

## Regenerate / check

Install Inkscape and Pillow (export tool versions used: Inkscape 1.4, Pillow 12.3.0;
minor renderer differences may alter PNG bytes). From the repository root:

```sh
python3 scripts/generate-app-icons.py
python3 scripts/check-app-icons.py
```

Generated resources are committed; builds do not need Inkscape or Pillow.

- Android API 25: five-density legacy square/round RGBA PNGs
- Android API 26+: adaptive background and vector foreground, scaled to fit the
  centered 66/108 safe circle; the system supplies the outer mask
- Android API 33+: monochrome foreground for themed launchers
- iOS: opaque, full-bleed RGB PNGs for iPhone, iPad and App Store, registered in
  the Xcode project and project generator; the native build probe compiles the
  same catalog and merges actool's icon metadata into its Info.plist

`Verify App Icons` checks resource coverage and runs Apple's actool and Android's
aapt2 compile/link. Existing native build CI remains responsible for full builds.

Desktop currently ships a CLI binary archive with no `.app`, desktop launcher or
Windows GUI packaging; no unrelated desktop wrapper is introduced. The SVG is
available for future supported desktop packaging.
