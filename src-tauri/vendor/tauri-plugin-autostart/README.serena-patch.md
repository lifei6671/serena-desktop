# SerenaDesktop Windows autostart patch

Upstream: `tauri-apps/plugins-workspace`, `plugins/autostart`, registry release `2.5.1`.

Source pin: upstream revision `e7a68fa63755603b9fa12d28e077eea645551d24`; crates.io checksum `459383cebc193cdd03d1ba4acc40f2c408a7abce419d64bdcd2d745bc2886f70`.

Reason: the upstream Windows backend writes the current executable path verbatim into the Run value. An installed path such as `...\\Serena Desktop\\serena-desktop.exe` therefore lacks required executable quoting.

This local patch changes only the Windows plugin setup path to pass a quoted executable to the existing `auto-launch` backend. It is removed when an official released plugin writes a correctly quoted Windows Run target and SerenaDesktop P6-004 passes without this patch.
