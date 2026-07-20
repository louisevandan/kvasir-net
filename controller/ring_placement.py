"""Layer-boundary candidates for all-rank ring placement.

Ring rank order and layer boundaries are separate decisions.  The planner can
search every boundary composition for small topologies, while larger searches
stay bounded and report that limitation instead of claiming global
infeasibility.
"""

from __future__ import annotations

from itertools import combinations
from math import comb


MAX_EXHAUSTIVE_COMPOSITIONS = 10_000


def _targets_from_cuts(total_layers: int, cuts: tuple[int, ...]) -> tuple[int, ...]:
    points = (0, *cuts, total_layers)
    return tuple(points[index + 1] - points[index] for index in range(len(points) - 1))


def _cuts_from_targets(targets: tuple[int, ...]) -> tuple[int, ...]:
    cuts = []
    offset = 0
    for target in targets[:-1]:
        offset += target
        cuts.append(offset)
    return tuple(cuts)


def boundary_candidates(
    total_layers: int,
    rank_count: int,
    seed_targets,
    *,
    max_exhaustive: int = MAX_EXHAUSTIVE_COMPOSITIONS,
):
    """Return ``(targets, exhaustive)`` for one fixed ring rank order.

    Every positive contiguous composition is searched when the state space is
    small enough.  For larger topologies the VRAM/compute seed, a balanced
    split, and every one-boundary movement around those seeds are searched.
    """
    if rank_count <= 0 or total_layers < rank_count:
        return [], True
    if rank_count == 1:
        return [(total_layers,)], True

    seed = tuple(int(value) for value in seed_targets)
    if len(seed) != rank_count or sum(seed) != total_layers or any(value <= 0 for value in seed):
        raise ValueError("ring boundary seed must be a positive composition of all layers")

    composition_count = comb(total_layers - 1, rank_count - 1)
    if composition_count <= max_exhaustive:
        candidates = [seed]
        seen = {seed}
        for cuts in combinations(range(1, total_layers), rank_count - 1):
            targets = _targets_from_cuts(total_layers, cuts)
            if targets not in seen:
                seen.add(targets)
                candidates.append(targets)
        return candidates, True

    balanced_base, remainder = divmod(total_layers, rank_count)
    balanced = tuple(
        balanced_base + (1 if index < remainder else 0)
        for index in range(rank_count)
    )
    candidates = []
    seen = set()

    def add(targets):
        targets = tuple(targets)
        if targets not in seen:
            seen.add(targets)
            candidates.append(targets)

    for base in (seed, balanced):
        add(base)
        base_cuts = _cuts_from_targets(base)
        for boundary_index in range(rank_count - 1):
            lower = 1 if boundary_index == 0 else base_cuts[boundary_index - 1] + 1
            upper = total_layers if boundary_index == rank_count - 2 else base_cuts[boundary_index + 1]
            for cut in range(lower, upper):
                cuts = list(base_cuts)
                cuts[boundary_index] = cut
                add(_targets_from_cuts(total_layers, tuple(cuts)))
    return candidates, False
