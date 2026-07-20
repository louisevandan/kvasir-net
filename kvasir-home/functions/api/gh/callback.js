/* GitHub OAuth callback: verify CSRF state, exchange code → token, read the
   login and whether the user starred louisevandan/kvasir-net, then set an
   HMAC-signed session cookie and return to /docs/api. Route /api/gh/callback. */
import { signSession, parseCookies } from "./_util.js";

const REPO = "louisevandan/kvasir-net";

export async function onRequestGet({ request, env }) {
  const url = new URL(request.url);
  const back = url.origin + "/docs/api";
  const code = url.searchParams.get("code");
  const state = url.searchParams.get("state");
  const cookies = parseCookies(request);

  if (!code || !state || state !== cookies.kvr_gh_state) {
    return Response.redirect(back + "?gh=error", 302);
  }
  if (!env.GITHUB_CLIENT_ID || !env.GITHUB_CLIENT_SECRET || !env.SESSION_SECRET) {
    return Response.redirect(back + "?gh=unconfigured", 302);
  }

  // exchange the code for an access token
  const tok = await fetch("https://github.com/login/oauth/access_token", {
    method: "POST",
    headers: { "content-type": "application/json", accept: "application/json" },
    body: JSON.stringify({
      client_id: env.GITHUB_CLIENT_ID,
      client_secret: env.GITHUB_CLIENT_SECRET,
      code,
      redirect_uri: url.origin + "/api/gh/callback",
    }),
  })
    .then((r) => r.json())
    .catch(() => ({}));

  const access = tok.access_token;
  if (!access) return Response.redirect(back + "?gh=error", 302);

  const gh = {
    authorization: `Bearer ${access}`,
    "user-agent": "kvasir-star-gate",
    accept: "application/vnd.github+json",
  };
  const user = await fetch("https://api.github.com/user", { headers: gh })
    .then((r) => r.json())
    .catch(() => ({}));
  // 204 = starred, 404 = not starred
  const starRes = await fetch(`https://api.github.com/user/starred/${REPO}`, { headers: gh });
  const starred = starRes.status === 204;

  const session = await signSession(
    { login: user.login || null, starred, iat: Date.now() },
    env.SESSION_SECRET
  );
  const headers = new Headers({ Location: back + (starred ? "?gh=ok" : "?gh=nostar") });
  headers.append(
    "Set-Cookie",
    `kvr_gh=${session}; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=604800`
  );
  headers.append("Set-Cookie", "kvr_gh_state=; Path=/; Max-Age=0");
  return new Response(null, { status: 302, headers });
}
