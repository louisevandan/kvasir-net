/* Clear the star-gate session. Route /api/gh/logout. */
export async function onRequestGet({ request }) {
  const url = new URL(request.url);
  const headers = new Headers({ Location: url.origin + "/docs/api" });
  headers.append("Set-Cookie", "kvr_gh=; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=0");
  return new Response(null, { status: 302, headers });
}
