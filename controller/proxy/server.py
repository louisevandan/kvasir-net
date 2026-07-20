"""Thin HTTP adapter to the first-rank stock llama.cpp server."""

from __future__ import annotations

import time
from typing import Any

import httpx

from controller.proxy.protocol import StageInferRequest


async def infer(base_url: str, request: StageInferRequest) -> dict[str, Any]:
    started = time.perf_counter()
    timeout = httpx.Timeout(None, connect=10.0)
    async with httpx.AsyncClient(timeout=timeout) as client:
        if request.messages:
            response = await client.post(
                f"{base_url.rstrip('/')}/v1/chat/completions",
                json={"messages": request.messages, "max_tokens": request.max_tokens, "stream": False},
            )
        else:
            response = await client.post(
                f"{base_url.rstrip('/')}/completion",
                json={"prompt": request.prompt, "n_predict": request.max_tokens, "stream": False},
            )
    response.raise_for_status()
    body = response.json()
    if request.messages:
        choices = body.get("choices") or []
        text = str(((choices[0].get("message") or {}).get("content")) if choices else "")
        tokens = int((body.get("usage") or {}).get("completion_tokens") or 0)
    else:
        text = str(body.get("content") or "")
        tokens = int(body.get("tokens_predicted") or 0)
    return {
        "request_id": request.request_id,
        "text": text,
        "tokens": tokens,
        "elapsed_ms": int((time.perf_counter() - started) * 1000),
        "status": "done",
        "llama_server": body,
    }
