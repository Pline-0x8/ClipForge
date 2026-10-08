# ClipForge artwork

Generated with the built-in imagegen tool on October 8, 2026. The master is
[`icons/clipforge-source.png`](../icons/clipforge-source.png).

Final prompt:

> Use case: logo-brand. Design an original distinctive desktop app icon for
> ClipForge, a clipboard snippet manager. One single sculptural continuous folded
> mint ribbon forming an angular open C, with a clever interlocking inner fold
> suggesting a paper clip and stacked snippets through negative space. Geometric
> custom brand mark, sophisticated precise silhouette, bold enough at 16 pixels,
> front view, centered within a dark midnight navy rounded-square tile. Restrained
> two-tone mint and jade, crisp near-flat edges with very subtle dimensional
> shading. No clipboard pictogram, no anvil, no spark, no emoji, no text, no letters
> printed, no objects combined, no decorative glow, no mockup, no watermark.
> Square composition, the mark occupies 70 percent of tile.

`scripts/build-icons.ps1` resizes the master into a 128 px tray PNG, a 256 px
README PNG, and a Windows ICO with 16, 24, 32, 48, 64, 128, and 256 px frames.
The original generated image is retained unchanged.

`scripts/capture-docs.cjs` captures the actual `ui/index.html`, `ui/styles.css`,
and `ui/app.js` in headless Chrome at the configured 840 × 650 window size and
2× pixel density. A demo IPC bridge supplies sample data. The captures illustrate
frontend usage; they do not validate native WebView rendering, clipboard access,
focus restoration, or keyboard injection.
