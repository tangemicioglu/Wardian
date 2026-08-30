# Remote PWA cache recovery

The remote service worker cache is versioned with the short source revision
injected during the Vite production build. Activation deletes older shell
caches, so a client holding the legacy `wardian-remote-app-shell-v1` cache
automatically moves to the current shell on its next load.
This requires no manual site-data deletion from the user.

Navigation requests are bounded by a five-second timeout and fall back to the
cached `/remote` shell. Shell precaching is best effort per URL, so one missing
optional asset cannot prevent the service worker from activating. Runtime asset
requests remain network-first and cache successful responses for later reuse.

The shell intentionally does not use stale-while-revalidate in this change:
versioned activation provides deterministic deploy invalidation, while the
navigation timeout addresses the indefinite-spinner failure directly.
