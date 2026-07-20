"""Runtime-neutral inference gateway dispatch."""

from __future__ import annotations

import json

import httpx

from controller.runtimes import chat_completion, stream_chat_completion
from controller.runtimes.base import LLAMA_RPC, normalize_runtime_mode


def selected_mode(controller) -> str:
    return normalize_runtime_mode((controller.get("plan") or {}).get("runtime_mode"))


async def chat(controller, payload, request_id=None):
    mode = selected_mode(controller)
    if mode != LLAMA_RPC:
        return await chat_completion(mode, controller, payload, request_id=request_id)
    async with httpx.AsyncClient(timeout=None) as client:
        response = await client.post(
            f"http://127.0.0.1:{controller['master_port']}/v1/chat/completions",
            json=payload,
        )
        response.raise_for_status()
        return response.json()


async def stream_chat(controller, payload, request_id=None):
    if selected_mode(controller) == LLAMA_RPC:
        async with httpx.AsyncClient(timeout=None) as client:
            async with client.stream(
                "POST",
                f"http://127.0.0.1:{controller['master_port']}/v1/chat/completions",
                json=payload,
            ) as response:
                response.raise_for_status()
                async for line in response.aiter_raw():
                    yield line
        return
    # Ring/proxy mode: relay real token-by-token SSE from the coordinator's
    # native llama-server endpoint (tools + streaming), via the first stage.
    async for chunk in stream_chat_completion(
        selected_mode(controller), controller, payload, request_id=request_id
    ):
        yield chunk
