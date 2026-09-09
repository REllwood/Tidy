# Tidy visual identity

The folded-paper T brings the app icon, sidebar and repository branding together. The interface uses evergreen accents, warm neutral surfaces, Geist for controls and a system serif for the home greeting.

`tidy-app-icon.png` is the original generated artwork. `tidy-mark.png` is the compact web export. Native application sizes are stored in `src-tauri/icons`; the sidebar uses `public/tidy-app-icon.png`. The existing SVG entry points embed the same artwork so repository and website references remain valid.

Both colour schemes share semantic design tokens in `src/index.css`. Recording meters use actual audio levels on a decibel scale; synthetic levels are confined to the browser preview.
