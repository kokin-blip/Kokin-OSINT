# Optional local web discovery

Install Docker Desktop separately, then run from this directory:

```sh
docker compose up -d
docker compose logs --tail=30
```

In Kokin enable online mode, enter a name/topic query under Search → Faces and
related evidence, review the disclosure, and choose Search web. No attachment
or face embedding is transmitted. SearXNG forwards text queries to upstream
search engines. Those engines can rate-limit, fail, or return no results.

The service binds only to `127.0.0.1:8088`. JSON output is enabled. The image is
pinned by registry manifest digest; updates require reviewing and replacing it.
The static key is solely for this local configuration. Do not expose this setup
as a public service. Health checks perform a real disclosed text search.

Stop with `docker compose down`. Kokin never installs Docker or starts containers
silently. Browser search remains available when Docker is absent.

Search responses are captured as evidence. Opening a result in the browser does
not collect it; use the normal Collect URL workflow to preserve a selected page.
