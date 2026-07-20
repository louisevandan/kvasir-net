/* Report gate status to the client. `configured` is false until the OAuth env
   vars are set — the frontend then fails open (no gate). Route /api/gh/status. */
import { verifySession, parseCookies } from "./_util.js";

export async function onRequestGet({ request, env }) {
  const configured = !!(env.GITHUB_CLIENT_ID && env.GITHUB_CLIENT_SECRET && env.SESSION_SECRET);
  const cookies = parseCookies(request);
  const sess = configured ? await verifySession(cookies.kvr_gh, env.SESSION_SECRET) : null;
  return new Response(
    JSON.stringify({
      configured,
      authed: !!sess,
      login: sess?.login || null,
      starred: !!sess?.starred,
    }),
    { headers: { "content-type": "application/json", "cache-control": "no-store" } }
  );
}
