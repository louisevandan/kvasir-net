/* ==========================================================================
   Code examples for the /docs/api page. Language-agnostic (NOT i18n) — only
   the surrounding prose is translated. Two families of snippets:
   • SNIPPETS         — consume the paid inference API (quote → pay → redeem).
   • NODE_SETUP_SNIPPETS — bring up your OWN hub + gateway + node and get a free
                          local OpenAI-compatible endpoint (your hardware).
   Languages: TS/Node.js, Python, Java, Rust, Go.

   Ground truth: wallet/desktop/src/{services,browserWallet}.ts for the pay
   contract; docker-compose.yml (hub → localhost:19000) and controller/hub.py
   (/c/{cid}/v1/chat/completions) for the self-host path. Values verified
   against gate.kvasir-ai.net (2026-07-16, devnet).
   ========================================================================== */

export interface Snippet {
  id: string;
  label: string;
  /** hljs-style language hint for the <code> block */
  lang: string;
  code: string;
}

export const GATEWAY_BASE = "https://gate.kvasir-ai.net";

/* Official OpenAI-compatible adapter (single Node service). Point any OpenAI
   client's baseURL at http://localhost:8787/v1; it signs and pays each call
   from YOUR wallet (quote → sign → redeem), non-custodial. */
export const ADAPTER_SNIPPET = `// npm i express @solana/web3.js @solana/spl-token bs58   (Node 18+)
// Then: OpenAI client baseURL = http://localhost:8787/v1 , apiKey = anything.
import express from "express";
import { Connection, Keypair, Transaction, PublicKey } from "@solana/web3.js";
import {
  getAssociatedTokenAddress, getAccount,
  createAssociatedTokenAccountInstruction,
  createTransferCheckedInstruction, TokenAccountNotFoundError,
} from "@solana/spl-token";
import bs58 from "bs58";

const GATEWAY = "https://gate.kvasir-ai.net";
const RPC = "https://api.devnet.solana.com";
const PORT = 8787;

// YOUR devnet wallet — holds KVR + SOL. It never leaves this process.
const payer = Keypair.fromSecretKey(bs58.decode(process.env.KVR_SECRET_KEY));
const conn = new Connection(RPC, "confirmed");

async function gw(path, method, body) {
  const res = await fetch(GATEWAY + path, {
    method,
    headers: body ? { "content-type": "application/json" } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error || \`HTTP \${res.status}\`);
  return data;
}

async function payKvr(q) {
  const mint = new PublicKey(q.mint), recipient = new PublicKey(q.recipient);
  const src = await getAssociatedTokenAddress(mint, payer.publicKey);
  const dst = await getAssociatedTokenAddress(mint, recipient);
  const ixs = [];
  try { await getAccount(conn, dst); }
  catch (e) {
    if (e instanceof TokenAccountNotFoundError)
      ixs.push(createAssociatedTokenAccountInstruction(payer.publicKey, dst, recipient, mint));
    else throw e;
  }
  ixs.push(createTransferCheckedInstruction(src, mint, dst, payer.publicKey, Math.round(q.priceToken * 1e6), 6));
  const tx = new Transaction().add(...ixs);
  tx.feePayer = payer.publicKey;
  tx.recentBlockhash = (await conn.getLatestBlockhash()).blockhash;
  tx.sign(payer);
  const sig = await conn.sendRawTransaction(tx.serialize());
  await conn.confirmTransaction(sig, "confirmed");
  return sig;
}

const app = express();
app.use(express.json());

app.post("/v1/chat/completions", async (req, res) => {
  try {
    const { model, messages } = req.body;
    const prompt = messages.map((m) => \`\${m.role}: \${m.content}\`).join("\\n");
    const { models } = await gw("/api/pay/models", "GET");
    const id = models.find((m) => m.id === model || m.name === model)?.id ?? models[0]?.id;
    if (!id) return res.status(503).json({ error: { message: "no model is being served" } });
    const q = await gw("/api/pay/quote", "POST", { model: id, prompt });
    const signature = await payKvr(q);
    const r = await gw("/api/inference", "POST", { requestId: q.requestId, signature });
    res.json({
      id: q.requestId, object: "chat.completion", created: Math.floor(Date.now() / 1000),
      model: r.model ?? id,
      choices: [{ index: 0, message: { role: "assistant", content: r.result }, finish_reason: "stop" }],
      usage: {
        prompt_tokens: r.usage?.promptTokens ?? 0,
        completion_tokens: r.usage?.completionTokens ?? 0,
        total_tokens: r.usage?.totalTokens ?? 0,
      },
    });
  } catch (e) {
    res.status(502).json({ error: { message: String(e.message || e) } });
  }
});

app.get("/v1/models", async (_req, res) => {
  const { models } = await gw("/api/pay/models", "GET");
  res.json({ object: "list", data: models.map((m) => ({ id: m.id, object: "model" })) });
});

app.listen(PORT, () => console.log(\`Kvasir OpenAI adapter on http://localhost:\${PORT}/v1\`));`;

/* One-off: create a devnet test wallet and fund it with SOL for fees. */
export const WALLET_SNIPPET = `// npm i @solana/web3.js bs58
import { Keypair, Connection, LAMPORTS_PER_SOL } from "@solana/web3.js";
import bs58 from "bs58";

const kp = Keypair.generate();
console.log("address:       ", kp.publicKey.toBase58());
console.log("KVR_SECRET_KEY:", bs58.encode(kp.secretKey)); // save this in your env

const conn = new Connection("https://api.devnet.solana.com", "confirmed");
const sig = await conn.requestAirdrop(kp.publicKey, LAMPORTS_PER_SOL);
await conn.confirmTransaction(sig, "confirmed");
console.log("airdropped 1 SOL");

// Prefer the CLI?  solana-keygen new -o ./devnet.json
//                  solana airdrop 2 <address> --url https://api.devnet.solana.com`;

/* ------------------------------------------------------------------ */
/* Consume the paid inference API (quote → pay → redeem)               */
/* ------------------------------------------------------------------ */
export const SNIPPETS: Snippet[] = [
  {
    id: "ts",
    label: "TS / Node.js",
    lang: "typescript",
    code: `// npm i @solana/web3.js @solana/spl-token bs58
import { Connection, Keypair, Transaction, PublicKey } from "@solana/web3.js";
import {
  getAssociatedTokenAddress, getAccount,
  createAssociatedTokenAccountInstruction,
  createTransferCheckedInstruction, TokenAccountNotFoundError,
} from "@solana/spl-token";
import bs58 from "bs58";

const GATEWAY = "https://gate.kvasir-ai.net";
const RPC = "https://api.devnet.solana.com";
const payer = Keypair.fromSecretKey(bs58.decode(process.env.KVR_SECRET_KEY!));

async function api<T>(path: string, method: "GET" | "POST", body?: unknown): Promise<T> {
  const res = await fetch(GATEWAY + path, {
    method,
    headers: body ? { "content-type": "application/json" } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) throw new Error(\`\${res.status}: \${await res.text()}\`);
  return res.json();
}

async function ask(prompt: string) {
  const conn = new Connection(RPC, "confirmed");
  // 1. live model catalog (empty when the swarm serves nothing — never hardcode)
  const { models } = await api<{ models: { id: string }[] }>("/api/pay/models", "GET");
  if (!models.length) throw new Error("no model is being served right now");
  const model = models[0].id;
  // 2. quote → requestId + price in KVR
  const q = await api<any>("/api/pay/quote", "POST", { model, prompt });
  // 3. pay on-chain: send priceToken KVR to the recipient's token account
  const mint = new PublicKey(q.mint), recipient = new PublicKey(q.recipient);
  const srcAta = await getAssociatedTokenAddress(mint, payer.publicKey);
  const dstAta = await getAssociatedTokenAddress(mint, recipient);
  const ixs = [];
  try { await getAccount(conn, dstAta); }
  catch (e) {
    if (e instanceof TokenAccountNotFoundError)
      ixs.push(createAssociatedTokenAccountInstruction(payer.publicKey, dstAta, recipient, mint));
    else throw e;
  }
  ixs.push(createTransferCheckedInstruction(srcAta, mint, dstAta, payer.publicKey, Math.round(q.priceToken * 1e6), 6));
  const tx = new Transaction().add(...ixs);
  tx.feePayer = payer.publicKey;
  tx.recentBlockhash = (await conn.getLatestBlockhash()).blockhash;
  tx.sign(payer);
  const signature = await conn.sendRawTransaction(tx.serialize());
  await conn.confirmTransaction(signature, "confirmed");
  // 4. redeem: gateway verifies the payment, runs inference, returns the result
  const r = await api<any>("/api/inference", "POST", { requestId: q.requestId, signature });
  console.log(r.result);
  console.log("charged", r.usage.costToken, "KVR");
}

ask("Explain Mixture-of-Experts in two sentences.");`,
  },
  {
    id: "python",
    label: "Python",
    lang: "python",
    code: `# pip install solders solana requests
import os, requests
from solders.keypair import Keypair
from solders.pubkey import Pubkey
from solders.message import Message
from solders.transaction import Transaction
from solana.rpc.api import Client
from spl.token.constants import TOKEN_PROGRAM_ID
from spl.token.instructions import (
    get_associated_token_address, transfer_checked, TransferCheckedParams,
    create_associated_token_account,
)

GATEWAY = "https://gate.kvasir-ai.net"
rpc = Client("https://api.devnet.solana.com")
payer = Keypair.from_base58_string(os.environ["KVR_SECRET_KEY"])

def api(path, method, body=None):
    r = requests.request(method, GATEWAY + path, json=body)
    r.raise_for_status()
    return r.json()

def ask(prompt):
    # 1. live model catalog (empty when nothing is served — never hardcode)
    models = api("/api/pay/models", "GET")["models"]
    if not models:
        raise RuntimeError("no model is being served right now")
    model = models[0]["id"]
    # 2. quote -> requestId + price
    q = api("/api/pay/quote", "POST", {"model": model, "prompt": prompt})
    amount = round(q["priceToken"] * 1_000_000)  # KVR decimals = 6
    # 3. on-chain KVR transfer to the recipient's associated token account
    mint, recipient = Pubkey.from_string(q["mint"]), Pubkey.from_string(q["recipient"])
    src = get_associated_token_address(payer.pubkey(), mint)
    dst = get_associated_token_address(recipient, mint)
    ixs = []
    if rpc.get_account_info(dst).value is None:
        ixs.append(create_associated_token_account(payer.pubkey(), recipient, mint))
    ixs.append(transfer_checked(TransferCheckedParams(
        program_id=TOKEN_PROGRAM_ID, source=src, mint=mint, dest=dst,
        owner=payer.pubkey(), amount=amount, decimals=6, signers=[])))
    bh = rpc.get_latest_blockhash().value.blockhash
    tx = Transaction([payer], Message.new_with_blockhash(ixs, payer.pubkey(), bh), bh)
    sig = rpc.send_transaction(tx).value
    rpc.confirm_transaction(sig, "confirmed")
    # 4. redeem: gateway verifies payment, runs inference, returns the result
    r = api("/api/inference", "POST", {"requestId": q["requestId"], "signature": str(sig)})
    print(r["result"])
    print("charged", r["usage"].get("costToken"), "KVR")

ask("Explain Mixture-of-Experts in two sentences.")`,
  },
  {
    id: "java",
    label: "Java",
    lang: "java",
    code: `// Maven: com.mmorrell:solanaj  +  com.squareup.okhttp3:okhttp
import okhttp3.*;
import org.json.JSONObject;
import org.p2p.solanaj.core.*;
import org.p2p.solanaj.rpc.RpcClient;
import org.p2p.solanaj.token.TokenManager;

public class KvasirClient {
  static final String GATEWAY = "https://gate.kvasir-ai.net";
  static final String RPC = "https://api.devnet.solana.com";
  static final OkHttpClient HTTP = new OkHttpClient();

  static JSONObject api(String path, String method, JSONObject body) throws Exception {
    RequestBody rb = body == null ? null
        : RequestBody.create(body.toString(), MediaType.parse("application/json"));
    Request req = new Request.Builder().url(GATEWAY + path)
        .method(method, method.equals("GET") ? null : (rb != null ? rb : RequestBody.create("", null)))
        .build();
    try (Response res = HTTP.newCall(req).execute()) {
      String text = res.body().string();
      if (!res.isSuccessful()) throw new RuntimeException(res.code() + ": " + text);
      return new JSONObject(text);
    }
  }

  static void ask(String prompt, Account payer) throws Exception {
    RpcClient rpc = new RpcClient(RPC);
    // 1. live model catalog (empty when nothing is served — never hardcode)
    var models = api("/api/pay/models", "GET", null).getJSONArray("models");
    if (models.length() == 0) throw new RuntimeException("no model is being served right now");
    String model = models.getJSONObject(0).getString("id");
    // 2. quote -> requestId + price
    JSONObject q = api("/api/pay/quote", "POST",
        new JSONObject().put("model", model).put("prompt", prompt));
    long amount = Math.round(q.getDouble("priceToken") * 1_000_000L); // KVR decimals = 6
    // 3. on-chain KVR transfer (TokenManager resolves ATAs + transferChecked, creates dest if missing)
    PublicKey mint = new PublicKey(q.getString("mint"));
    PublicKey recipient = new PublicKey(q.getString("recipient"));
    String signature = new TokenManager(rpc).transfer(payer, recipient, mint, amount);
    // 4. redeem: gateway verifies payment, runs inference, returns the result
    JSONObject r = api("/api/inference", "POST",
        new JSONObject().put("requestId", q.getString("requestId")).put("signature", signature));
    System.out.println(r.getString("result"));
    System.out.println("charged " + r.getJSONObject("usage").optDouble("costToken") + " KVR");
  }
}`,
  },
  {
    id: "rust",
    label: "Rust",
    lang: "rust",
    code: `// Cargo: solana-sdk, solana-client, spl-token, spl-associated-token-account,
//        reqwest (blocking, json), serde_json
use solana_client::rpc_client::RpcClient;
use solana_sdk::{signature::Keypair, signer::Signer, transaction::Transaction, pubkey::Pubkey};
use spl_associated_token_account::{get_associated_token_address,
    instruction::create_associated_token_account_idempotent};
use spl_token::instruction::transfer_checked;
use std::str::FromStr;

const GATEWAY: &str = "https://gate.kvasir-ai.net";
const RPC: &str = "https://api.devnet.solana.com";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let payer = Keypair::from_base58_string(&std::env::var("KVR_SECRET_KEY")?);
    let rpc = RpcClient::new(RPC.to_string());
    let http = reqwest::blocking::Client::new();
    // 1. live model catalog (empty when nothing is served — never hardcode)
    let models: serde_json::Value = http.get(format!("{GATEWAY}/api/pay/models")).send()?.json()?;
    let model = models["models"][0]["id"].as_str().ok_or("no model served")?;
    // 2. quote -> requestId + price
    let q: serde_json::Value = http.post(format!("{GATEWAY}/api/pay/quote"))
        .json(&serde_json::json!({ "model": model, "prompt": "Explain MoE in two sentences." }))
        .send()?.json()?;
    let amount = (q["priceToken"].as_f64().unwrap() * 1_000_000.0).round() as u64; // decimals = 6
    // 3. on-chain KVR transfer to the recipient's associated token account
    let mint = Pubkey::from_str(q["mint"].as_str().unwrap())?;
    let recipient = Pubkey::from_str(q["recipient"].as_str().unwrap())?;
    let src = get_associated_token_address(&payer.pubkey(), &mint);
    let dst = get_associated_token_address(&recipient, &mint);
    let ixs = vec![
        create_associated_token_account_idempotent(&payer.pubkey(), &recipient, &mint, &spl_token::id()),
        transfer_checked(&spl_token::id(), &src, &mint, &dst, &payer.pubkey(), &[], amount, 6)?,
    ];
    let bh = rpc.get_latest_blockhash()?;
    let tx = Transaction::new_signed_with_payer(&ixs, Some(&payer.pubkey()), &[&payer], bh);
    let signature = rpc.send_and_confirm_transaction(&tx)?.to_string();
    // 4. redeem: gateway verifies payment, runs inference, returns the result
    let r: serde_json::Value = http.post(format!("{GATEWAY}/api/inference"))
        .json(&serde_json::json!({ "requestId": q["requestId"], "signature": signature }))
        .send()?.json()?;
    println!("{}", r["result"].as_str().unwrap_or(""));
    println!("charged {} KVR", r["usage"]["costToken"]);
    Ok(())
}`,
  },
  {
    id: "go",
    label: "Go",
    lang: "go",
    code: `// go get github.com/gagliardetto/solana-go
package main

import (
	"bytes"; "context"; "encoding/json"; "io"; "math"; "net/http"; "os"; "fmt"

	"github.com/gagliardetto/solana-go"
	"github.com/gagliardetto/solana-go/programs/associated-token-account"
	"github.com/gagliardetto/solana-go/programs/token"
	"github.com/gagliardetto/solana-go/rpc"
	confirm "github.com/gagliardetto/solana-go/rpc/sendAndConfirmTransaction"
	"github.com/gagliardetto/solana-go/rpc/ws"
)

const gateway = "https://gate.kvasir-ai.net"

func api(path, method string, body any) map[string]any {
	var rd io.Reader
	if body != nil { b, _ := json.Marshal(body); rd = bytes.NewReader(b) }
	req, _ := http.NewRequest(method, gateway+path, rd)
	if body != nil { req.Header.Set("content-type", "application/json") }
	res, _ := http.DefaultClient.Do(req)
	defer res.Body.Close()
	var out map[string]any
	json.NewDecoder(res.Body).Decode(&out)
	if res.StatusCode >= 300 { panic(fmt.Sprint(res.StatusCode, out)) }
	return out
}

func main() {
	ctx := context.Background()
	payer := solana.MustPrivateKeyFromBase58(os.Getenv("KVR_SECRET_KEY"))
	client := rpc.New(rpc.DevNet_RPC)
	wsc, _ := ws.Connect(ctx, rpc.DevNet_WS)
	// 1. live model catalog (empty when nothing is served — never hardcode)
	models := api("/api/pay/models", "GET", nil)["models"].([]any)
	if len(models) == 0 { panic("no model is being served right now") }
	model := models[0].(map[string]any)["id"].(string)
	// 2. quote -> requestId + price
	q := api("/api/pay/quote", "POST", map[string]string{"model": model, "prompt": "Explain MoE."})
	amount := uint64(math.Round(q["priceToken"].(float64) * 1_000_000)) // KVR decimals = 6
	// 3. on-chain KVR transfer to the recipient's associated token account
	mint := solana.MustPublicKeyFromBase58(q["mint"].(string))
	recipient := solana.MustPublicKeyFromBase58(q["recipient"].(string))
	src, _, _ := solana.FindAssociatedTokenAddress(payer.PublicKey(), mint)
	dst, _, _ := solana.FindAssociatedTokenAddress(recipient, mint)
	bh, _ := client.GetLatestBlockhash(ctx, rpc.CommitmentFinalized)
	tx, _ := solana.NewTransaction([]solana.Instruction{
		associatedtokenaccount.NewCreateInstruction(payer.PublicKey(), recipient, mint).Build(),
		token.NewTransferCheckedInstruction(amount, 6, src, mint, dst, payer.PublicKey(), nil).Build(),
	}, bh.Value.Blockhash, solana.TransactionPayer(payer.PublicKey()))
	tx.Sign(func(k solana.PublicKey) *solana.PrivateKey { return &payer })
	sig, _ := confirm.SendAndConfirmTransaction(ctx, client, wsc, tx)
	// 4. redeem: gateway verifies payment, runs inference, returns the result
	r := api("/api/inference", "POST", map[string]any{"requestId": q["requestId"], "signature": sig.String()})
	fmt.Println(r["result"])
	fmt.Println("charged", r["usage"].(map[string]any)["costToken"], "KVR")
}`,
  },
];

/* ------------------------------------------------------------------ */
/* Run your OWN hub + gateway + node → a free local OpenAI endpoint     */
/* Automates the shell/docker bring-up; loading a model is a one-time   */
/* step in the hub UI (localhost:19000). Then /c/<cid>/v1 is standard   */
/* OpenAI, served on your own hardware — no per-token KVR.              */
/* ------------------------------------------------------------------ */
export const NODE_SETUP_SNIPPETS: Snippet[] = [
  {
    id: "ts",
    label: "TS / Node.js",
    lang: "typescript",
    code: `// Free inference on your own hardware. Run once (Docker + git required).
import { execSync } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";

const REPO = "https://github.com/louisevandan/kvasir-net";
const HUB = "http://localhost:19000";
const sh = (cmd: string) => execSync(cmd, { stdio: "inherit", shell: "/bin/bash" });

// 1. bring up the hub (bakes the inference engine binaries) on :19000
sh(\`test -d kvasir || git clone \${REPO} kvasir\`);
sh("cd kvasir && cp -n env.example .env; docker compose up -d --build");
// optional — the KVR gateway, to serve others and earn:
// sh("cd kvasir/solana/staking-service && docker compose up -d --build");

// 2. wait for the hub, then finish setup in its UI (one time)
for (let i = 0; i < 90; i++) { try { if ((await fetch(HUB)).ok) break; } catch {} await sleep(2000); }
console.log(\`Hub up → open \${HUB}: add a node slot with your GPU, load an open\` +
            \` model, copy the controller id, then set KVASIR_CID.\`);

// 3. free inference — a standard OpenAI endpoint on YOUR machine, no KVR
const cid = process.env.KVASIR_CID;
const res = await fetch(\`\${HUB}/c/\${cid}/v1/chat/completions\`, {
  method: "POST", headers: { "content-type": "application/json" },
  body: JSON.stringify({ model: "local", messages: [{ role: "user", content: "Hello!" }] }),
});
console.log((await res.json()).choices[0].message.content);`,
  },
  {
    id: "python",
    label: "Python",
    lang: "python",
    code: `# Free inference on your own hardware. Run once (Docker + git required).
import os, time, subprocess, requests

REPO = "https://github.com/louisevandan/kvasir-net"
HUB = "http://localhost:19000"
def sh(cmd): subprocess.run(cmd, shell=True, check=True)

# 1. bring up the hub (bakes the inference engine binaries) on :19000
if not os.path.isdir("kvasir"):
    sh(f"git clone {REPO} kvasir")
sh("cd kvasir && cp -n env.example .env; docker compose up -d --build")
# optional — the KVR gateway, to serve others and earn:
# sh("cd kvasir/solana/staking-service && docker compose up -d --build")

# 2. wait for the hub, then finish setup in its UI (one time)
for _ in range(90):
    try:
        if requests.get(HUB, timeout=2).ok: break
    except Exception: time.sleep(2)
print(f"Hub up -> open {HUB}: add a node slot with your GPU, load an open "
      f"model, copy the controller id, then set KVASIR_CID.")

# 3. free inference — a standard OpenAI endpoint on YOUR machine, no KVR
cid = os.environ["KVASIR_CID"]
r = requests.post(f"{HUB}/c/{cid}/v1/chat/completions", json={
    "model": "local", "messages": [{"role": "user", "content": "Hello!"}]})
print(r.json()["choices"][0]["message"]["content"])`,
  },
  {
    id: "java",
    label: "Java",
    lang: "java",
    code: `// Free inference on your own hardware. Run once (Docker + git required).
import java.net.http.*;
import java.net.URI;
import java.nio.file.*;

public class KvasirNode {
  static final String REPO = "https://github.com/louisevandan/kvasir-net";
  static final String HUB = "http://localhost:19000";

  static void sh(String cmd) throws Exception {
    new ProcessBuilder("bash", "-lc", cmd).inheritIO().start().waitFor();
  }

  public static void main(String[] a) throws Exception {
    // 1. bring up the hub (bakes the inference engine binaries) on :19000
    if (!Files.isDirectory(Path.of("kvasir"))) sh("git clone " + REPO + " kvasir");
    sh("cd kvasir && cp -n env.example .env; docker compose up -d --build");
    // optional — the KVR gateway, to serve others and earn:
    // sh("cd kvasir/solana/staking-service && docker compose up -d --build");

    // 2. wait for the hub, then finish setup in its UI (one time)
    HttpClient http = HttpClient.newHttpClient();
    for (int i = 0; i < 90; i++) {
      try { http.send(HttpRequest.newBuilder(URI.create(HUB)).build(), HttpResponse.BodyHandlers.discarding()); break; }
      catch (Exception e) { Thread.sleep(2000); }
    }
    System.out.println("Hub up -> open " + HUB + ": add a node slot with your GPU, load an "
        + "open model, copy the controller id, then set KVASIR_CID.");

    // 3. free inference — a standard OpenAI endpoint on YOUR machine, no KVR
    String cid = System.getenv("KVASIR_CID");
    var req = HttpRequest.newBuilder(URI.create(HUB + "/c/" + cid + "/v1/chat/completions"))
        .header("content-type", "application/json")
        .POST(HttpRequest.BodyPublishers.ofString(
            "{\\"model\\":\\"local\\",\\"messages\\":[{\\"role\\":\\"user\\",\\"content\\":\\"Hello!\\"}]}"))
        .build();
    System.out.println(http.send(req, HttpResponse.BodyHandlers.ofString()).body());
  }
}`,
  },
  {
    id: "rust",
    label: "Rust",
    lang: "rust",
    code: `// Free inference on your own hardware. Run once (Docker + git required).
// Cargo: reqwest (blocking, json), serde_json
use std::{process::Command, thread::sleep, time::Duration, path::Path, env};

const REPO: &str = "https://github.com/louisevandan/kvasir-net";
const HUB: &str = "http://localhost:19000";

fn sh(cmd: &str) { Command::new("bash").arg("-lc").arg(cmd).status().unwrap(); }

fn main() {
    // 1. bring up the hub (bakes the inference engine binaries) on :19000
    if !Path::new("kvasir").is_dir() { sh(&format!("git clone {REPO} kvasir")); }
    sh("cd kvasir && cp -n env.example .env; docker compose up -d --build");
    // optional — the KVR gateway, to serve others and earn:
    // sh("cd kvasir/solana/staking-service && docker compose up -d --build");

    // 2. wait for the hub, then finish setup in its UI (one time)
    let http = reqwest::blocking::Client::new();
    for _ in 0..90 { if http.get(HUB).send().is_ok() { break } sleep(Duration::from_secs(2)); }
    println!("Hub up -> open {HUB}: add a node slot with your GPU, load an open \\
              model, copy the controller id, then set KVASIR_CID.");

    // 3. free inference — a standard OpenAI endpoint on YOUR machine, no KVR
    let cid = env::var("KVASIR_CID").unwrap();
    let body = serde_json::json!({ "model": "local",
        "messages": [{ "role": "user", "content": "Hello!" }] });
    let r = http.post(format!("{HUB}/c/{cid}/v1/chat/completions")).json(&body).send().unwrap();
    println!("{}", r.text().unwrap());
}`,
  },
  {
    id: "go",
    label: "Go",
    lang: "go",
    code: `// Free inference on your own hardware. Run once (Docker + git required).
package main

import (
	"bytes"; "net/http"; "os"; "os/exec"; "time"; "io"; "fmt"
)

const repo = "https://github.com/louisevandan/kvasir-net"
const hub = "http://localhost:19000"

func sh(cmd string) { c := exec.Command("bash", "-lc", cmd); c.Stdout = os.Stdout; c.Stderr = os.Stderr; c.Run() }

func main() {
	// 1. bring up the hub (bakes the inference engine binaries) on :19000
	if _, err := os.Stat("kvasir"); err != nil { sh("git clone " + repo + " kvasir") }
	sh("cd kvasir && cp -n env.example .env; docker compose up -d --build")
	// optional — the KVR gateway, to serve others and earn:
	// sh("cd kvasir/solana/staking-service && docker compose up -d --build")

	// 2. wait for the hub, then finish setup in its UI (one time)
	for i := 0; i < 90; i++ { if _, err := http.Get(hub); err == nil { break }; time.Sleep(2 * time.Second) }
	fmt.Printf("Hub up -> open %s: add a node slot with your GPU, load an open model, "+
		"copy the controller id, then set KVASIR_CID.\\n", hub)

	// 3. free inference — a standard OpenAI endpoint on YOUR machine, no KVR
	cid := os.Getenv("KVASIR_CID")
	body := []byte(\`{"model":"local","messages":[{"role":"user","content":"Hello!"}]}\`)
	res, _ := http.Post(hub+"/c/"+cid+"/v1/chat/completions", "application/json", bytes.NewReader(body))
	out, _ := io.ReadAll(res.Body)
	fmt.Println(string(out))
}`,
  },
];

/* ------------------------------------------------------------------ */
/* Native OpenAI endpoint with a prepaid-credit API key (stream + tools) */
/* Backend live at gate.kvasir-ai.net (commit 552763d).                  */
/* ------------------------------------------------------------------ */

/* Issue a key once — the wallet signs a SIWS challenge; apiKey is returned once. */
export const KEY_ISSUE_SNIPPET = `# 1. get a challenge for your (whitelisted) wallet
curl -X POST https://gate.kvasir-ai.net/api/credits/challenge \\
  -H 'content-type: application/json' \\
  -d '{"wallet":"<YOUR_WALLET>"}'
# → { "nonce": "...", "message": "linkcpp gateway ...\\nnonce: ..." }

# 2. sign the returned \`message\` with your wallet (ed25519) → signature (base58)

# 3. exchange it for an API key — returned ONCE, store it
curl -X POST https://gate.kvasir-ai.net/api/credits/apikey \\
  -H 'content-type: application/json' \\
  -d '{"wallet":"<YOUR_WALLET>","nonce":"<nonce>","signature":"<sig>","label":"my-app"}'
# → { "apiKey": "kvr-...." }`;

/* Fastest streaming smoke test — plain curl. */
export const STREAM_CURL_SNIPPET = `curl -N https://gate.kvasir-ai.net/v1/chat/completions \\
  -H "Authorization: Bearer kvr-...." -H 'content-type: application/json' \\
  -d '{"model":"f793eb7b:ctrl-f11eb9","messages":[{"role":"user","content":"hi"}],"stream":true}'`;

/* Call it with the standard OpenAI SDK in each language — only base_url + key change. */
export const INFERENCE_API_SNIPPETS: Snippet[] = [
  {
    id: "ts",
    label: "TS / Node.js",
    lang: "typescript",
    code: `// npm i openai
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "https://gate.kvasir-ai.net/v1",
  apiKey: process.env.KVR_API_KEY, // kvr-....
});

const stream = await client.chat.completions.create({
  model: "f793eb7b:ctrl-f11eb9",            // GET /v1/models to list
  messages: [{ role: "user", content: "What's the weather in Seoul?" }],
  tools: [{ type: "function", function: {
    name: "get_weather",
    parameters: { type: "object", properties: { city: { type: "string" } }, required: ["city"] },
  }}],
  tool_choice: "auto",
  stream: true,
  // reasoning model — turn thinking off for short answers / tool calls
  chat_template_kwargs: { enable_thinking: false },
} as any);

for await (const chunk of stream) {
  const d = chunk.choices[0].delta;
  if (d.content) process.stdout.write(d.content);
  if (d.tool_calls) console.log(d.tool_calls);
}`,
  },
  {
    id: "python",
    label: "Python",
    lang: "python",
    code: `# pip install openai
import os
from openai import OpenAI

client = OpenAI(base_url="https://gate.kvasir-ai.net/v1", api_key=os.environ["KVR_API_KEY"])

stream = client.chat.completions.create(
    model="f793eb7b:ctrl-f11eb9",           # GET /v1/models to list
    messages=[{"role": "user", "content": "What's the weather in Seoul?"}],
    tools=[{"type": "function", "function": {
        "name": "get_weather",
        "parameters": {"type": "object",
                       "properties": {"city": {"type": "string"}}, "required": ["city"]}}}],
    tool_choice="auto",
    stream=True,
    # reasoning model — turn thinking off for short answers / tool calls
    extra_body={"chat_template_kwargs": {"enable_thinking": False}},
)
for chunk in stream:
    d = chunk.choices[0].delta
    if d.content: print(d.content, end="", flush=True)
    if d.tool_calls: print(d.tool_calls)`,
  },
  {
    id: "java",
    label: "Java",
    lang: "java",
    code: `// implementation("com.openai:openai-java:0.9.0")
import com.openai.client.OpenAIClient;
import com.openai.client.okhttp.OpenAIOkHttpClient;
import com.openai.models.chat.completions.ChatCompletionCreateParams;

OpenAIClient client = OpenAIOkHttpClient.builder()
    .baseUrl("https://gate.kvasir-ai.net/v1")
    .apiKey(System.getenv("KVR_API_KEY"))   // kvr-....
    .build();

ChatCompletionCreateParams params = ChatCompletionCreateParams.builder()
    .model("f793eb7b:ctrl-f11eb9")          // GET /v1/models to list
    .addUserMessage("What's the weather in Seoul?")
    .build();

// streaming — iterate the token deltas
try (var stream = client.chat().completions().createStreaming(params)) {
  stream.stream().forEach(chunk ->
      chunk.choices().forEach(c -> c.delta().content().ifPresent(System.out::print)));
}`,
  },
  {
    id: "rust",
    label: "Rust",
    lang: "rust",
    code: `// async-openai = "0.23", futures = "0.3", tokio = { features = ["full"] }
use async_openai::{Client, config::OpenAIConfig,
    types::{CreateChatCompletionRequestArgs, ChatCompletionRequestUserMessageArgs}};
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = OpenAIConfig::new()
        .with_api_base("https://gate.kvasir-ai.net/v1")
        .with_api_key(std::env::var("KVR_API_KEY")?); // kvr-....
    let client = Client::with_config(config);

    let req = CreateChatCompletionRequestArgs::default()
        .model("f793eb7b:ctrl-f11eb9")      // GET /v1/models to list
        .messages([ChatCompletionRequestUserMessageArgs::default()
            .content("What's the weather in Seoul?").build()?.into()])
        .stream(true).build()?;

    let mut stream = client.chat().create_stream(req).await?;
    while let Some(res) = stream.next().await {
        for choice in res?.choices {
            if let Some(c) = choice.delta.content { print!("{c}"); }
        }
    }
    Ok(())
}`,
  },
  {
    id: "go",
    label: "Go",
    lang: "go",
    code: `// go get github.com/sashabaranov/go-openai
package main

import (
	"context"; "fmt"; "io"; "os"
	openai "github.com/sashabaranov/go-openai"
)

func main() {
	cfg := openai.DefaultConfig(os.Getenv("KVR_API_KEY")) // kvr-....
	cfg.BaseURL = "https://gate.kvasir-ai.net/v1"
	client := openai.NewClientWithConfig(cfg)

	stream, _ := client.CreateChatCompletionStream(context.Background(), openai.ChatCompletionRequest{
		Model:    "f793eb7b:ctrl-f11eb9", // GET /v1/models to list
		Messages: []openai.ChatCompletionMessage{{Role: "user", Content: "What's the weather in Seoul?"}},
		Stream:   true,
	})
	defer stream.Close()
	for {
		res, err := stream.Recv()
		if err == io.EOF { break }
		fmt.Print(res.Choices[0].Delta.Content)
	}
}`,
  },
];

/* Self-issue an API key end to end from a wallet secret — for headless coding
   agents. Signs the SIWS challenges (ed25519 → base64), self-registers, and
   issues a key, with no browser. SIWS spec: sign the challenge `message` string
   verbatim; submit the 64-byte signature as base64. */
export const SELF_ISSUE_SNIPPETS: Snippet[] = [
  {
    id: "ts",
    label: "TS / Node.js",
    lang: "typescript",
    code: `// npm i @solana/web3.js bs58 tweetnacl   (Node 18+)
import { Keypair } from "@solana/web3.js";
import bs58 from "bs58";
import nacl from "tweetnacl";

const GW = "https://gate.kvasir-ai.net";
const kp = Keypair.fromSecretKey(bs58.decode(process.env.KVR_SECRET_KEY!));
const wallet = kp.publicKey.toBase58();

const post = (path: string, body: unknown) =>
  fetch(GW + path, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })
    .then(async (r) => ({ ok: r.ok, status: r.status, data: await r.json().catch(() => ({} as any)) }));
// SIWS: sign the challenge message verbatim, submit as base64
const sign = (msg: string) => Buffer.from(nacl.sign.detached(new TextEncoder().encode(msg), kp.secretKey)).toString("base64");

// 1. self-register onto the whitelist (403 => self-register closed, operator must approve)
const reg = await post("/api/credits/register/challenge", { wallet });
if (reg.ok) {
  const r = await post("/api/credits/register", { wallet, nonce: reg.data.nonce, signature: sign(reg.data.message) });
  if (!r.ok && r.status !== 403) throw new Error(r.data.error);
}
// 2. issue a key
const ch = await post("/api/credits/challenge", { wallet });
if (!ch.ok) throw new Error(ch.status === 403 ? "not whitelisted — awaiting operator approval" : ch.data.error);
const key = await post("/api/credits/apikey", { wallet, nonce: ch.data.nonce, signature: sign(ch.data.message), label: "agent" });

console.log(key.data.apiKey); // kvr-.... — now an OpenAI Bearer key at GW + "/v1"`,
  },
  {
    id: "python",
    label: "Python",
    lang: "python",
    code: `# pip install solders requests
import os, base64, requests
from solders.keypair import Keypair

GW = "https://gate.kvasir-ai.net"
kp = Keypair.from_base58_string(os.environ["KVR_SECRET_KEY"])
wallet = str(kp.pubkey())

def post(path, body):
    r = requests.post(GW + path, json=body)
    return r.status_code, (r.json() if r.content else {})

# SIWS: sign the challenge message verbatim, submit as base64
def sign(msg: str) -> str:
    return base64.b64encode(bytes(kp.sign_message(msg.encode()))).decode()

# 1. self-register onto the whitelist (403 => self-register closed, operator must approve)
st, ch = post("/api/credits/register/challenge", {"wallet": wallet})
if st == 200:
    st2, r = post("/api/credits/register", {"wallet": wallet, "nonce": ch["nonce"], "signature": sign(ch["message"])})
    if st2 not in (200, 403):
        raise RuntimeError(r)

# 2. issue a key
st, ch = post("/api/credits/challenge", {"wallet": wallet})
if st != 200:
    raise RuntimeError("not whitelisted — awaiting operator approval" if st == 403 else ch)
st, key = post("/api/credits/apikey", {"wallet": wallet, "nonce": ch["nonce"], "signature": sign(ch["message"]), "label": "agent"})

print(key["apiKey"])  # kvr-.... — now an OpenAI Bearer key at GW + "/v1"`,
  },
];

/* Reference rows — labels are translated (t.apiDocs.inferenceApiRef[key]); values universal. */
export interface CreditRefRow {
  key: "base" | "auth" | "endpoints" | "balance" | "pricing" | "errors" | "context" | "model";
  value: string;
}
export const INFERENCE_API_REF: CreditRefRow[] = [
  { key: "base", value: "https://gate.kvasir-ai.net/v1" },
  { key: "auth", value: "Authorization: Bearer <apiKey>" },
  { key: "endpoints", value: "GET /v1/models · POST /v1/chat/completions (stream + tools)" },
  { key: "balance", value: "GET /api/credits/balance → { balance, spent, symbol }" },
  { key: "pricing", value: "basePrice + tokens × perToken KVR  (now 0.01 + tokens × 0.00002)" },
  { key: "errors", value: "402 no credit · 401 bad key · 403 not whitelisted" },
  { key: "context", value: "128K tokens (request body ≤ 2 MB)" },
  { key: "model", value: "f793eb7b:ctrl-f11eb9 (Qwen3.5-122B-A10B-Q4_K_M)" },
];

/* The four canonical API calls, shown as a language-agnostic reference. */
export interface ApiRef {
  method: string;
  path: string;
  reqKey: "apiModels" | "apiQuote" | "apiPay" | "apiInfer";
  body?: string;
  sample: string;
}

export const API_REFS: ApiRef[] = [
  {
    method: "GET",
    path: "/api/pay/models",
    reqKey: "apiModels",
    sample: `{
  "recipient": "8uu2gDKFVtNS79yqYyztJeerEKAh4cnZGdQytCjsYNfF",
  "mint": "6cuJAmqtMuGzJ7s7eWQSqfJvEFRUdTiYR3cuMmiNoCPQ",
  "symbol": "KVR",
  "models": [{ "id": "f793eb7b:ctrl-f11eb9", "name": "Qwen3.5-122B-A10B-Q4_K_M" }]
}`,
  },
  {
    method: "POST",
    path: "/api/pay/quote",
    reqKey: "apiQuote",
    body: `{ "model": "<id>", "prompt": "<your prompt>" }`,
    sample: `{
  "requestId": "44b8c15c-4491-4eab-a2a9-62326344ee50",
  "model": "f793eb7b:ctrl-f11eb9",
  "priceToken": 3.08,
  "recipient": "8uu2gDKFVtNS79yqYyztJeerEKAh4cnZGdQytCjsYNfF",
  "mint": "6cuJAmqtMuGzJ7s7eWQSqfJvEFRUdTiYR3cuMmiNoCPQ",
  "symbol": "KVR",
  "estimated": true,
  "estPromptTokens": 2, "estCompletionTokens": 256, "estTotalTokens": 258
}`,
  },
  {
    method: "SPL",
    path: "transfer_checked",
    reqKey: "apiPay",
    body: `round(priceToken × 10^6) KVR  →  getAssociatedTokenAddress(mint, recipient)`,
    sample: `// amount (base units) = round(priceToken * 1_000_000)   // decimals = 6
// destination          = recipient's associated token account (the vault)
// sign with your wallet keypair, submit, keep the transaction signature`,
  },
  {
    method: "POST",
    path: "/api/inference",
    reqKey: "apiInfer",
    body: `{ "requestId": "<uuid>", "signature": "<tx sig>" }`,
    sample: `{
  "requestId": "…", "paid": true, "signature": "…",
  "model": "f793eb7b:ctrl-f11eb9", "priceToken": 3.08,
  "result": "…model response (markdown)…",
  "usage": { "promptTokens": 2, "completionTokens": 141, "totalTokens": 143, "costToken": 1.35 }
}`,
  },
];
