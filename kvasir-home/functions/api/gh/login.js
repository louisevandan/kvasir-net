/* Start GitHub OAuth: redirect to the authorize screen with a CSRF state.
   Requires env GITHUB_CLIENT_ID. If unconfigured, bounce back so the page
   fails open (soft gate). Cloudflare Pages Function → route /api/gh/login. */
export async function onRequestGet({ request, env }) {
  const url = new URL(request.url);
  const back = url.origin + "/docs/api";
  if (!env.GITHUB_CLIENT_ID) {
    return Response.redirect(back + "?gh=unconfigured", 302);
  }
  const state = crypto.randomUUID();
  const authorize = new URL("https://github.com/login/oauth/authorize");
  authorize.searchParams.set("client_id", env.GITHUB_CLIENT_ID);
  authorize.searchParams.set("redirect_uri", url.origin + "/api/gh/callback");
  authorize.searchParams.set("scope", "read:user");
  authorize.searchParams.set("state", state);
  const headers = new Headers({ Location: authorize.toString() });
  headers.append(
    "Set-Cookie",
    `kvr_gh_state=${state}; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=600`
  );
  return new Response(null, { status: 302, headers });
}
