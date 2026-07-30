"""Client + reverse proxy for the linker control plane.

The gateway (this app) owns authentication, the public API surface, and KVR
settlement. Everything about nodes, controllers, planning, model loading and
runtime state lives in the *linker* service, which is consumed strictly over
its REST/WebSocket API and is never modified.

Trust boundary
--------------
The browser authenticates against the gateway with a `linkcpp_session` cookie
(issued by `controller/siws.py`). The gateway then calls linker with a shared
**service token**, which linker's auth middleware checks *before* any session
lookup. That ordering is what lets the two services keep incompatible session
signing schemes (Python `siws` vs linker's Node `AuthTokens`) — a browser
session is never presented to linker, so it never has to verify one.

Consequently linker must not be reachable from outside; the gateway is the
only client that should ever hold the service token.

Serving linker's UI
-------------------
Linker ships a single-page app whose assets are absolute (`/assets/...`) and
which carries no client-side router. Two properties make it embeddable without
touching linker:

* the gateway serves its own UI under `/web/` and never uses `/assets/`, so
  that prefix can be proxied straight through with no HTML rewriting;
* with no router, the app never rewrites the URL, so there is no history
  fallback to emulate.

Its API calls are absolute `/api/...`, which resolve against the gateway and
are delegated back here. The one moving part is `/api/events`, a WebSocket the
app opens against `window.location.host`; `relay_websocket` bridges it.
"""

import os
from urllib.parse import urlencode

import httpx

# Base URL of the linker service. Compose puts it on the project network as
# `linker`; a bare host install typically reaches it on localhost.
LINKER_URL = os.environ.get("LINKCPP_LINKER_URL", "http://linker:19001").rstrip("/")
# Shared secret presented to linker as `x-linkcpp-service-token`. Must match the
# value linker was started with, otherwise every delegated call 401s.
LINKER_SERVICE_TOKEN = os.environ.get(
    "LINKCPP_LINKER_SERVICE_TOKEN",
    os.environ.get("LINKCPP_UNIT_SERVICE_TOKEN", ""),
).strip()

# Model loads and plan runs hold the connection for minutes; only the connect
# phase should fail fast. Reads are unbounded because a `serve` call streams
# progress until the runtime is up. Built lazily rather than at import, so the
# module stays importable when httpx is stubbed out (see tests/unit).
def _timeout():
    return httpx.Timeout(connect=10.0, read=None, write=None, pool=10.0)

# Headers that describe a single transport hop and must not be relayed, plus
# the ones the proxy re-derives itself.
_DROP_REQUEST_HEADERS = frozenset({
    "connection", "keep-alive", "proxy-authenticate", "proxy-authorization",
    "te", "trailer", "transfer-encoding", "upgrade",
    # Rewritten per-hop or supplied by httpx.
    "host", "content-length",
    # The gateway is the only authenticator: a browser session must not leak
    # into linker, and any inbound bearer is replaced by the service token.
    "cookie", "authorization", "x-linkcpp-service-token",
})
_DROP_RESPONSE_HEADERS = frozenset({
    "connection", "keep-alive", "proxy-authenticate", "proxy-authorization",
    "te", "trailer", "transfer-encoding", "upgrade",
    # httpx already decoded the body; forwarding these would misdescribe it.
    "content-encoding", "content-length",
})

_client = None


def client():
    """Process-wide pooled client. Created lazily so importing this module has
    no side effects (tests import it without a running linker)."""
    global _client
    if _client is None:
        _client = httpx.AsyncClient(base_url=LINKER_URL, timeout=_timeout(),
                                    follow_redirects=False)
    return _client


async def aclose():
    global _client
    if _client is not None:
        await _client.aclose()
        _client = None


def service_headers(extra=None):
    """Headers that authenticate the gateway to linker."""
    headers = {}
    if LINKER_SERVICE_TOKEN:
        headers["x-linkcpp-service-token"] = LINKER_SERVICE_TOKEN
    if extra:
        headers.update(extra)
    return headers


async def call(method, path, *, params=None, json=None, headers=None):
    """Issue one delegated API call and return the parsed JSON body.

    Raises httpx.HTTPStatusError on a non-2xx response so callers can map
    linker's status onto their own.
    """
    response = await client().request(
        method, path if path.startswith("/") else "/" + path,
        params=params, json=json, headers=service_headers(headers),
    )
    response.raise_for_status()
    if not response.content:
        return None
    return response.json()


def _forward_request_headers(request):
    out = {
        key: value for key, value in request.headers.items()
        if key.lower() not in _DROP_REQUEST_HEADERS
    }
    # Preserve the client's view of the origin so linker-generated absolute
    # URLs (endpoint hints, report callbacks) point back at the gateway.
    out["x-forwarded-host"] = request.headers.get("host", "")
    out["x-forwarded-proto"] = request.url.scheme
    return out


def _forward_response_headers(response):
    return {
        key: value for key, value in response.headers.items()
        if key.lower() not in _DROP_RESPONSE_HEADERS
    }


async def proxy(request, path=None):
    """Relay a Starlette request to linker and stream the response back.

    Streaming rather than buffering matters for two cases: SSE token streams
    from the inference gateway, and multi-hundred-megabyte model/shard
    downloads that nodes pull through this path.
    """
    # Imported here so the module stays importable without FastAPI installed.
    from fastapi.responses import StreamingResponse

    target = path if path is not None else request.url.path
    if not target.startswith("/"):
        target = "/" + target
    if request.url.query:
        target = f"{target}?{request.url.query}"

    upstream = client().build_request(
        request.method, target,
        headers=_forward_request_headers(request),
        content=request.stream(),
    )
    response = await client().send(upstream, stream=True)

    async def body():
        try:
            async for chunk in response.aiter_raw():
                yield chunk
        finally:
            await response.aclose()

    return StreamingResponse(
        body(),
        status_code=response.status_code,
        headers=_forward_response_headers(response),
        media_type=response.headers.get("content-type"),
    )


def websocket_url(path, params=None):
    scheme = "wss" if LINKER_URL.startswith("https://") else "ws"
    authority = LINKER_URL.split("://", 1)[-1]
    query = f"?{urlencode(params)}" if params else ""
    return f"{scheme}://{authority}{path}{query}"


async def relay_websocket(ws, path, params=None):
    """Bridge an accepted client WebSocket to the matching socket on linker.

    Both directions run concurrently and either side closing tears the pair
    down, so a linker restart does not strand browser sockets.
    """
    import asyncio
    import inspect

    import websockets

    await ws.accept()
    # websockets renamed this argument in 14.0; support both so the image is
    # not pinned to one release line.
    header_kwarg = (
        "additional_headers"
        if "additional_headers" in inspect.signature(websockets.connect).parameters
        else "extra_headers"
    )
    try:
        async with websockets.connect(
            websocket_url(path, params),
            open_timeout=10,
            ping_interval=20,
            **{header_kwarg: service_headers()},
        ) as upstream:

            async def to_upstream():
                while True:
                    message = await ws.receive()
                    kind = message.get("type")
                    if kind == "websocket.disconnect":
                        return
                    if message.get("text") is not None:
                        await upstream.send(message["text"])
                    elif message.get("bytes") is not None:
                        await upstream.send(message["bytes"])

            async def to_client():
                async for message in upstream:
                    if isinstance(message, bytes):
                        await ws.send_bytes(message)
                    else:
                        await ws.send_text(message)

            done, pending = await asyncio.wait(
                {asyncio.create_task(to_upstream()), asyncio.create_task(to_client())},
                return_when=asyncio.FIRST_COMPLETED,
            )
            for task in pending:
                task.cancel()
            for task in done:
                task.result()
    except Exception:
        # The client socket is already accepted; closing is the only signal we
        # can still give it once the upstream leg fails.
        pass
    finally:
        try:
            await ws.close()
        except Exception:
            pass
