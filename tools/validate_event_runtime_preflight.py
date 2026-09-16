#!/usr/bin/env python3
"""Reject a remote event-runtime run whose sealed preflight is incomplete."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


SNAPSHOT_CONTENT_TYPE = "application/vnd.p4.agent.snapshot-v1+json"


def _endpoint_port(endpoint: str) -> int:
    try:
        _, raw_port = endpoint.rsplit(":", 1)
        port = int(raw_port)
    except (AttributeError, TypeError, ValueError) as error:
        raise ValueError(f"invalid native endpoint: {endpoint!r}") from error
    if not 1 <= port <= 65535:
        raise ValueError(f"native endpoint port is outside 1..65535: {endpoint!r}")
    return port


def _address(value: Any, label: str) -> str:
    if not isinstance(value, list) or len(value) < 2 or value[0] != 0:
        raise ValueError(f"{label} is not an agent endpoint")
    return str(value[1])


def _route_address(value: Any, label: str) -> str:
    if not isinstance(value, list) or len(value) < 2 or value[0] != 2:
        raise ValueError(f"{label} is not an outer return endpoint")
    return str(value[1])


def validate(config: dict[str, Any], evidence: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    ingress = config.get("ingress_agent")
    if not isinstance(ingress, str) or not ingress:
        errors.append("config.ingress_agent is required")
    if evidence.get("schema") != 1:
        errors.append("evidence.schema must be 1")
    if evidence.get("ingress_agent") != ingress:
        errors.append("evidence ingress does not equal the sealed config ingress")

    nodes = config.get("nodes")
    if not isinstance(nodes, list) or not nodes:
        errors.append("config.nodes must be a nonempty list")
        return errors
    routes = evidence.get("routes")
    if not isinstance(routes, list):
        errors.append("evidence.routes must be a list")
        return errors

    by_agent: dict[str, dict[str, Any]] = {}
    for route in routes:
        if not isinstance(route, dict) or not isinstance(route.get("agent"), str):
            errors.append("every route needs an agent address")
            continue
        agent = route["agent"]
        if agent in by_agent:
            errors.append(f"duplicate route evidence for {agent}")
        by_agent[agent] = route

    endpoints: set[tuple[str, str]] = set()
    configured_agents: set[str] = set()
    for index, node in enumerate(nodes):
        if not isinstance(node, dict):
            errors.append(f"node[{index}] is not an object")
            continue
        agent = node.get("agent")
        endpoint = node.get("endpoint")
        if not isinstance(agent, str):
            errors.append(f"node[{index}].agent is required")
            continue
        configured_agents.add(agent)
        route = by_agent.get(agent)
        if route is None:
            errors.append(f"missing exact roundtrip evidence for {agent}")
            continue
        try:
            port = _endpoint_port(endpoint)
        except ValueError as error:
            errors.append(str(error))
            continue
        endpoint_key = (agent, str(endpoint))
        if endpoint_key in endpoints:
            errors.append(f"duplicate native endpoint on {agent}: {endpoint}")
        endpoints.add(endpoint_key)

        ranges = route.get("dynamic_port_ranges")
        if not isinstance(ranges, list) or not ranges:
            errors.append(f"missing OS dynamic port ranges for {agent}")
        else:
            for item in ranges:
                if not isinstance(item, dict):
                    errors.append(f"invalid dynamic port range for {agent}")
                    continue
                first, last = item.get("first"), item.get("last")
                if not isinstance(first, int) or not isinstance(last, int) or first > last:
                    errors.append(f"invalid dynamic port range for {agent}: {item!r}")
                elif first <= port <= last:
                    errors.append(
                        f"native endpoint {endpoint} for {agent} overlaps "
                        f"OS dynamic port range {first}..{last}"
                    )

    for agent in sorted(configured_agents):
        route = by_agent.get(agent)
        if route is None:
            continue
        try:
            if _address(route.get("request_target"), "request_target") != agent:
                errors.append(f"route request target does not equal {agent}")
            if _address(route.get("reply_source"), "reply_source") != agent:
                errors.append(f"route reply source does not equal {agent}")
            if _route_address(route.get("reply_target"), "reply_target") != ingress:
                errors.append(f"reply target for {agent} does not return to {ingress}")
            if _route_address(route.get("reply_return_route"), "reply_return_route") != ingress:
                errors.append(f"reply return route for {agent} does not equal {ingress}")
        except ValueError as error:
            errors.append(f"{agent}: {error}")
        if route.get("content") != SNAPSHOT_CONTENT_TYPE:
            errors.append(f"route probe for {agent} did not return an agent snapshot")
        if route.get("nodes") != []:
            errors.append(f"agent {agent} is not empty before the run")
        if route.get("transport_failure_count") != 0:
            errors.append(f"agent {agent} has retained transport failures")
        if route.get("task_owned_native_processes") != 0:
            errors.append(f"agent {agent} has task-owned native children")
        if route.get("task_owned_native_listeners") != 0:
            errors.append(f"agent {agent} has task-owned native listeners")
        if route.get("task_owned_agent_close_wait_connections") != 0:
            errors.append(f"agent {agent} has retained CLOSE_WAIT TCP connections")

    extra = sorted(set(by_agent) - configured_agents)
    if extra:
        errors.append(f"evidence contains unconfigured agents: {', '.join(extra)}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--evidence", required=True, type=Path)
    args = parser.parse_args()
    config = json.loads(args.config.read_text(encoding="utf-8"))
    evidence = json.loads(args.evidence.read_text(encoding="utf-8"))
    errors = validate(config, evidence)
    print(json.dumps({"passed": not errors, "errors": errors}, ensure_ascii=False))
    return 0 if not errors else 1


if __name__ == "__main__":
    raise SystemExit(main())
