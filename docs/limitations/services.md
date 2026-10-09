# Unsupported services and collection limits

Collection supports public unauthenticated HTTP(S) GETs. Captures are static HTML
only: no JavaScript execution, browser session/cookies, login, CAPTCHA bypass,
private platform APIs or arbitrary request headers/bodies. Cross-host, port or
scheme redirects are refused. Responses are capped at 16 MiB with bounded timeouts.
PDF/image bytes may be preserved, but HTML is the only extractor in this slice.
DNS/RDAP, crt.sh, Wayback, paid services and third-party connectors are not wired.
Recording is a deliberate developer action; sanitized fixtures are never
committed automatically. The implementation refuses credential-bearing requests
and common secret patterns, but this is not proof that arbitrary bytes are safe
for publication: review every recording before adding it to a fixture corpus.
