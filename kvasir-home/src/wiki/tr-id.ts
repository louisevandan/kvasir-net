/* Bahasa Indonesia — terjemahan entri wiki. Struktur (slug, kategori, urutan
   blok, kode) mencerminkan persis entries.ts (sumber bahasa Inggris); istilah
   teknis dan pengenal (KVR, linkcpp, mesin inferensi, GGUF, MoE, ring runtime,
   tok/s, dll.) dipertahankan apa adanya. Kerangka kepatuhan (devnet, token
   utilitas, non-kustodial) tetap utuh. */
import type { WikiTranslation } from "./entries";

export const idWiki: Record<string, WikiTranslation> = {
  "kvasir-network": {
    title: "Jaringan Kvasir",
    summary: "Jaringan inferensi AI terdesentralisasi (DePIN) tempat perangkat sehari-hari melayani model terbuka dan memperoleh KVR.",
    blocks: [
      {
        t: "p",
        md: "**Kvasir** adalah jaringan inferensi AI terdesentralisasi: model terbuka berukuran besar dibagi ke perangkat keras bersama dengan mesin **linkcpp**, sehingga tidak ada satu node pun yang memegang seluruh model. Siapa pun dapat menyumbangkan GPU, CPU, NPU — bahkan ponsel — dan memperoleh **KVR** untuk lapisan atau pakar yang benar-benar dilayani perangkatnya. Pengembang mengakses jaringan lewat gateway yang kompatibel dengan OpenAI/Anthropic dan membayar per inferensi.",
      },
      {
        t: "ul",
        items: [
          "**Mesin bersumber tersedia** — linkcpp berlisensi BSL (gratis untuk pengembangan dan pengujian, penggunaan produksi memerlukan lisensi); bidang data mesin inferensi di bawahnya tetap asli dan dapat diperiksa.",
          "**Non-kustodial** — imbalan diselesaikan ke dompet Solana milik masing-masing pemilik node; kunci tidak pernah meninggalkan pengguna.",
          "**Terbukti di perangkat nyata** — model 122B pernah berjalan terbagi di 4 GPU AMD MI250, di armada heterogen node GPU/CPU/NPU/seluler, dengan kontribusi tiap node tercatat ujung-ke-ujung.",
          "**Dinamai dari mitos Nordik** — Kvasir, makhluk paling bijak, lahir dari sari gabungan semua dewa dan bukan milik siapa pun.",
        ],
      },
      { t: "h2", kick: "Satu permintaan, banyak perangkat", text: "Bagaimana inferensi mengalir" },
      {
        t: "code",
        caption: "Setiap lompatan adalah HTTP/TCP biasa; yang terdistribusi adalah modelnya sendiri.",
        code: `client SDK ──▶ gateway (OpenAI/Anthropic API, KVR settlement)
        ──▶ hub controller (plan · orchestrate)
        ──▶ serving topology: pipeline ring over layer windows,
            or expert-swarm dispatch at (layer, expert-range) grain
        ──▶ token streams back · each node's contribution is credited`,
      },
      {
        t: "p",
        md: "Peran bisa **bertumpuk**: satu mesin dapat sekaligus menjadi node komputasi, host gateway, dan host hub, dan imbalannya dijumlahkan. Tugas jaringan adalah membuat keseluruhan tampak seperti satu mesin — satu endpoint di depan, ribuan perangkat tak sempurna di belakang.",
      },
      {
        t: "p",
        md: "Saat ini jaringan berjalan di **Solana devnet**; KVR adalah token utilitas / kontribusi, bukan aset yang dapat diperdagangkan atau investasi, dan tidak ada di halaman ini yang merupakan nasihat keuangan.",
      },
    ],
  },
  hub: {
    title: "Hub",
    summary: "Bidang kendali: menemukan perangkat, merencanakan penempatan lapisan, meluncurkan worker, mengorkestrasi ring.",
    blocks: [
      {
        t: "p",
        md: "**Hub** adalah bidang kendali jaringan, disediakan linkcpp sebagai satu image Docker (`controller.hub:app`, layanan FastAPI di port **19000**). Ia menemukan perangkat, memeriksa kompatibilitas runtime, merencanakan penempatan dengan planner, meluncurkan worker mesin inferensi standar, dan mengekspos gateway per controller. Ini infrastruktur yang sengaja membosankan: HTTP request/response, status tahan-restart, tanpa transport eksotis.",
      },
      { t: "h2", kick: "Tiga pintu masuk", text: "Bagaimana mesin bergabung ke hub" },
      {
        t: "ul",
        items: [
          "**Slot node lokal** — lima slot tetap per hub, dipetakan ke port RPC **50052–50056**. Slot selalu ada; Anda mengedit anggaran GPU + VRAM/RAM/CPU sebuah slot alih-alih membuat node sembarang, dan sumber daya hanya dapat diedit **selama slot belum terikat** — melindungi kontrak kapasitas di bawah controller yang berjalan.",
          "**Unit jarak jauh** — daftarkan hub linkcpp lain yang berjalan dan impor node-node yang terlihat. Endpoint bidang data selalu diturunkan dari URL *unit* terdaftar plus port worker yang diekspos unit — tidak pernah dari host node yang diiklankan sistem jarak jauh.",
          "**Agen node terkelola** — layanan khusus-worker (`nodeagent.py`) yang bergabung lewat HTTP request/response sederhana (`/control/join|status|download|load|unload`) dan melapor via `POST /api/node-reports`. Sengaja **bukan** stream persisten, agar bertahan di perutean LAN/VPN sederhana.",
        ],
      },
      { t: "h2", kick: "Tak ada yang dimuat tanpa verifikasi", text: "Gerbang kompatibilitas" },
      {
        t: "p",
        md: "Setiap unit, node, dan agen melaporkan identitas protokol / runtime-pack plus detail backend. Ketidakcocokan unit, runtime-pack, revisi mesin inferensi, dan ABI RPC **diblokir keras sebelum bind, plan, load, atau infer**; perbedaan backend (CUDA/Metal/Vulkan/CPU) dicatat sebagai kapabilitas node, bukan penolakan. Pemuatan adaptif juga diblokir bila sebuah node tak dapat menyediakan pemantauan sumber daya yang dibutuhkan rencana yang aman.",
      },
      {
        t: "code",
        caption: "Apa yang selamat dari restart, dan apa yang tidak.",
        code: `persisted   → /models/linkcpp/hub-state.json
              slots · controllers · bindings · remote units · 2FA enrollment
runtime-only → live worker/model processes, in-flight operations
              (a container restart stops serving; models reload on demand)`,
      },
      {
        t: "p",
        md: "Karena hub adalah peran paling kritis, host hub memperoleh **imbalan uptime per jam tertinggi**. Mengoperasikan hub publik memerlukan staking **100.000 KVR**.",
      },
    ],
  },
  gateway: {
    title: "Gateway",
    summary: "Titik masuk publik: API kompatibel OpenAI/Anthropic dan penyelesaian bayar-per-inferensi dalam KVR.",
    blocks: [
      {
        t: "p",
        md: "**Gateway** adalah tempat pengembang bertemu jaringan. Setiap controller mengekspos endpoint kompatibel OpenAI (`/v1/chat/completions`, `/v1/responses`, `/v1/models`) dan kompatibel Anthropic (`/anthropic/v1/messages`, `/anthropic/v1/models`), semuanya ditopang model termuat yang sama — klien yang ada berfungsi hanya dengan mengganti base URL dan kunci.",
      },
      {
        t: "code",
        caption: "Panggilan gaya OpenAI standar ke gateway Kvasir.",
        code: `curl https://gate.kvasir-ai.net/v1/chat/completions \\
  -H "Authorization: Bearer $KVR_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{ "model": "Qwen3.5-122B-A10B",
        "messages": [{ "role": "user", "content": "..." }] }'`,
      },
      { t: "h2", kick: "Pengukuran", text: "Bayar per inferensi dalam KVR" },
      {
        t: "p",
        md: "Pemakaian diselesaikan dalam KVR lewat alur tiga langkah — **quote → payment → inference** — sehingga permintaan diberi harga sebelum dijalankan dan node yang melayaninya dikreditkan sesudahnya. Gateway juga mengagregasi **katalog model langsung** dari setiap hub yang terjangkau, sehingga `/v1/models` mencerminkan apa yang benar-benar bisa dilayani jaringan saat ini.",
      },
      {
        t: "ul",
        items: [
          "Host gateway memperoleh **imbalan uptime per jam** karena menjaga titik masuk tetap daring, plus **bonus ×1.5** pada setiap inferensi yang ikut mereka layani.",
          "Mengoperasikan gateway publik memerlukan staking **100.000 KVR** (sama seperti hub).",
          "Deployment publik melindungi akses operator dengan **SIWS + 2FA**; hub polos dirancang hanya untuk host tepercaya / LAN / VPN.",
        ],
      },
    ],
  },
  node: {
    title: "Node",
    summary: "Perangkat apa pun yang melayani sebagian model — GPU, CPU, NPU, atau ponsel — memperoleh KVR untuk kerja yang dilakukannya.",
    blocks: [
      {
        t: "p",
        md: "**Node** adalah perangkat apa pun yang melayani sebagian model: mesin GPU, mesin CPU, perangkat NPU, atau ponsel. Node hanya memegang bagiannya — jendela lapisan di ring, atau irisan pakar di swarm — dan memperoleh KVR yang dibobot persis sesuai kerja yang dilakukan. Armada langsung mencampur AMD MI250, NVIDIA GB10 dan RTX Pro 6000, sebuah MacBook, mesin CPU x86 Windows, dan node seluler dalam satu jaringan.",
      },
      { t: "h2", kick: "Dari unduhan sampai pencairan", text: "Siklus hidup sebuah node" },
      {
        t: "code",
        code: `join      → slot bind / unit import / agent /control/join
report    → capabilities: backend · VRAM/RAM/CPU budgets · monitoring
plan      → planner assigns a layer window (or expert range)
download  → partial shard: only the tensors that window needs
serve     → run its share; pass boundaries / answer dispatch
earn      → units × layer_share × perf_tier → owner wallet`,
      },
      {
        t: "ul",
        items: [
          "**Node komputasi** memperoleh per unit kontribusi, dibobot porsi lapisan dan diskalakan tingkat performa — tanpa perlu staking.",
          "Node terdaftar di bawah dompet pemiliknya; imbalan diselesaikan ke dompet itu, secara non-kustodial. Empat dompet pemilik berbeda yang masing-masing memperoleh porsi lapisannya telah diverifikasi ujung-ke-ujung.",
          "Data kapabilitas (backend, presisi akumulasi, anggaran sumber daya) menentukan apa yang boleh ditempatkan planner di sebuah node — dan, di swarm, rank mana yang boleh dilayaninya.",
          "Node yang tak dapat menyediakan pemantauan sumber daya dikecualikan dari pemuatan adaptif alih-alih dipercaya membabi buta.",
        ],
      },
    ],
  },
  "relay-443": {
    title: "Relay 443",
    summary: "Bidang data untuk perangkat di balik NAT: kedua ujung menelepon keluar melalui jembatan WebSocket di port 443.",
    blocks: [
      {
        t: "p",
        md: "Ponsel di balik NAT operator tak bisa menerima koneksi masuk, dan edge seperti Cloudflare hanya meloloskan port 80/443. **Relay 443** menyelesaikan keduanya: jembatan WebSocket per edge dengan **preamble peran 1 byte** membuat kedua sisi menelepon **keluar**, sehingga ponsel ikut serta di bidang data sambil membuka **nol port masuk**.",
      },
      {
        t: "code",
        caption: "Dua koneksi keluar bertemu di tengah; preamble memberi tahu siapa siapa.",
        code: `phone   ──outbound──▶ wss://edge:443  ◀──outbound── backbone
                     [role byte: worker]   [role byte: dialer]
        bridge splices the two streams → one ordinary TCP pipe`,
      },
      { t: "h2", kick: "Ditempa di produksi", text: "Tiga bug nyata, tiga perbaikan" },
      {
        t: "ul",
        items: [
          "**Kesepakatan sidik jari build** — kedua ujung harus membuktikan menjalankan runtime pack yang sama sebelum satu byte tensor pun mengalir.",
          "**Autentikasi unduhan node-token** — unduhan shard parsial diautentikasi dengan node token turunan dompet yang sudah dimiliki aplikasi.",
          "**Macet frame `Int.ushr`** — `ushr` Kotlin hanya memakai 5 bit terendah dari shift, sehingga `len ushr 56` menjadi `len ushr 24` dan diam-diam merusak setiap frame ≥ 64 KiB (`result_output` 593 KB adalah korban pertama). Diperbaiki dengan memindahkan pengemasan panjang ke shift `Long` — perbaikan penopang untuk dispatch pakar berkelompok yang rutin melampaui 64 KiB.",
        ],
      },
      {
        t: "p",
        md: "Relay membawa apa pun yang dibutuhkan topologi — batas lapisan ring atau stream dispatch pakar — dan mekanisme yang sama yang terverifikasi untuk ring itulah yang dipakai worker ponsel produksi di swarm.",
      },
      {
        t: "p",
        md: "Kedua upgrade `/api/expert-relay` dan `/api/ring-relay` **disambung mentah**: gateway meneruskan frame WebSocket byte demi byte tanpa mengurainya, sehingga relay tetap menjadi pipa tipis yang agnostik-model. Ia tetap **mengukur byte yang dijembataninya per sesi**, dan kerja terukur itu mengalir ke buku besar kontribusi hub lalu diselesaikan ke dompet worker itu sendiri dalam **KVR** — me-relay untuk ponsel di balik NAT memperoleh persis seperti node yang terhubung langsung.",
      },
    ],
  },

  linkcpp: {
    title: "linkcpp",
    summary: "Bidang kendali bersumber tersedia (BSL) yang mengubah perangkat keras sehari-hari menjadi mesin inferensi terdistribusi.",
    blocks: [
      {
        t: "p",
        md: "**linkcpp** adalah mesin di balik Kvasir: bidang kendali di sekeliling bidang data RPC mesin inferensi yang menjalankan model AI besar di banyak GPU dan mesin memakai binari `ggml-rpc-server` / `llama-server` *standar*. Semua yang ditambahkannya adalah orkestrasi — penemuan GPU, slot node, perencanaan penempatan lapisan, peluncuran worker, dan gateway OpenAI/Anthropic.",
      },
      { t: "h2", kick: "Arsitektur", text: "Satu hub, worker standar" },
      {
        t: "code",
        caption: "Jalur permintaan melalui deployment linkcpp.",
        code: `browser / SDK
  → hub :19000                      # FastAPI control plane (Docker)
  → GPU-less llama-server master    # per controller, :8080+
  → ggml-rpc-server workers         # slots :50052-50056 · units · agents`,
      },
      {
        t: "ul",
        items: [
          "**Sumber tersedia di bawah BSL** — baca, jalankan, dan bangun di atasnya secara gratis untuk pengembangan dan pengujian; penggunaan produksi memerlukan lisensi.",
          "Bidang data mesin inferensi tetap **tanpa fork** (kecuali satu patch GPU-over-RPC seluler yang dipatok), sehingga peningkatan performa dari upstream terus mengalir.",
          "Dikirim sebagai **satu image Docker**: hub FastAPI plus dua binari mesin inferensi terpanggang di dalamnya; node worker native dibangun di luar Docker untuk CUDA/Metal/Vulkan/CPU.",
        ],
      },
      { t: "h2", kick: "Planner", text: "Metadata GGUF masuk, penempatan keluar" },
      {
        t: "p",
        md: "Planner membaca metadata GGUF dan menghasilkan jendela lapisan bersambung per node, `--tensor-split` yang sesuai, dan perkiraan VRAM KV-cache / lapisan / pakar per node — plus offload opsional FFN pakar MoE ke RAM node, dikeluarkan sebagai aturan `-ot` mesin inferensi (mis. `blk\\.38\\.ffn_(up|down|gate)_(ch|)exps=CPU`) dan dibawa ke peluncuran via `--override-tensor`. Rencana yang tak muat dilaporkan **infeasible** sebelum apa pun dimuat, bukan ditemukan sebagai OOM saat runtime.",
      },
      {
        t: "p",
        md: "Kompatibilitas runtime adalah konsep kelas satu: protokol, runtime-pack, revisi mesin inferensi, dan ABI RPC diverifikasi, dan ketidakcocokan diblokir keras sebelum bind, plan, load, atau inferensi apa pun.",
      },
    ],
  },
  "ring-runtime": {
    title: "Ring runtime",
    summary: "Inferensi pipeline tanpa master: setiap perangkat menjalankan jendela lapisannya dan hanya meneruskan batas ke tetangganya.",
    blocks: [
      {
        t: "p",
        md: "**Ring runtime** adalah topologi penyajian latensi-rendah Kvasir. Setiap perangkat hanya memuat **jendela lapisan** bersambungnya, lalu membuka tepat dua tautan — pendahulu dan penerus. Batas hidden-state beredar mengelilingi ring; rank terakhir menyampel token dan mengembalikannya. **Tanpa master pusat, dan tak ada node yang memegang seluruh model.**",
      },
      { t: "h2", kick: "Mengapa bukan bintang", text: "Masalah master RPC" },
      {
        t: "p",
        md: "Pada topologi RPC klasik, satu master membuka **GGUF utuh** dan menelepon setiap worker. Itu rusak di jaringan terbuka dalam tiga hal: master harus memegang dan melayani seluruh checkpoint; setiap worker harus bisa ditelepon — ponsel di balik NAT operator tidak bisa; dan master menjadi pemilik tunggal di jaringan yang seharusnya tak punya pemilik. Ring menghapus ketiganya: tiap stage memiliki jendelanya, koneksi berjalan tetangga-ke-tetangga, dan relay membuat perangkat NAT terjangkau.",
      },
      {
        t: "code",
        caption: "Satu langkah dekode mengelilingi ring 4-stage.",
        code: `token n:  stage A (layers 0-14)  ──h──▶  stage B (15-26)
                                             │h
          stage D (37-48) ◀──h──  stage C (27-36)
          └─ samples token n, sends it around → client`,
      },
      {
        t: "ul",
        items: [
          "Penempatan berasal dari **rank manifest** planner — mis. 49 lapisan Qwen3.5-122B terbagi ke GPU, CPU, NPU, dan ponsel.",
          "Batasnya kecil (satu vektor hidden-state per token), jadi lompatan tetap murah bahkan di tautan lemah.",
          "GPU seluler menjalankan stage ring **langsung** (Adreno via OpenCL) — jalur RPC ke GPU ponsel terbukti tak layak karena tata letak buffer Adreno tak selamat dari serialisasi RPC, tetapi stage lokal memiliki backend-nya sendiri, jadi hanya batas yang menyeberangi kabel.",
        ],
      },
      {
        t: "p",
        md: "Ring adalah jalur **latensi**; lantainya adalah granularitas lapisan (~1.4 GB pada 122B). Swarm pakar menghapus lantai itu dan menyambung ke jalinan penyajian yang sama.",
      },
    ],
  },
  "layer-window": {
    title: "Jendela lapisan & shard parsial",
    summary: "Irisan model bersambung milik sebuah node — dapat diunduh sebagai mini-GGUF alih-alih checkpoint penuh.",
    blocks: [
      {
        t: "p",
        md: "**Jendela lapisan** adalah rentang bersambung lapisan transformer yang dilayani node ring. Node tak butuh checkpoint penuh untuk melayaninya — **mini-GGUF stage** hanya membawa tensor jendela itu: untuk 122B, **254 MB berisi 26 tensor** (dari total 338) dibanding model penuh 77.6 GB, atau ~1.5 GB untuk jendela satu-lapisan di ponsel.",
      },
      {
        t: "code",
        caption: "Satu baris rank manifest: siapa melayani apa, dalam anggaran berapa.",
        code: `rank 3  layers [39,48]  vram=3.4GiB  kv=0.9GiB  backend=opencl
shard: mini-GGUF with exactly those blk.39-48 tensors → download → load`,
      },
      { t: "h2", kick: "Diklaim, bukan ditugaskan", text: "Pendaftaran mandiri" },
      {
        t: "ul",
        items: [
          "Node mem-poll **peta cakupan/permintaan** untuk melihat jendela mana yang kurang terlayani dan berapa bayaran masing-masing.",
          "Ia memilih jendela tak tercakup **berimbalan tertinggi** yang muat di anggarannya, mengunduh persis itu, lalu bergabung.",
          "Cakupan memulihkan diri: saat sebuah node pergi, jendelanya kembali langka — dan karenanya kembali menguntungkan.",
          "Terverifikasi ujung-ke-ujung dengan ponsel di balik NAT: poll → daftar mandiri → unduh parsial → muat GPU Adreno → inferensi ring tuntas, kontribusi tercatat.",
        ],
      },
      {
        t: "p",
        md: "Swarm pakar memakai ulang pasar yang persis sama ini pada butiran lebih halus **(lapisan, rentang pakar)** — peta yang sama, pendaftaran mandiri yang sama, imbalan yang sama, unit yang lebih kecil.",
      },
    ],
  },
  moe: {
    title: "Mixture of Experts (MoE)",
    summary: "Model yang FFN-nya adalah ratusan pakar independen, dan hanya beberapa yang menyala per token.",
    blocks: [
      {
        t: "p",
        md: "Model **Mixture-of-Experts** mengganti FFN tunggal tiap lapisan dengan bank FFN pakar independen plus **router** yang memilih beberapa per token. Qwen3.5-122B-A10B adalah contoh andalan jaringan:",
      },
      {
        t: "stats",
        items: [
          { n: "49", l: "lapisan" },
          { n: "256", l: "pakar / lapisan" },
          { n: "8", l: "aktif / token" },
          { n: "12,544", l: "total pakar" },
          { n: "5.3 MB", l: "satu pakar (Q4)" },
          { n: "86%", l: "bobot pada pakar" },
          { n: "3072", l: "n_embd" },
          { n: "77.6 GB", l: "checkpoint penuh" },
        ],
      },
      {
        t: "p",
        md: "Setiap lapisan terbagi menjadi **jalur padat** — attention + KV, norm, router (`ffn_gate_inp`), pakar bersama — dan **bank pakar** yang disimpan sebagai tiga tensor bertumpuk (`ffn_up_exps`, `ffn_gate_exps`, `ffn_down_exps`). Jalur padat adalah minoritas byte; bank pakar adalah 86% model.",
      },
      {
        t: "ul",
        items: [
          "Indeks pakar adalah **dimensi terluar GGUF**, jadi tiap pakar adalah lempengan bersambung yang selaras blok-kuantisasi — ekstraksinya salinan rentang byte, tanpa dekuantisasi.",
          "Per token hanya **8 dari 256** pakar menyala per lapisan, jadi lalu lintas pakar sebuah lapisan saat dekode hanyalah segelintir perkalian matriks kecil atas satu vektor hidden (~6 KB dispatch).",
          "Para pakar saling independen — kepemilikan dapat disebar antar perangkat dan diseimbangkan ulang dengan bebas.",
        ],
      },
      {
        t: "p",
        md: "Inilah mengapa MoE adalah substrat alami swarm: bobotnya sudah terkemas dalam unit seukuran perangkat yang dapat dimiliki secara independen.",
      },
    ],
  },
  "expert-sharding": {
    title: "Sharding pakar",
    summary: "Membelah MoE pada butiran pakar, sehingga ponsel membawa 42–340 MB pakar alih-alih lapisan 1.4 GB.",
    blocks: [
      {
        t: "p",
        md: "**Sharding pakar** menurunkan unit angkut swarm dari lapisan (~1.4 GB pada 122B) ke pakar (**5.3 MB**). Perangkat lemah mengunduh irisan 8–64 pakar (**42–340 MB**), memuatnya sebagai worker fungsi-murni — tanpa attention, tanpa KV, tanpa sampler — dan menghitung pakarnya kapan pun router backbone memilihnya.",
      },
      { t: "h2", kick: "Dua peran", text: "Backbone × worker" },
      {
        t: "code",
        caption: "Titik potong di dalam satu lapisan MoE (router berjalan sekali, di backbone).",
        code: `cur   = ffn_norm(x)                     # backbone
ids,p = top_k(softmax(cur @ router), 8) # backbone — authoritative
send  (cur rows, local_ids) → worker    # ~6 KB per decode step
recv  expert_out            ← worker    # worker: 3 mat-muls
x = x + combine(p, partials) + shared(cur)   # backbone — exact`,
      },
      {
        t: "ul",
        items: [
          "**Backbone** memegang jalur padat (attention, norm, router, pakar bersama, combine) dan menyimpan semua pakar sebagai replika cadangan ter-offload ke RAM demi toleransi churn.",
          "**Worker** (`linkcpp-expert-worker --serve`) menjawab `(n_used, n_tokens, cur, sel) → experts` lewat satu stream TCP berumur panjang — stream yang sama yang diterowongkan relay 443 untuk ponsel.",
          "Cakupan memulihkan diri lewat **pasar cakupan pakar**: `POST /api/expert-coverage` mengirim heartbeat kepemilikan, `GET /api/expert-demand` mengagregasi kelangkaan, `POST /api/expert-volunteer` menetapkan rentang paling langka yang dipangkas sesuai anggaran node.",
        ],
      },
      { t: "h2", kick: "Diukur, bukan dijanjikan", text: "Terverifikasi di perangkat nyata" },
      {
        t: "ul",
        items: [
          "Komputasi sharded == monolitik hingga **max|Δ| = 3.6e-12** (pengelompokan ulang eksak, bukan aproksimasi).",
          "Dispatch lintas-proses pada dekode 122B langsung: **argmax MATCH**, cosine logit 0.99869 — identik byte demi byte dengan in-process.",
          "Sebuah Galaxy S25 mengunduh mandiri irisannya sebesar 1.58 GB dan menghitung pakar layer-0 di tiap token: **8/8 token identik** dengan run lokal.",
          "Sebuah GPU jarak jauh melalui internet publik — satu perjalanan bolak-balik WAN per token — tetap **greedy 8/8 identik** (cosine 0.99773): **overhead throughput 1.2%** pada tautan langsung, ~28% melalui edge CDN. Biaya jujur dari dispatch serial per-token, dan mengapa tuas jalinan adalah pengelompokan, bukan latensi lebih rendah.",
          "Dispatch berkelompok mencapai **53k tok/s per worker** pada batch 512 (ROCm) — sifat jalinan-throughput yang membuat swarm praktis.",
        ],
      },
    ],
  },
  "router-authority": {
    title: "Otoritas router",
    summary: "Invarian koherensi swarm: perutean diputuskan sekali, di backbone — worker hanya menerima id pakar.",
    blocks: [
      {
        t: "callout",
        md: "**Invariannya:** satu-satunya keputusan diskret dalam jaringan adalah perutean MoE (top-8 dari 256). Kvasir menjalankan router **tepat sekali, di backbone**, dan hanya mengirim id pakar terpilih ke worker. Swarm heterogen boleh sedikit berbeda pada *besaran* keluaran tiap pakar — tetapi tak pernah berbeda pada *pakar mana yang berjalan*.",
      },
      {
        t: "p",
        md: "Tanpa aturan ini, tiap backend akan menjalankan ulang router dan memilih **pakar yang berbeda** pada token perbatasan — divergensi katastrofik yang nyata, karena sejak token itu komputasi bercabang seperti dengan seed acak lain. Dengannya, perbedaan perangkat keras mengecil menjadi galat kontinu terbatas yang diserap combine berbobot probabilitas.",
      },
      { t: "h2", kick: "Apa yang dicegahnya", text: "Mode divergensi yang ditutup satu titik keputusan" },
      {
        t: "table",
        head: ["Mode divergensi", "Tanpa otoritas", "Dengan otoritas"],
        rows: [
          ["Ketidakcocokan perutean", "Backend memilih top-8 berbeda di perbatasan", "Id diputuskan sekali, dikirim ke pemiliknya"],
          ["Percabangan lintasan", "Satu token terbalik mencabangkan seluruh urutan", "Dekode/sampling dipatok ke satu node"],
          ["Verifikasi", "Perbandingan bit antar backend (mustahil)", "Pemeriksaan toleransi pada residu terdefinisi baik"],
        ],
      },
      {
        t: "p",
        md: "Biayanya dapat diabaikan: backbone toh sudah menghitung `ffn_norm` dan logit router; yang menyeberangi kabel hanyalah baris hidden plus id terpilih — sekitar **6 KB per langkah dekode**.",
      },
    ],
  },
  "numerical-equivalence": {
    title: "Ekuivalensi numerik",
    summary: "Backend berbeda tak pernah sepakat bit-demi-bit; swarm memperlakukan toleransi terukur sebagai kontrak kelas satu.",
    blocks: [
      {
        t: "p",
        md: "CUDA, ROCm, Adreno, dan CPU menghitung operasi yang sama dengan urutan reduksi, fusi FMA, akumulator, dan aproksimasi fungsi transendental yang berbeda — hasilnya berbeda ~1e-6…1e-3 per operasi, **memang begitu desainnya, tak pernah bit-identik**. Swarm yang tersusun dari perangkat keras apa pun yang datang tak bisa menuntut ketepatan bit, maka Kvasir mengukur ekuivalensi sebagai gantinya.",
      },
      {
        t: "table",
        head: ["Pasangan backend (122B nyata, pakar layer-0)", "max|Δ|", "cosine"],
        rows: [
          ["CUDA (GB10 Blackwell) vs ROCm (MI250)", "3.5e-10", "1.0000000000"],
          ["ROCm (MI250) vs numpy (x86)", "7.9e-7", "0.99996"],
          ["ARM CPU ponsel vs numpy (x86)", "1.4e-6", "0.99992"],
          ["CUDA (GB10 Blackwell) vs Grace ARM CPU", "2.6e-5", "0.99975"],
        ],
      },
      {
        t: "p",
        md: "Matriks backend lengkap telah ditutup: dua backend GPU (CUDA, ROCm) berbagi sumber kernel dan mendarat **praktis bit-identik** (cosine 1.0000000000), sementara pasangan GPU↔CPU tetap ekuivalen pada ~0.9997. Worker CUDA dan worker ROCm dapat dipertukarkan; worker GPU dan worker CPU ekuivalen secara numerik.",
      },
      { t: "h2", kick: "Mengapa berbeda", text: "Penjumlahan floating-point tidak asosiatif" },
      {
        t: "ul",
        items: [
          "**Urutan reduksi matmul** — tensor core, tile MFMA, workgroup OpenCL, dan lane SIMD mengakumulasi dalam urutan berbeda.",
          "**Presisi akumulasi** — penyimpanan F16/BF16 dengan akumulator F32 vs F16: tuas terbesar divergensi.",
          "**Aproksimasi transendental** — exp (softmax), silu (swiglu), dan rsqrt (norm) memakai varian polinomial/tabel yang berbeda per backend.",
        ],
      },
      { t: "h2", kick: "Kontraknya", text: "Toleransi, kapabilitas, otoritas tunggal" },
      {
        t: "ul",
        items: [
          "Verifikasi adalah **toleransi** — \"kesepakatan top-1 ≥ 99.x%, KL ≤ ε\" — tak pernah kesetaraan bit.",
          "Backend dan presisi akumulasi diiklankan sebagai **kapabilitas** node; node berakumulasi F32 diutamakan untuk rank yang sensitif keluaran.",
          "Node di luar toleransi ditandai tak layak untuk rank sensitif, bukan ditolak mentah-mentah.",
          "Keputusan diskret (perutean, sampling) dipatok pada otoritas tunggal agar galat kontinu tak pernah menjadi divergensi diskret.",
        ],
      },
    ],
  },
  gguf: {
    title: "GGUF",
    summary: "Format berkas model terkuantisasi yang dipakai mesin inferensi — dan tata letak yang membuat pengirisan parsial dan pakar jadi murah.",
    blocks: [
      {
        t: "p",
        md: "**GGUF** adalah format model satu-berkas dari ekosistem mesin inferensi: metadata (arsitektur, jumlah lapisan, dimensi, kuantisasi) plus tensor sebagai byte terkuantisasi mentah (mis. Q4_K_M). Planner linkcpp membaca metadata untuk menghitung penempatan dan perkiraan ukuran; sisi penyajian mengiris byte tensor untuk menghasilkan unduhan.",
      },
      {
        t: "ul",
        items: [
          "**Mini-GGUF stage** membawa tensor satu jendela lapisan — 254 MB alih-alih 77.6 GB untuk stage ring 122B.",
          "**GGUF shard pakar** membawa satu irisan (lapisan, rentang pakar), dilayani `GET /api/proxy/models/{m}/expert-shard?layers=0:2&experts=0:16` dengan autentikasi node-token.",
          "Keduanya **berkas GGUF yang valid**: pembaca di node memuatnya dengan perkakas standar, tanpa format khusus.",
        ],
      },
      {
        t: "code",
        caption: "Mengapa pengirisan pakar hanyalah salinan byte: indeks pakar adalah dimensi terluar.",
        code: `tensor ffn_up_exps: ne = [n_ff, n_embd, 256]   # 256 = experts, outermost
expert e occupies rows [e·slab : (e+1)·slab)    # quant-block aligned
sliced = tensor.data[a:b]                       # no dequant, no re-pack
writer.add_tensor(name, sliced, raw_dtype=tensor.tensor_type)`,
      },
      {
        t: "p",
        md: "Router (`ffn_gate_inp`) dan pakar bersama **dikecualikan** dari shard pakar — keduanya milik backbone, persis seperti yang dituntut otoritas router.",
      },
    ],
  },

  kvr: {
    title: "KVR",
    summary: "Token utilitas jaringan: pengembang membelanjakannya untuk inferensi, kontributor memperolehnya dari komputasi.",
    blocks: [
      {
        t: "p",
        md: "**KVR** (nama on-chain \"Kvasir\", 6 desimal, Solana) adalah satu token yang mengalir dua arah: pengembang **membelanjakan** KVR untuk menjalankan inferensi lewat gateway, kontributor **memperoleh** KVR untuk komputasi yang disediakan node mereka. Imbalan dihitung dari kerja nyata — lapisan dan pakar yang benar-benar dilayani — bukan dari sekadar partisipasi.",
      },
      {
        t: "ul",
        items: [
          "**Sisi belanja** — bayar per inferensi lewat gateway: quote → payment → inference.",
          "**Sisi perolehan** — unit kontribusi × porsi lapisan × tingkat performa untuk komputasi; uptime per jam untuk peran hub/gateway.",
          "**Penyelesaian** — di Solana, ke dompet milik masing-masing pemilik node; layanan penyelesaian mengkreditkan setiap node yang menyentuh sebuah permintaan.",
          "**Dinamai dari mitos** — Madu Puisi, diseduh dari Kvasir, memberi kebijaksanaan kepada siapa pun yang meminumnya: akses terbuka, dan imbalan bagi semua yang ikut menuang.",
        ],
      },
      {
        t: "callout",
        md: "**Devnet, token utilitas.** KVR saat ini berjalan di Solana devnet dan merupakan token utilitas / kontribusi — bukan aset yang dapat diperdagangkan, harga, atau investasi. Tidak ada di sini yang merupakan nasihat keuangan atau janji imbal hasil.",
      },
    ],
  },
  "contribution-units": {
    title: "Unit kontribusi",
    summary: "Rumus imbalan: unit mengikuti token yang dilayani berbobot porsi lapisan, lalu diskalakan tingkat performa.",
    blocks: [
      {
        t: "code",
        caption: "Bagaimana imbalan komputasi dihitung.",
        code: `units    += (tokens / 1k) × (node_layers / total_layers)
effective = units × perf_tier × gateway_bonus
infra      : hub uptime/hr > gateway uptime/hr  (summed on top)`,
      },
      {
        t: "p",
        md: "Satu **unit** ≈ 1k token yang dilayani, dibobot **porsi lapisan** node pada tiap inferensi — node yang menjalankan 12 dari 49 lapisan memperoleh 12/49 unit tiap inferensi. Pengali tingkat lalu mengganjar kecepatan terukur, dan peran infrastruktur menumpuk uptime per jam di atasnya.",
      },
      { t: "h2", kick: "Contoh hitung", text: "Satu inferensi, empat node" },
      {
        t: "table",
        head: ["Node", "Lapisan", "Porsi", "Tingkat", "Unit efektif / 1k token"],
        rows: [
          ["GPU", "15 / 49", "0.306", "S ×1.5", "0.459"],
          ["CPU", "12 / 49", "0.245", "B ×1.0", "0.245"],
          ["NPU", "12 / 49", "0.245", "B ×1.0", "0.245"],
          ["Ponsel", "10 / 49", "0.204", "C ×0.7", "0.143"],
        ],
      },
      {
        t: "ul",
        items: [
          "Imbalan mengikuti **kerja nyata**: node yang tak melayani apa pun tak memperoleh apa pun, berapa pun uptime-nya (untuk peran komputasi).",
          "Peran **bertumpuk** — satu mesin bisa jadi komputasi + gateway + hub, dan alirannya dijumlahkan.",
          "Semuanya diselesaikan dalam KVR ke dompet pemilik node; dasbor menampilkan mentah × tingkat = efektif dan saldo yang dapat diklaim.",
        ],
      },
    ],
  },
  "performance-tiers": {
    title: "Tingkat performa",
    summary: "Throughput terukur menetapkan pengali: S ×1.5 · A ×1.25 · B ×1.0 · C ×0.7.",
    blocks: [
      {
        t: "table",
        head: ["Tingkat", "Throughput terukur", "Pengali"],
        rows: [
          ["S", "≥ 90 tok/s", "×1.5"],
          ["A", "≥ 60 tok/s", "×1.25"],
          ["B", "≥ 30 tok/s", "×1.0"],
          ["C", "< 30 tok/s", "×0.7"],
        ],
      },
      {
        t: "p",
        md: "Kecepatan dekode terukur sebuah node menetapkan tingkatnya, dan tingkat mengalikan unit yang diperolehnya — perangkat keras lebih cepat memperoleh lebih banyak secara proporsional untuk kerja yang sama. Tampilan status node di dompet menunjukkan tingkat tiap node di samping kontribusinya.",
      },
      {
        t: "ul",
        items: [
          "Tingkat itu **diukur, bukan diakui sendiri** — throughput berasal dari performa penyajian nyata node dan diukur ulang seiring waktu.",
          "Ponsel tingkat C tetap memperoleh — ×0.7 dari porsi lapisannya — dan itulah intinya: lantainya berlaku untuk semua partisipasi, bukan hanya pusat data.",
          "Tingkat mengalikan unit *efektif*, jadi ia berkomposisi dengan porsi lapisan dan bonus gateway alih-alih menggantikannya.",
        ],
      },
    ],
  },
  staking: {
    title: "Staking",
    summary: "Stake KVR untuk memperoleh bunga APR; 100.000 KVR yang di-stake membuat dompet layak mengoperasikan node hub atau gateway.",
    blocks: [
      {
        t: "p",
        md: "Staking mengunci KVR di dompet Anda sendiri untuk memperoleh **bunga APR** dan memenuhi syarat imbalan node. Mengoperasikan node **hub** atau **gateway** memerlukan stake **100.000 KVR**; node komputasi biasa bergabung tanpa stake apa pun dan memperoleh untuk lapisan yang dijalankannya.",
      },
      {
        t: "ul",
        items: [
          "Staking dilakukan di panel staking dasbor dompet: masukkan jumlah, **Stake**, dan posisi mulai mengakumulasi APR plus kelayakan imbalan node.",
          "Syarat 100k adalah **filter komitmen nyata** bagi dua peran yang menjadi sandaran lalu lintas orang lain — titik masuk dan bidang kendali.",
          "Staking bersifat non-kustodial seperti yang lain: posisinya hidup di dompet Anda sendiri, dan pokok, bunga terkumpul, serta imbalan node semuanya terlihat di panel staking.",
          "KVR devnet untuk staking datang lewat distribusi atau swap (swap SOL/ETH ↔ KVR: segera hadir); SOL devnet untuk biaya berasal dari faucet publik.",
        ],
      },
    ],
  },
  "non-custodial-wallet": {
    title: "Dompet non-kustodial",
    summary: "Kunci hanya hidup di perangkat pengguna — web, desktop, iOS, dan Android — dan imbalan diselesaikan langsung ke sana.",
    blocks: [
      {
        t: "p",
        md: "Kvasir Wallet **non-kustodial sejak desain**: frasa pemulihan 12 kata dan kunci hanya disimpan di perangkat milik pengguna, tak pernah pada operator. Imbalan diselesaikan di Solana langsung ke dompet pemilik tiap node — terverifikasi pada empat dompet pemilik berbeda, masing-masing memperoleh porsi lapisannya sendiri.",
      },
      {
        t: "ul",
        items: [
          "**Platform** — web, desktop (macOS/Windows/Linux, dompet + node dalam satu aplikasi Electron), iOS, dan Android.",
          "**Satu frasa, semua perangkat** — frasa 12 kata yang sama memulihkan akun yang sama di desktop, ponsel, dan web; passphrase lokal membuka tiap instalasi.",
          "**Dompet = identitas node** — dompet menandatangani identitas jaringan node, jadi \"siapa yang memperoleh untuk perangkat ini\" bersifat kriptografis, bukan baris akun di server seseorang.",
          "**Kehilangan frasa berarti kehilangan akun** — non-kustodial memotong dua arah; tak ada operator yang bisa meresetnya.",
        ],
      },
      {
        t: "p",
        md: "Aplikasi selulernya sekaligus aplikasi node: dompet yang sama yang menyimpan KVR Anda mengatur backend komputasi dan mode node ponsel, melakukan staking, dan mengklaim imbalan.",
      },
    ],
  },
  "siws-2fa": {
    title: "SIWS + 2FA",
    summary: "Login operator adalah tanda tangan dompet (Sign-In With Solana) atas nonce server, plus 2FA TOTP opsional.",
    blocks: [
      {
        t: "p",
        md: "Untuk deployment publik, akses operator ke hub dan gateway diautentikasi dengan **Sign-In With Solana**: dompet operator menandatangani nonce yang diterbitkan server, membuktikan kepemilikan tanpa kata sandi atau kredensial yang dititipkan. Di atasnya, **2FA TOTP** dan kode cadangan sekali-pakai melindungi sesi — di hub maupun gateway.",
      },
      {
        t: "ul",
        items: [
          "**Tak ada kata sandi di mana pun** — kunci dompet adalah identitasnya dan nonce mencegah replay; tak ada apa pun di sisi server yang bisa di-phishing atau bocor.",
          "**Pendaftaran TOTP per dompet** dipersistenkan di status hub, jadi 2FA bertahan melewati restart bersama slot dan binding.",
          "**Kode cadangan sekali pakai** — tiap kode terpakai saat login, untuk pemulihan saat perangkat autentikator tak tersedia.",
          "**Cakupan dinyatakan jujur** — hub polos dan port RPC dirancang untuk host tepercaya / LAN / VPN; SIWS + 2FA adalah lapisan yang membuat domain *publik* aman untuk diekspos.",
        ],
      },
    ],
  },
  "token-economy": {
    title: "Ekonomi KVR",
    summary: "Bagaimana biaya konsumen dan imbalan node membentuk satu lingkar yang menguatkan diri — siklus bajik yang membuat jaringan tumbuh lebih murah seiring ia tumbuh lebih besar.",
    blocks: [
      {
        t: "p",
        md: "Kvasir adalah **pasar dua sisi** yang diselesaikan dalam satu token. Konsumen membayar **KVR** per inferensi ke perbendaharaan; node memperoleh **KVR** untuk kerja persis yang mereka layani, dibayarkan kembali ke dompet mereka sendiri. Tujuan desainnya adalah agar kedua sisi ini tidak bersaing — melainkan **saling menggandakan**: lebih banyak pasokan membuat jaringan lebih murah dan lebih baik, yang menarik lebih banyak permintaan, yang pembayarannya mendanai imbalan lebih kaya, yang menarik lebih banyak pasokan.",
      },
      { t: "h2", kick: "Roda gila", text: "Pemakaian dan pasokan tumbuh bersama" },
      {
        t: "p",
        md: "Karena inferensi **harus** dibayar dalam KVR, setiap unit pemakaian adalah permintaan nyata atas token — utilitas, bukan spekulasi. Permintaan itu menopang nilai KVR yang diperoleh node, yang menjaga kontribusi tetap menarik, yang menumbuhkan kapasitas, yang menurunkan harga dan latensi, yang menarik lebih banyak pemakaian. Keunggulan paling tajam Kvasir mengetatkan lingkar itu lebih jauh lagi: seorang peserta bisa menjadi **konsumen dan pemasok sekaligus** (seorang *prosumer*), sehingga kedua sisi kerap tumbuh di dalam orang yang sama.",
      },
      {
        t: "callout",
        md: "**\"Gratis saat Anda berkontribusi\" bersifat gratis-bersih, bukan nol-biaya.** Anda membayar untuk apa yang Anda inferensikan dan memperoleh untuk apa yang Anda layani; berkontribusilah kira-kira sebanyak yang Anda konsumsi, maka keduanya saling meniadakan. Jaringannya tidak gratis — *tagihan Anda*-lah yang gratis.",
      },
      { t: "h2", kick: "Menjaganya tetap bajik", text: "Tiga invarian, dan spiral yang dicegahnya" },
      {
        t: "table",
        head: ["Invarian", "Spiral yang dicegah"],
        rows: [
          ["Imbalan didanai pendapatan nyata (emisi hanya untuk merintis, lalu meruncing)", "Inflasi menggerus KVR hingga kedua sisi runtuh"],
          ["KVR adalah medium wajib untuk inferensi", "Nilai token terlepas dari pemakaian menjadi spekulasi murni"],
          ["Harga mengambang antara lantai biaya dan langit-langit di bawah pasar", "Terlalu rendah membuat node kelaparan; terlalu tinggi kehilangan pengguna ke API terpusat"],
        ],
      },
      {
        t: "p",
        md: "Kvasir sudah mengganjar **kerja nyata** (KVR per token yang dilayani × porsi lapisan, bukan sekadar kehadiran) dan menyelesaikan secara non-kustodial, yang merupakan bagian tersulit dari membuat imbalan berbasis pendapatan menjadi jujur. Sisanya — harga yang digerakkan utilisasi dan peruncingan emisi→pendapatan — adalah peta jalan ekonomi yang mengubah \"lebih banyak node → lebih murah\" dari intuisi menjadi aturan yang ditegakkan protokol. Entri **Harga inferensi** membahas sisi harga; **Unit kontribusi** membahas bagaimana kerja menjadi imbalan.",
      },
    ],
  },
  "inference-pricing": {
    title: "Harga inferensi",
    summary: "Berapa biaya sebuah inferensi dalam KVR hari ini, mengapa jaringan terdesentralisasi lebih murah secara struktural, dan bagaimana harga dirancang turun seiring pasokan tumbuh.",
    blocks: [
      {
        t: "p",
        md: "Akses ke jaringan bersifat **bayar-per-inferensi**: gateway mengutip harga KVR untuk permintaan Anda, dompet Anda membayarnya di on-chain, dan barulah hub menjalankan model. Penetapan harga adalah rumus kecil yang transparan — lantai per permintaan plus tarif per token — dikutip di muka dan diselesaikan atas pemakaian token **sebenarnya** setelah generasi.",
      },
      {
        t: "code",
        caption: "Rumus penyelesaian — dikutip sebelumnya, ditagih atas pemakaian nyata sesudahnya.",
        code: `cost (KVR) = basePrice + total_tokens × perToken
# quote:  estimate with the model's nominal output length
# charge: recompute on the real prompt + completion tokens`,
      },
      { t: "h2", kick: "Mengapa bisa lebih murah", text: "Tak ada margin pusat yang harus dibayar" },
      {
        t: "p",
        md: "API terpusat menetapkan harga pada biaya **plus** margin besar dan pemulihan modal. Jaringan terdesentralisasi menetapkan harga mendekati **biaya marjinal** para kontributornya — listrik dan amortisasi perangkat keras — plus biaya protokol yang tipis. Celah struktural itu ada tanpa memandang ukuran. Pertumbuhan memperlebarnya: **sharding pakar** berarti lebih banyak node yang masing-masing memegang irisan lebih kecil, sehingga perangkat lebih murah pun bisa melayani, menurunkan biaya marjinal partisipasi dan memperdalam pasokan.",
      },
      {
        t: "callout",
        md: "**Harga diatur, bukan bebas sepenuhnya.** Tarif adalah parameter ekonomi yang sensitif, diubah hanya oleh dompet genesis di bawah tanda tangan dompet + 2FA — tak pernah oleh variabel lingkungan. Ini menjaga ekonomi token tetap stabil dan dapat diaudit.",
      },
      { t: "h2", kick: "Ke mana arahnya", text: "Harga yang digerakkan utilisasi" },
      {
        t: "p",
        md: "Arah desainnya adalah harga yang **mengambang bersama utilisasi jaringan** antara lantai (dijaga di atas biaya marjinal node, agar melayani tetap sepadan) dan langit-langit (dijaga di bawah alternatif terpusat, agar tetap kompetitif). Pasokan menganggur mendorong harga turun; kemacetan mendorongnya naik. Itulah mekanisme yang akhirnya membuat **\"lebih banyak node bersama → harga lebih rendah\"** benar dalam kode — termostat alami **ekonomi KVR**.",
      },
    ],
  },
  "run-expert-worker": {
    title: "Menjalankan worker pakar",
    summary: "Ubah GPU, CPU, atau ponsel cadangan menjadi worker pakar: bangun, jadi relawan untuk irisan paling langka, unduh, layani, dan menelepon keluar lewat 443 untuk memperoleh KVR.",
    blocks: [
      {
        t: "p",
        md: "**Worker pakar** adalah fungsi murni `(hidden, ids) → out` — tanpa attention, tanpa KV cache, tanpa sampler — yang menghitung irisan pakar sebuah model MoE kapan pun router backbone memilihnya. Anda tidak memilih apa yang dilayani; **pasar cakupan** memberi Anda rentang paling langka dan berimbalan tertinggi yang dipangkas sesuai anggaran Anda, sehingga ponsel 4 GB dan GPU pusat data sama-sama menemukan slot.",
      },
      { t: "h2", kick: "Tujuh langkah", text: "Bangun → jadi relawan → layani → telepon → peroleh" },
      {
        t: "code",
        caption: "Seluruh jalur — skrip dial menunggu backbone dengan loop coba-ulang.",
        code: `# 1. build the worker for your backend (cuda | rocm | cpu)
bash scripts/build-node-runtime.sh <backend>   # -> linkcpp-expert-worker

# 2. volunteer — the market assigns the scarcest range within your budget
POST /api/expert-volunteer {model, max_experts}     -> {layer, experts:[a,b]}

# 3. download only that slice (node-token auth)
GET  /api/proxy/models/<model>/expert-shard?layers=L:L+1&experts=a:b   # a mini-GGUF

# 4. serve it
linkcpp-expert-worker --model <slice.gguf> --serve <port> --layer L --n-embd <E>

# 5. dial out over 443 (no inbound ports)
scripts/expert-relay-dial.py --mode worker \\
  --hub wss://gate.kvasir-ai.net/api/expert-relay --session <S> --local 127.0.0.1:<port>

# 6. heartbeat so the demand map and rewards can see you
POST /api/expert-coverage`,
      },
      {
        t: "ul",
        items: [
          "**Irisannya mungil.** Irisan layer-0 berisi 128 pakar adalah **794 MB** melawan model penuh 72 GB — butiran yang memungkinkan perangkat lemah ikut serta. Anda hanya mengunduh rentang yang ditetapkan pasar.",
          "**Menelepon keluar, tak pernah masuk.** Langkah 5 membuka satu WebSocket keluar di 443, sehingga NAT operator dan edge CDN meloloskannya dan Anda mengekspos nol port masuk — jalur yang sama yang dipakai ponsel.",
          "**Heartbeat itu penopang.** Tanpa `POST /api/expert-coverage` Anda tak melayani apa pun yang diketahui peta permintaan, dan tak ada yang Anda lakukan yang dikreditkan.",
          "**Imbalan per kerja.** Kerja yang dijembatani terakumulasi ke buku besar kontribusi hub; gateway meng-delta-kredit KVR ke dompet **milik Anda sendiri** (non-kustodial). Anda perlu alamat dompet untuk dibayar.",
        ],
      },
      {
        t: "callout",
        md: "Worker berbicara dengan protokol dispatch yang sama seperti GPU di dalam pusat data — `(n_used, n_tokens, cur, sel) → experts` lewat satu stream berumur panjang. Worker shard-parsial cukup menyetel `n_used = 1`. Keseragaman itulah alasan ponsel, kotak CPU, dan kartu Blackwell menjadi anggota yang dapat dipertukarkan dari swarm yang sama.",
      },
    ],
  },
  "hub-operations": {
    title: "Mengoperasikan hub",
    summary: "Catatan operator untuk menjalankan hub dan gateway: hot-patch tanpa rebuild, selamat dari restart, jaga katalog tetap terdaftar, dan kunci permukaannya hingga tinggal 443.",
    blocks: [
      {
        t: "p",
        md: "Hub (bidang kendali) dan gateway (titik masuk publik) adalah dua layanan berumur panjang yang dijaga tetap sehat oleh operator. Hub dan port RPC-nya **tanpa autentikasi sejak desain** — hanya host tepercaya / LAN / VPN — dan seluruh lalu lintas publik memusat pada satu permukaan 443 gateway. Inilah catatan operasional yang menjaga pengaturan itu tetap stabil melewati perubahan kode, restart, dan reboot.",
      },
      { t: "h2", kick: "Deploy & hot-patch", text: "Ubah kode tanpa membangun ulang" },
      {
        t: "ul",
        items: [
          "**Jalur cepat:** perbarui kode hub/gateway dengan `docker cp <file> <container>:/app/...` + `docker restart` — tanpa rebuild image. Tetapi **menambah variabel lingkungan tak bisa dilakukan begini** (perlu recreate kontainer); lebih baik pakai API runtime-config yang mempersistenkannya ke hub-state.",
          "**Penyimpangan compose:** kontainer yang berjalan lama bisa menyimpang dari berkas compose-nya (mode jaringan, entrypoint, env). Selalu `docker inspect` konfigurasi sebenarnya sebelum recreate `docker compose up -d` — jika sudah menyimpang, recreate akan menghapus pengaturan produksi. Pakai cp + restart.",
          "**Diff sebelum menambal:** `docker cp` keluarkan berkas di dalam kontainer dan bandingkan (diff) dengan HEAD repo sebelum menggantinya, agar hot-patch dari sesi sebelumnya tak hilang diam-diam.",
        ],
      },
      { t: "h2", kick: "Selamat dari restart", text: "Status bertahan; model termuat tidak" },
      {
        t: "ul",
        items: [
          "Restart hub **menghentikan penyajian.** Slot, controller, dan binding pulih dari `hub-state.json`, tetapi model termuat hanya runtime. Setelah restart, baca `last_load` tiap controller dan picu ulang `POST /api/controllers/{cid}/serve` — bahkan model besar kembali dalam ~1 menit berkat page cache.",
          "**Watchdog gateway:** sondir tiap model yang dilayani dengan permintaan 1-token setiap 30 s dan muat-ulang otomatis dari `last_load` saat gagal (dengan cooldown). **Sondir *semua* model, bukan `catalog[0]`** — begitu model sehat dari hub lain terurut ke depan, sondir hanya-pertama akan melewatkan model besar yang sedang tumbang (bug nyata, sudah diperbaiki).",
          "**TTL katalog:** `POST /api/pay/hub/register` memiliki TTL 90 s, jadi jaga pendaftaran tetap hidup dengan loop heartbeat ~60 s, dibuat tahan-reboot dengan cron `@reboot` atau unit systemd.",
        ],
      },
      { t: "h2", kick: "Kunci rapat", text: "Semua yang publik lewat 443" },
      {
        t: "ul",
        items: [
          "Hub (:19000) dan port RPC mengasumsikan jaringan tepercaya; satu-satunya yang boleh menghadap internet adalah gateway di 443 (termasuk passthrough relay WebSocket-nya).",
          "Jika hub harus berada di IP publik, firewall-i ke IP tepercaya — tetapi port terbit Docker **di-DNAT sebelum rantai INPUT**, jadi aturan pada `dport` takkan cocok. Filter di rantai `DOCKER-USER` memakai port tujuan asli conntrack (`--ctorigdstport`) sebagai gantinya, dan persistenkan aturan dengan systemd oneshot yang diurutkan `After=docker.service`.",
        ],
      },
      { t: "h2", kick: "Penyelesaian & jebakan", text: "Pull, bukan push — dan satu jebakan shell" },
      {
        t: "ul",
        items: [
          "**Penyelesaian bersifat pull, bukan push:** hub mengakumulasi kontribusi; gateway mem-poll `GET /api/contributions` dan meng-delta-kredit KVR. Jika restart hub mereset penghitungnya, gateway mengatur ulang basis agar tak ada yang dibayar ganda. Tarif kerja pakar disetel oleh `LINKCPP_EXPERT_UNITS_PER_MB`.",
          "**Jebakan `pkill`:** `ssh host 'pkill -f X; ...'` mencocokkan baris perintahnya *sendiri* dan membunuh dirinya sendiri. Pakai kelas karakter dalam pola (`X[x]`), dan jangan pernah menaruh spawn dan pkill dalam satu perintah jarak jauh yang sama.",
        ],
      },
    ],
  },
  "hub-wan-interconnect": {
    title: "Interkoneksi WAN hub (optik 200G)",
    summary: "Bagaimana hub saling terhubung pada 200 Gb/s melintasi ruangan, kampus, atau kota: optik mana pada jarak mana, apa yang dicolok di mana, dan apa yang dibutuhkan untuk benar-benar mencapai line rate.",
    blocks: [
      {
        t: "p",
        md: "Ketika dua hub sama-sama punya rute publik, bidang data dispatch-pakar sebaiknya berupa **tautan langsung** — relay 443 untuk edge di balik NAT. Entri ini adalah resep konkret untuk menjadikan tautan langsung itu berkelas 200 Gb/s dengan komponen katalog. Satu aturan menata semuanya: **serat adalah kaca netral-kecepatan; kecepatan berada pada pluggable di tiap ujung.**",
      },
      { t: "h2", kick: "Langkah 1 · pilih berdasarkan jarak", text: "Tangga jangkauan" },
      {
        t: "table",
        head: ["jarak", "komponen", "dicolok ke"],
        rows: [
          ["same rack, 0.5–3 m", "QSFP56 DAC (passive copper)", "NIC ↔ NIC, tanpa switch"],
          ["same room, ≤30 m", "QSFP56 AOC (active optical)", "NIC ↔ NIC / switch"],
          ["campus, 2–10 km", "200G FR4 (2 km) / LR4 (10 km) module + duplex LC, single-mode fiber", "cage QSFP56 pada NIC atau switch"],
          ["metro, ≤40 km", "200G ER4 module, single-mode fiber", "cage QSFP56 pada NIC atau switch"],
          ["region, ≤120 km", "400G ZR+ coherent module set to a 200G line rate", "cage QSFP-DD switch/router (bukan NIC)"],
          ["long-haul, 100s of km", "carrier-leased 200G wavelength (or 2×100G) over DWDM", "switch Anda menyerahkan ke carrier"],
        ],
      },
      { t: "h2", kick: "Langkah 2 · apa dicolok di mana", text: "Sisi NIC vs sisi switch" },
      {
        t: "ul",
        items: [
          "**Sisi NIC** — kartu kelas ConnectX-6/7 mengekspos cage QSFP56; DAC/AOC/FR4/LR4/ER4 semuanya duduk langsung di NIC. Hub kelas GB10 sudah punya dua port QSFP 200 GbE di papan, jadi tautan dua-hub butuh tepat satu kabel dan nol perangkat keras baru.",
          "**Sisi switch** — optik koheren ZR+ berformat QSFP-DD dan berada di switch atau router; NIC hub lalu bergabung ke switch itu pada 200G lewat DAC pendek. Pakai tingkat ini saat hub jauh berjarak puluhan kilometer.",
          "**Serat itu sendiri** — pasangan LC dupleks single-mode standar (G.652), disewa sebagai dark fiber per untai. Kaca yang sama membawa 100G hari ini dan 400G nanti; peningkatan cukup tukar modul, tak pernah pekerjaan sipil.",
          "**Di luar ~120 km** — Anda berhenti membeli komponen dan mulai menyewa panjang gelombang dari carrier; batas demarkasinya adalah handoff Ethernet di switch Anda.",
        ],
      },
      {
        t: "code",
        caption: "Tiga rakitan acuan, termurah lebih dulu.",
        code: `two-hub bench   : hub A qsfp0 ──QSFP56 DAC 1m── hub B qsfp0
campus pair     : hub A [LR4] ──dark fiber, ≤10km── [LR4] hub B
metro federation: hub ──DAC── switch [ZR+ @200G] ──SMF ≤120km── [ZR+] switch ──DAC── hub`,
      },
      { t: "h2", kick: "Langkah 3 · benar-benar mencapai 200G", text: "Line rate adalah konfigurasi, bukan pembelian" },
      {
        t: "ul",
        items: [
          "Pakai **RDMA (RoCE)** untuk stream dispatch bila tersedia — host kelas GB10 mengumpani NIC lewat tautan PCIe terbelah, dan kecepatan penuh terukur (~185–190 Gb/s) muncul di bawah RoCE dengan topologi yang terpetakan benar; jalur yang salah-petakan terbatas mendekati separuh laju, dan TCP polos tanpa penyetelan mendarat jauh lebih rendah.",
          "Aktifkan **jumbo frame (MTU 9000)** ujung-ke-ujung dan pertahankan `TCP_NODELAY` pada soket dispatch (hub sudah menyetelnya).",
          "Bersiaplah untuk *memverifikasi*, bukan mengasumsikan: jalankan perftest antar-hub setelah tiap perubahan fisik — beda antara 95 dan 190 Gb/s tak terlihat sampai diukur.",
          "Jaga **relay 443 sebagai jalur cadangan** — kebijakan dial adalah langsung-dulu untuk peer publik, relay untuk NAT. Tugas relay adalah jangkauan, tugas tautan langsung adalah kecepatan.",
        ],
      },
      {
        t: "p",
        md: "Mengapa ini penting bagi arsitektur: latensi dekode dibatasi oleh waktu bolak-balik (~5 µs/km di serat — fisika, tak terpengaruh bandwidth), jadi pipa gemuk membeli **kecepatan prefill, throughput dispatch berkelompok, dan distribusi irisan-pakar nyaris seketika**, bukan latensi per-token yang lebih rendah. Itulah persis peran tingkat-hub dalam desain dua-tingkat: kapasitas di tingkat pipa-gemuk, jangkauan di tingkat relay.",
      },
    ],
  },
  "load-adaptive-scaling": {
    title: "Penskalaan adaptif-beban",
    summary: "Jalur penyajian MoE Kvasir tumbuh dan menyusut seiring lalu lintas: koordinator melibatkan kembali worker terbukti di bawah kejenuhan, dan hub merekrut node menganggur dengan menaikkan permintaan pakar — semuanya berbasis-tarik, sehingga perangkat ber-NAT pun ikut bergabung.",
    blocks: [
      {
        t: "p",
        md: "Jalur penyajian MoE Kvasir menskala secara elastis seiring beban, dalam dua lapisan yang saling bekerja sama. Saat sepi, koordinator melayani semuanya secara lokal demi jalur tercepat per token; saat jenuh, dua lapisan di bawah menumbuhkan swarm — dan menyusutkannya lagi saat lonjakan berlalu.",
      },
      { t: "h2", kick: "Lapisan 1", text: "Sisi koordinator: dispatch adaptif-beban" },
      {
        t: "p",
        md: "Koordinator backbone (sebuah `linkcpp-server` yang menjalankan model penuh) melayani pakar terutekan entah pada GPU-nya sendiri (cepat, lokal) atau dengan men-dispatch-nya ke worker jarak-jauh. Sebuah thread latar memutuskan yang mana, setiap beberapa detik:",
      },
      {
        t: "ul",
        items: [
          "Ia menjajaki slot inferensi **miliknya sendiri**. Ketika `busy >= saturation threshold` (default 2), koordinator sedang di bawah beban.",
          "Di bawah beban, jika sebuah worker yang **terbukti** aktif — yang `last_serve_ms > 0`, artinya benar-benar pernah menghitung pakar sebelumnya — koordinator terus men-dispatch kepadanya melewati batas-waktu-diam normal, mengutamakan throughput agregat ketimbang latensi per-token.",
          "Worker yang tersambung tetapi tak pernah melayani (ponsel yang menyambung ke relay namun tak pernah menghitung) **tidak** direkrut di bawah beban, karena men-dispatch kepadanya akan menggantikan jalur lokal yang cepat dengan fallback yang lambat. Worker baru tetap mendapat percobaan pertama lewat jendela tenggang singkat.",
          "Kueri-diri dibatasi waktu sehingga penjajakan yang macet tak pernah bisa memblokir dispatch.",
        ],
      },
      { t: "h2", kick: "Lapisan 2", text: "Sisi hub: rekrutmen adaptif-beban" },
      {
        t: "p",
        md: "Hub kontrol mengawasi setiap koordinator MoE dan menumbuhkan kumpulan worker saat diperlukan:",
      },
      {
        t: "ul",
        items: [
          "Sebuah loop latar menjajaki slot tiap koordinator dan mencatat kejenuhan per model.",
          "Selama sebuah model jenuh, **target replika-pakar efektifnya** dinaikkan (base + boost). Pasar cakupan lalu membaca pakar-pakar yang sudah tercakup sebagai langka kembali, dan sebuah model **tanpa** worker aktif di-seed dari metadata GGUF-nya (jumlah pakar) sehingga permintaan terlihat bahkan dari nol.",
          "Node yang menganggur menjajaki pasar permintaan (`/api/expert-volunteer`) dan diberi sepotong `(layer, expert-range)` untuk dilayani. Mereka mengunduh potongan itu, menyambung ke relay, dan mendaftarkan cakupan; hub secara otomatis mengaitkan mereka ke peta dispatch koordinator.",
          "Saat beban surut, target turun kembali dan permintaan lenyap, sehingga worker ekstra tak lagi di-dispatch dan menua keluar.",
        ],
      },
      {
        t: "callout",
        md: "Desainnya **berbasis-tarik**: node meminta pekerjaan alih-alih didorong, sehingga worker di balik NAT berpartisipasi tanpa konektivitas masuk. Sebuah node yang direkrut di Lapisan 2 yang mulai melayani menjadi worker yang **terbukti** yang lalu dipertahankan tetap terlibat oleh Lapisan 1 di bawah beban — kedua lapisan berpadu menjadi satu loop elastis.",
      },
      {
        t: "p",
        md: "**Observabilitas:** `GET /api/moe/recruitment` melaporkan busy/kejenuhan per-model serta target base vs efektif; `/api/expert-demand` membawa flag `recruiting`.",
      },
    ],
  },
};
