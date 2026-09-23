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

/**
 * The line that actually fixes it.
 *
 * Node races the address families (Happy Eyeballs) and gives the first one
 * 250 ms to connect before starting the second. From here the IPv4 handshake to
 * api.telegram.org takes 700-800 ms, so it never wins that race: at 250 ms Node
 * starts the IPv6 attempt, that address has no route, and the whole connection
 * fails at about 0.3 s reporting ETIMEDOUT.
 *
 * Measured, first request of a fresh process, six runs each:
 *   250 ms attempt timeout — 2 of 6 failed, always at ~0.3 s
 *   5000 ms attempt timeout — 6 of 6 succeeded, all at ~0.8 s
 *
 * Only the first request is exposed, because undici pools the connection
 * afterwards. That is what made this so hard to see: a run fails on its opening
 * call and then behaves perfectly, which reads as "the other service is flaky"
 * rather than as a setting on our side.
 *
 * Ordering v4 first is not enough on its own — it decides which address is
 * tried first, not how long it is given.
 */
try { net.setDefaultAutoSelectFamilyAttemptTimeout(5000); } catch { /* older runtimes */ }
