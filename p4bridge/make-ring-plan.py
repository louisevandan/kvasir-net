#!/usr/bin/env python3
"""Write a p4 load plan for a two-node ring.

The ring's recovery depends on this file. It used to live in /tmp on one
machine, which meant losing that machine lost the procedure -- so it lives here
now, beside the template it reads.

    python3 make-ring-plan.py --site sites/gb10.json > plan.json
    P4_BRIDGE_CATALOG=... node load.mjs --plan plan.json --confirm

Every host-specific value is in the site file. The template supplies what was
measured rather than chosen: resource_profile (including the per-stage
max_physical_result_bytes, 137,935,244 for the head and 3,696,012 for the tail),
context sizes, batch sizes and timeouts. The head stage inherits the template's
first stage and the tail its last, because those are the two ends of the
pipeline whichever way it is split.
"""
import argparse
import json
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))


def load_generation(record, catalog):
    """A new generation, and a check that it really is new.

    The adapter compares load_generation for exact equality on every session,
    inference, settlement and UNLOAD. Reusing one strands the model: it refuses
    every session and cannot be unloaded at the number you have. The value is
    the wall clock in milliseconds, so the only way to collide is for the clock
    to go backwards -- which is what this checks, against every generation we
    still have a record of, rather than against a hand-kept list that goes
    stale the moment it is used.
    """
    generation = int(time.time() * 1000)
    seen = []
    for path in (record, catalog):
        if not path or not os.path.exists(path):
            continue
        try:
            with open(path, encoding='utf-8') as handle:
                data = json.load(handle)
        except (OSError, ValueError) as error:
            # Say so and carry on. A bookkeeping file that cannot be read is a
            # reason to look, not a reason to block a recovery -- the check it
            # feeds only guards against the clock going backwards.
            print(f'warning: could not read {path} ({error}); '
                  'the clock check is weaker for it', file=sys.stderr)
            continue
        if not isinstance(data, dict):
            print(f'warning: {path} is not an object; skipped', file=sys.stderr)
            continue
        for holder in [data, *data.get('models', [])]:
            if not isinstance(holder, dict):
                continue
            previous = holder.get('load_generation')
            # null means "not loaded" -- UNLOAD writes it that way. Reading it
            # as a number crashed the generator every time a plan was made
            # right after an unload, which is precisely the recovery sequence.
            # Found by GB10 #1 mid-recovery, 2026-09-25.
            if previous is None:
                continue
            try:
                seen.append((path, int(previous)))
            except (TypeError, ValueError):
                print(f'warning: {path} has a load_generation that is not a number '
                      f'({previous!r}); skipped', file=sys.stderr)
    for path, previous in seen:
        if generation <= previous:
            raise SystemExit(
                f'the clock is behind {path}: {generation} <= {previous}.\n'
                'Reusing or going below a live generation strands the model. Fix the clock.')
    return generation


def stage(template, site, node, agent_key, begin, end, n_layer):
    host = site[agent_key]
    off = [i for i in range(n_layer) if not begin <= i < end]
    built = dict(template)
    built.update({
        'agent': host['agent'],
        'node': node,
        'binary': host['binary'],
        'endpoint': host.get('endpoint', '127.0.0.1:42100'),
        'plan': (
            f'--model "{host["model"]}" {site["common_flags"]} '
            f'--layer-begin {begin} --layer-end {end} '
            f'--kv-layer-begin {begin} --kv-layer-end {end} '
            f'--override-tensor "blk\\.({"|".join(str(i) for i in off)})\\..*=CPU"'),
        'environment': [
            ['PATH', host.get('path', '/usr/local/cuda/bin:/usr/local/bin:/usr/bin:/bin')],
            ['LD_LIBRARY_PATH', f'{host["lib"]}:/usr/local/cuda/targets/sbsa-linux/lib'],
            ['CUDA_VISIBLE_DEVICES', host.get('cuda_visible_devices', '0')],
            ['OMP_WAIT_POLICY', 'PASSIVE'],
            ['GOMP_SPINCOUNT', '0'],
        ],
    })
    return built


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--site', required=True, help='host-specific values; see sites/gb10.json')
    parser.add_argument('--template', default=os.path.join(HERE, 'load-plan.step37.json'),
                        help='plan whose resource_profile and sizes are inherited')
    parser.add_argument('--record', default=None,
                        help='state/last-load.json, checked so the clock cannot go backwards')
    parser.add_argument('--catalog', default=os.environ.get('P4_BRIDGE_CATALOG'),
                        help='catalog.json, checked the same way')
    args = parser.parse_args()

    with open(args.site, encoding='utf-8') as handle:
        site = json.load(handle)
    with open(args.template, encoding='utf-8') as handle:
        template = json.load(handle)

    record = args.record or os.path.join(HERE, 'state', 'last-load.json')
    generation = load_generation(record, args.catalog)

    n_layer = template['model']['n_layer']
    split = site['split']
    if not 0 < split < n_layer:
        raise SystemExit(f'split {split} is not inside 0..{n_layer}')

    head = stage(template['stages'][0], site, site['head']['node'], 'head', 0, split, n_layer)
    tail = stage(template['stages'][-1], site, site['tail']['node'], 'tail', split, n_layer, n_layer)
    for built in (head, tail):
        built['generation'] = generation

    json.dump({
        'ingress_agent': site['head']['agent'],
        'load_generation': generation,
        'model': template['model'],
        'stages': [head, tail],
    }, sys.stdout, indent=2)
    sys.stdout.write('\n')
    print(f'load_generation {generation} · split {split} '
          f'· head {head["node"]}@{head["agent"]} · tail {tail["node"]}@{tail["agent"]}',
          file=sys.stderr)


if __name__ == '__main__':
    main()
