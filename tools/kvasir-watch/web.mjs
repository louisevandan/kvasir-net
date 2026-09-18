/**
 * Looking outside our own files.
 *
 * Asked "can you research for that?", the bot said no, and it was right: it
 * could read the repository and nothing else. This is the part that goes out
 * and finds material we have not written.
 *
 * ## Everything out there is untrusted input
 *
 * The repository is ours. A web page is not. It can contain a sentence shaped
 * like an order to this bot, placed there on purpose or by accident, and the
 * model reading it cannot tell the difference between "the page says X" and
 * "the page tells me to do X" unless we make that difference structural.
 *
 * So fetched text is fenced and labelled as untrusted throughout, the bot has
 * no tools to obey an instruction with even if it wanted to, and every claim it
 * repeats carries the URL it came from — so a reader can check whether the
 * source is worth anything. That last part is not decoration. Search results
 * are full of confident pages written by nobody.
 *
 * ## Bounded because the budget is finite, not because the web is
 *
 * A few queries, a few pages, a few seconds each, a cap on bytes. These are
 * there so one question cannot spend the whole run; they are not a judgement
 * about which parts of the web are acceptable.
 */
import './net.mjs';

const UA = 'Mozilla/5.0 (compatible; kvasir-watch/1.0; +https://kvasir-ai.net)';

/** Strip markup down to the words. */
function toText(html) {
  return html
    .replace(/<script[\s\S]*?<\/script>/gi, ' ')
    .replace(/<style[\s\S]*?<\/style>/gi, ' ')
    .replace(/<noscript[\s\S]*?<\/noscript>/gi, ' ')
    .replace(/<\/(p|div|li|h[1-6]|tr|section|article)>/gi, '\n')
    .replace(/<[^>]+>/g, ' ')
    .replace(/&nbsp;/g, ' ').replace(/&amp;/g, '&').replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&#39;/g, "'")
    .replace(/&#x27;/g, "'").replace(/&hellip;/g, '…')
    .replace(/[ \t ]+/g, ' ')
    .replace(/\n\s*\n\s*\n+/g, '\n\n')
    .trim();
}

/**
 * Search, through Tavily.
 *
 * One provider, deliberately. The first version scraped DuckDuckGo's HTML
 * because it needed no account, and it behaved exactly as scraping a search
 * engine does: the first query returned ten results and the next returned an
 * anti-bot page. A research tool that works until it is used is worse than one
 * that says it needs a key.
 *
 * Tavily is built for this — it returns page text alongside the links, so most
 * questions need no second round of fetching, and results come back ranked for
 * a reading agent rather than for a person scanning a page of adverts.
 *
 * Needs TAVILY_API_KEY. Without it, searching fails with that sentence rather
 * than quietly returning nothing, because "no key" and "nothing found" are
 * different answers.
 */
export async function search(query, { limit = 5, timeoutMs = 25_000, depth = 'advanced' } = {}) {
  const key = (process.env.TAVILY_API_KEY ?? '').trim();
  if (!key) throw new Error('TAVILY_API_KEY is not set, so there is nothing to search with');

  const response = await fetch('https://api.tavily.com/search', {
    method: 'POST',
    headers: { 'content-type': 'application/json', authorization: `Bearer ${key}` },
    body: JSON.stringify({
      // Sent in the body as well: the older API took the key here, and an
      // account on either revision then works without a code change.
      api_key: key,
      query,
      max_results: limit,
      search_depth: depth,
      // The page text, so a useful answer does not depend on fetching each
      // result separately and hoping it is not behind a wall.
      include_raw_content: true,
      include_answer: false,
    }),
    signal: AbortSignal.timeout(timeoutMs),
  });
  if (!response.ok) {
    const detail = await response.text().catch(() => '');
    throw new Error(`Tavily returned ${response.status}${detail ? `: ${detail.slice(0, 200)}` : ''}`);
  }
  const body = await response.json();
  return (body.results ?? []).slice(0, limit).map((result) => ({
    title: String(result.title ?? '').slice(0, 160),
    url: String(result.url ?? ''),
    snippet: String(result.content ?? '').slice(0, 320),
    body: result.raw_content ? toText(String(result.raw_content)) : '',
  })).filter((result) => result.url);
}

/** One page, as plain text, with a ceiling. */
export async function readPage(url, { maxChars = 6000, timeoutMs = 15_000 } = {}) {
  const response = await fetch(url, {
    headers: { 'user-agent': UA, accept: 'text/html,text/plain' },
    signal: AbortSignal.timeout(timeoutMs),
    redirect: 'follow',
  });
  if (!response.ok) throw new Error(`${response.status}`);
  const type = response.headers.get('content-type') ?? '';
  if (!/text\/html|text\/plain|application\/xhtml/i.test(type)) throw new Error(`not text (${type.split(';')[0]})`);
  const html = await response.text();
  const text = toText(html);
  if (text.length < 200) throw new Error('almost no readable text');
  return text.slice(0, maxChars);
}

/**
 * Run the searches and read what comes back.
 *
 * @param {string[]} queries
 * @returns {Promise<{text: string, sources: {title: string, url: string}[]}>}
 */
export async function gather(queries, { pages = 3, perPage = 5000 } = {}) {
  const found = [];
  const problems = [];
  for (const query of queries.slice(0, 3)) {
    try { found.push(...await search(query, { limit: 5 })); }
    catch (error) { problems.push(`"${query}" — ${error.message}`); console.error(`web: ${problems.at(-1)}`); }
  }
  if (!found.length) {
    return { text: problems.length ? `The search could not be run: ${problems[0]}` : '', sources: [] };
  }

  // De-duplicate by host as well as by URL: five pages from one content farm
  // are one source wearing five hats.
  const byUrl = new Map();
  const hosts = new Set();
  for (const hit of found) {
    let host;
    try { host = new URL(hit.url).host; } catch { continue; }
    if (byUrl.has(hit.url) || hosts.has(host)) continue;
    byUrl.set(hit.url, hit);
    hosts.add(host);
  }

  const parts = [];
  const sources = [];
  for (const hit of [...byUrl.values()].slice(0, pages)) {
    let body = hit.body?.slice(0, perPage) ?? '';
    if (body.length < 200) {
      // Tavily had no text for this one; go and read it.
      try { body = await readPage(hit.url, { maxChars: perPage }); }
      catch (error) { body = `(could not be read: ${error.message})\n${hit.snippet}`; }
    }
    parts.push(`\n--- ${hit.title || hit.url}\n${hit.url}\n${body}`);
    sources.push({ title: hit.title || hit.url, url: hit.url });
  }
  return { text: parts.join('\n'), sources };
}
