# SerenaDesktop Windows ownership patch

Upstream: `tauri-apps/plugins-workspace`, `plugins/single-instance`, registry release `2.4.4`.

Source pin: upstream revision `6aa2854f314481a459be1189b02c65a2450789ab`; crates.io checksum `5cd0cb5c412a5071b69bab6a6df1583cbb89460d4a83b6a24769b08d15b6b1e1`.

Reason: Windows mutex-to-event-HWND startup race can admit a second main process.
Related: upstream #3495, #3542, and merged foreground-rights PR #3592.

This local patch changes only the Windows backend: a secondary waits in bounded 50 ms slices for at most five seconds, takes ownership only after mutex release/abandonment, and otherwise fails closed. It is removed when an official released plugin provides equivalent bounded, fail-closed ownership semantics and SerenaDesktop P6-002 passes without it.
