/**
 * Make outbound HTTPS work on these hosts. Import for the side effect.
 *
 *     import './net.mjs';
 *
 * The fleet has no IPv6 default route, but DNS answers with an AAAA record for
 * most things worth calling — api.telegram.org, api.github.com, Google's
 * calendar. Node picks that address and the connection dies as `ENETUNREACH`,
 * surfacing as a bare `fetch failed` with no status, while curl succeeds every
 * time because it tries both families. The failure is intermittent enough to
 * read as "the other service is flaky".
 *
 * This lived inline in the two modules that had already been bitten. Three
 * others had not been bitten *yet* — the commit watcher, the calendar, and the
 * health probes in the collector — and one of those probes reports the
 * settlement gateway, whose deliberate outage looks identical to this bug. A
 * guard that only some callers remember to apply is a guard that eventually
 * gets forgotten, so it lives in one file now and every caller imports it.
 */
import { setDefaultResultOrder } from 'node:dns';
import net from 'node:net';

try { setDefaultResultOrder('ipv4first'); } catch { /* older runtimes */ }
// And if a v6 address is chosen anyway, fall back rather than fail.
try { net.setDefaultAutoSelectFamily(true); } catch { /* older runtimes */ }
