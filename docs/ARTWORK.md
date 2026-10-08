# ClipForge artwork

Generated with the built-in imagegen tool on October 8, 2026. The master is
[`icons/clipforge-source.png`](../icons/clipforge-source.png).

Final prompt:

> Use case: logo-brand. Create a polished square desktop app icon for ClipForge,
> a clipboard manager with named snippet registers. A bold, simple clipboard
> silhouette combined with a small forge spark, strong readable geometry at tray
> icon scale, dark charcoal and luminous mint green matching a dark productivity
> app. Centered rounded-square icon tile, generous clean margin, crisp edges,
> subtle depth, no letters, no words, no watermark. One icon only.

`scripts/build-icons.ps1` resizes the master into a 128 px tray PNG, a 256 px
README PNG, and a Windows ICO with 16, 24, 32, 48, 64, 128, and 256 px frames.
The original generated image is retained unchanged.

`scripts/capture-docs.cjs` captures the actual `ui/index.html`, `ui/styles.css`,
and `ui/app.js` in headless Chrome at the configured 840 × 650 window size and
2× pixel density. A demo IPC bridge supplies sample data. The captures illustrate
frontend usage; they do not validate native WebView rendering, clipboard access,
focus restoration, or keyboard injection.
