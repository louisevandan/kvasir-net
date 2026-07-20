#!/usr/bin/env python3
"""Sweep single-request decode tuning options against a linkcpp controller.

This benchmark intentionally keeps parallel=1. It tests options that can improve
per-user decode latency/TPS without relying on concurrent user throughput.
"""
import argparse
import csv
import json
import sys
import time
import urllib.error
import urllib.request


DEFAULT_PROMPT = "최신 자바에 대해 간략히 설명해"


def http_json(method, url, body=None, timeout=None):
    data = None
    headers = {}
    if body is not None:
        data = json.dumps(body, ensure_ascii=False).encode("utf-8")
        headers["content-type"] = "application/json"
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw = resp.read()
            return json.loads(raw.decode("utf-8")) if raw else {}
    except urllib.error.HTTPError as exc:
        detail = exc.read().decode("utf-8", "replace")
        raise RuntimeError(f"{method} {url} failed: HTTP {exc.code}: {detail}") from exc


def get_controller(base_url, cid):
    return http_json("GET", f"{base_url}/api/controllers/{cid}", timeout=10)


def wait_phase(base_url, cid, phases, timeout_s):
    deadline = time.time() + timeout_s
    last = None
    while time.time() < deadline:
        last = get_controller(base_url, cid)
        if last.get("phase") in phases:
            return last
        time.sleep(2)
    raise TimeoutError(f"controller {cid} did not reach {phases}; last={last}")


def wait_no_active_inference(base_url, cid, timeout_s=60):
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        activity = http_json("GET", f"{base_url}/api/controllers/{cid}/inference-activity", timeout=10)
        if not activity.get("active") and not activity.get("queued"):
            return
        time.sleep(1)
    raise TimeoutError(f"controller {cid} still has active/queued inference")


def load_variant(base_url, cid, load_req, timeout_s):
    ctrl = get_controller(base_url, cid)
    if ctrl.get("can_unload"):
        http_json("POST", f"{base_url}/api/controllers/{cid}/unload", {}, timeout=30)
        wait_phase(base_url, cid, {"idle"}, timeout_s=180)
    http_json("POST", f"{base_url}/api/controllers/{cid}/load", load_req, timeout=timeout_s)
    return wait_phase(base_url, cid, {"running"}, timeout_s=timeout_s)


def latest_history(base_url, cid):
    hist = http_json("GET", f"{base_url}/api/controllers/{cid}/inference-history?limit=1&detail=true", timeout=10)
    rows = hist.get("history") or []
    return rows[0] if rows else {}


def chat_once(base_url, cid, model, prompt, max_tokens, seed):
    body = {
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0,
        "stream": False,
        "max_tokens": max_tokens,
    }
    if seed is not None:
        body["seed"] = seed
    started = time.time()
    out = http_json("POST", f"{base_url}/c/{cid}/v1/chat/completions", body, timeout=None)
    elapsed = time.time() - started
    hist = latest_history(base_url, cid)
    detail = hist.get("detail") or {}
    metrics = detail.get("metrics") or hist.get("metrics") or {}
    timings = detail.get("timings") or {}
    return {
        "elapsed_s": round(elapsed, 3),
        "response": out,
        "history": hist,
        "metrics": metrics,
        "timings": timings,
    }


def variants(include_mtp=False, include_draft=False, draft_model=""):
    base = [
        ("baseline", {}),
        ("batch_512_ubatch_128", {"batch": 512, "ubatch": 128}),
        ("batch_1024_ubatch_256", {"batch": 1024, "ubatch": 256}),
        ("ngram_cache_n2", {"spec_type": "ngram-cache", "spec_draft_n_max": 2, "spec_draft_n_min": 1}),
        ("ngram_cache_n3", {"spec_type": "ngram-cache", "spec_draft_n_max": 3, "spec_draft_n_min": 1}),
        ("ngram_cache_n4", {"spec_type": "ngram-cache", "spec_draft_n_max": 4, "spec_draft_n_min": 1}),
        ("ngram_mod_2_5_2", {
            "spec_type": "ngram-mod",
            "spec_draft_n_max": 3,
            "spec_draft_n_min": 1,
            "spec_ngram_mod_n_min": 2,
            "spec_ngram_mod_n_max": 5,
            "spec_ngram_mod_n_match": 2,
        }),
        ("ngram_mod_3_8_2", {
            "spec_type": "ngram-mod",
            "spec_draft_n_max": 4,
            "spec_draft_n_min": 1,
            "spec_ngram_mod_n_min": 3,
            "spec_ngram_mod_n_max": 8,
            "spec_ngram_mod_n_match": 2,
        }),
        ("cache_reuse_256", {"cache_reuse": 256}),
    ]
    if include_mtp:
        base.append(("draft_mtp_n1", {"spec_type": "draft-mtp", "spec_draft_n_max": 1, "spec_draft_n_min": 1}))
        base.append(("draft_mtp_n2", {"spec_type": "draft-mtp", "spec_draft_n_max": 2, "spec_draft_n_min": 1}))
    if include_draft and draft_model:
        base.append(("draft_simple_n3", {
            "spec_type": "draft-simple",
            "spec_draft_model": draft_model,
            "spec_draft_n_max": 3,
            "spec_draft_n_min": 1,
        }))
    return base


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base-url", default="http://127.0.0.1:19000")
    ap.add_argument("--cid", required=True)
    ap.add_argument("--prompt", default=DEFAULT_PROMPT)
    ap.add_argument("--max-tokens", type=int, default=96)
    ap.add_argument("--seed", type=int, default=1234)
    ap.add_argument("--load-timeout", type=int, default=900)
    ap.add_argument("--out", default="")
    ap.add_argument("--only", default="", help="comma-separated variant names")
    ap.add_argument("--include-mtp", action="store_true")
    ap.add_argument("--include-draft", action="store_true")
    ap.add_argument("--draft-model", default="")
    args = ap.parse_args()

    ctrl = get_controller(args.base_url, args.cid)
    last_load = dict(ctrl.get("last_load") or {})
    if not last_load.get("model"):
        raise SystemExit("controller has no last_load model to benchmark")
    last_load["parallel"] = 1
    last_load.setdefault("ctx", ctrl.get("ctx") or 4096)
    last_load.setdefault("cache_type_k", "f16")
    last_load.setdefault("cache_type_v", "f16")
    selected = set(x.strip() for x in args.only.split(",") if x.strip())
    rows = []
    all_variants = variants(args.include_mtp, args.include_draft, args.draft_model)
    for name, perf in all_variants:
        if selected and name not in selected:
            continue
        load_req = dict(last_load)
        load_req.update(perf)
        load_req["parallel"] = 1
        print(f"== {name}: load {json.dumps(perf, ensure_ascii=False)}", flush=True)
        try:
            load_variant(args.base_url, args.cid, load_req, args.load_timeout)
            wait_no_active_inference(args.base_url, args.cid)
            result = chat_once(args.base_url, args.cid, load_req["model"], args.prompt, args.max_tokens, args.seed)
            metrics = result["metrics"]
            timings = result["timings"]
            row = {
                "variant": name,
                "status": "ok",
                "tps": metrics.get("tps"),
                "output_tokens": metrics.get("output_tokens"),
                "elapsed_s": result["elapsed_s"],
                "predicted_n": timings.get("predicted_n"),
                "predicted_ms": timings.get("predicted_ms"),
                "predicted_per_second": timings.get("predicted_per_second"),
                "prompt_per_second": timings.get("prompt_per_second"),
                "finish_reason": metrics.get("finish_reason"),
                "archive_name": result["history"].get("archive_name"),
                "error": "",
            }
        except Exception as exc:
            row = {"variant": name, "status": "error", "error": str(exc)}
        rows.append(row)
        print(json.dumps(row, ensure_ascii=False), flush=True)

    if args.out:
        fieldnames = [
            "variant", "status", "tps", "output_tokens", "elapsed_s", "predicted_n",
            "predicted_ms", "predicted_per_second", "prompt_per_second",
            "finish_reason", "archive_name", "error",
        ]
        with open(args.out, "w", newline="", encoding="utf-8") as f:
            writer = csv.DictWriter(f, fieldnames=fieldnames)
            writer.writeheader()
            writer.writerows(rows)
    return 0 if all(r.get("status") == "ok" for r in rows) else 1


if __name__ == "__main__":
    sys.exit(main())
