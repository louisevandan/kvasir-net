/* Bahasa Indonesia — terjemahan entri wiki. Struktur (slug, kategori, urutan
   blok, kode) mencerminkan persis entries.ts (sumber bahasa Inggris); istilah
   teknis dan pengenal (KVR, p4, bridge, GGUF, MoE, tok/s, dll.) dipertahankan
   apa adanya. Kerangka kepatuhan (devnet, token utilitas, non-kustodial) tetap
   utuh. */
import type { WikiTranslation } from "./entries";

export const idWiki: Record<string, WikiTranslation> = {
  "node-relay": {
    title: "Relay node",
    summary: "Alamat publik yang dipegang atas nama mesin yang tidak memilikinya, sehingga node di balik NAT dapat dijangkau tanpa membuka satu porta pun.",
    blocks: [
      { t: "p", md: "**Relay node** memberi mesin seorang kontributor sebuah alamat yang dapat dihubungi jaringan. p4 mengantarkan pekerjaan dengan membuka koneksi *ke* sebuah node, dan mesin rumahan di balik penerjemahan alamat jaringan tidak punya alamat semacam itu. Relay memegangnya secara publik, node memelihara satu koneksi keluar ke relay, dan pekerjaan yang dihubungi pada alamat publik itu turun melalui koneksi yang sudah dipegang node." },
      { t: "p", md: "Kedua ujung p4 tidak tahu bahwa relay itu ada. Penghubung melihat alamat biasa; agen node tetap terikat pada `127.0.0.1` dan tidak mendengarkan apa pun selain itu." },
      { t: "h2", text: "Mengapa terowongan, bukan porta yang diteruskan" },
      { t: "p", md: "p4 tidak membawa autentikasi apa pun: host mana pun yang menjangkau porta agen dapat mengirim `NODE_LOAD`, `NODE_UNLOAD`, atau `INSPECT`. Meneruskan porta router rumah ke sana akan memaparkan mesin kepada siapa pun yang menemukannya. Di balik relay, node tidak mendengarkan apa pun dan membuktikan dompet operator sebelum koneksinya membawa apa pun, dengan skema tanda tangan yang sama seperti gerbang penyelesaian: keterjangkauan dan autentikasi diselesaikan oleh mekanisme yang sama." },
      { t: "h2", text: "Yang tidak dilakukannya" },
      { t: "ul", items: [
        "**Tidak membaca lalu lintas.** Muatan lewat byte demi byte dan tidak pernah diurai, sehingga relay tidak dapat membedakan satu perintah dari yang lain — dan memang tidak boleh, karena memahami lalu lintas berarti mampu mengubahnya.",
        "**Tidak menjadwalkan.** Penempatan tetap milik rencana operator; bagi relay, node hanyalah sebuah alamat.",
        "**Bukan bukti kerja.** Byte yang melewati relay tidak mengatakan apa pun tentang inferensi yang dilakukan, dan tidak pernah dihitung sebagai kontribusi.",
      ] },
      { t: "h2", text: "Terkait" },
      { t: "p", md: "Lihat juga **agen p4**, proses yang dijalankan mesin kontributor, dan **operator node**, dompet yang diautentikasi relay sebelum memberikan alamat." },
    ],
  },
  "kvasir-network": {
    title: "Jaringan Kvasir",
    summary: "Jaringan inferensi AI terdesentralisasi (DePIN) tempat perangkat sehari-hari melayani model terbuka dan memperoleh KVR.",
    blocks: [
      {
        t: "p",
        md: "**Kvasir** adalah jaringan inferensi AI terdesentralisasi: model terbuka berukuran besar dibagi ke perangkat keras bersama dengan mesin **p4**, sehingga tidak ada satu node pun yang harus memegang seluruh model. Siapa pun dapat menyumbangkan GPU, CPU, NPU — bahkan ponsel — dan memperoleh **KVR** untuk lapisan atau pakar yang benar-benar dilayani perangkatnya. Pengembang mengakses jaringan lewat gateway yang kompatibel dengan OpenAI/Anthropic dan membayar per inferensi.",
      },
      {
        t: "ul",
        items: [
          "**Mesin bersumber tersedia** — p4 berlisensi Business Source License 1.1 (penggunaan internal yang tidak dimonetisasi diizinkan; penggunaan yang di-hosting atau menghasilkan pendapatan memerlukan lisensi komersial); bidang data llama.cpp di bawahnya tetap dekat dengan upstream dan dapat diperiksa.",
          "**Dompet kustodi mandiri** — kunci tidak pernah meninggalkan perangkat pengguna, dan imbalan dibayarkan ke dompet Solana milik masing-masing pemilik node. Di devnet, KVR yang di-stake dan kredit prabayar disimpan oleh treasury gateway dan dicatat di buku besarnya hingga program staking on-chain dirilis.",
          "**Terbukti di perangkat nyata** — model 122B pernah berjalan ujung-ke-ujung di 3 mesin fisik pada armada uji kami, dengan kontribusi tiap node tercatat ujung-ke-ujung.",
          "**Dinamai dari mitos Nordik** — Kvasir, makhluk paling bijak, lahir dari sari gabungan semua dewa dan bukan milik siapa pun.",
        ],
      },
      { t: "h2", kick: "Satu permintaan, banyak perangkat", text: "Bagaimana inferensi mengalir" },
      {
        t: "code",
        caption: "Setiap lompatan adalah HTTP/TCP biasa; yang terdistribusi adalah modelnya sendiri.",
        code: `client SDK ──▶ gateway (OpenAI/Anthropic API, KVR settlement)
        ──▶ bridge (session · submit · gather)
        ──▶ serving topology: pipeline ring over layer windows,
            or expert-swarm dispatch at (layer, expert-range) grain
        ──▶ token streams back · each node's contribution is credited`,
      },
      {
        t: "p",
        md: "Peran bisa **bertumpuk**: satu mesin dapat sekaligus menjadi node komputasi, host gateway, dan host bridge, dan imbalannya dijumlahkan. Tugas jaringan adalah membuat keseluruhan tampak seperti satu mesin — satu endpoint di depan, ribuan perangkat tak sempurna di belakang.",
      },
      {
        t: "p",
        md: "Saat ini jaringan berjalan di **Solana devnet**; KVR adalah token utilitas / kontribusi, bukan aset yang dapat diperdagangkan atau investasi, dan tidak ada di halaman ini yang merupakan nasihat keuangan.",
      },
    ],
  },
  architecture: {
    title: "Arsitektur Kvasir",
    summary: "Satu peta untuk seluruh sistem: dompet, gateway yang menerima pembayaran, bridge yang menjadi wajah mesin inferensi, dan jaringan p4 yang menjalankan model.",
    blocks: [
      {
        t: "p",
        md: "Kvasir adalah empat lapisan dengan satu sambungan di tiap batasnya. **Dompet** memegang kunci. **Gateway** menerima pembayaran dan menyimpan buku besar. **Bridge** memberi wajah HTTP kepada mesin inferensi. **Jaringan p4** yang benar-benar menjalankan model. Semua yang ada di bawah ini adalah konsekuensi dari di mana sambungan-sambungan itu jatuh — dan diagramnya menandai apa yang sudah berjalan hari ini terhadap apa yang masih berupa rancangan.",
      },
      { t: "h2", kick: "Dompet", text: "Kunci tak pernah meninggalkan perangkat" },
      {
        t: "p",
        md: "iOS (Swift), Android (Kotlin), dan desktop (React + Electron) adalah build terpisah dari dompet yang sama, dan build desktop itu pula yang disajikan gateway di `/` sebagai dompet peramban — dompet utuh dengan penandatanganan di dalam halaman, bukan konsol baca-saja. Imbalan dibayarkan ke alamat Solana milik masing-masing pemilik; gateway tak pernah memegang kunci pengguna.",
      },
      { t: "h2", kick: "Gateway", text: "Satu proses, dua permukaan" },
      {
        t: "p",
        md: "`solana/staking-service` sekaligus menjadi **API gateway** (`/v1/chat/completions` yang kompatibel OpenAI, plus alur bayar-per-permintaan `/api/pay/quote` → `/api/inference`) dan **gateway penyelesaian** (staking, registri node, akun kredit, kredit kontribusi). Keduanya satu proses karena berbagi satu buku besar: sebuah permintaan baru dilayani setelah transfer KVR-nya terverifikasi on-chain, dan buku besar yang sama mengkreditkan node-node yang melayaninya.",
      },
      {
        t: "callout",
        md: "**Pembayaran diselesaikan sebelum inferensi berjalan.** Jika setelah itu bridge gagal, gateway mengembalikan dana pembayar dari treasury dan mengembalikan 502 alih-alih menagih untuk nol hasil. Tak ada model tiruan dan tak ada katalog pengganti di belakangnya: model yang ditawarkan aplikasi adalah model yang memang sedang dilayani sebuah bridge, atau daftarnya kosong.",
      },
      { t: "h2", kick: "Bridge", text: "Wajah HTTP mesin inferensi" },
      {
        t: "p",
        md: "Bridge (`p4bridge`) adalah sebuah **OUTER** dalam istilah p4: ia memasang sesi melintasi stage-stage, mengirim permintaan ke stage kepala, lalu mengumpulkan aliran token. Bagi gateway ia adalah kontrak kecil yang tetap — model apa yang termuat, siapa berkontribusi berapa, dan completion.",
      },
      {
        t: "table",
        head: ["Rute", "Apa yang dijawabnya"],
        rows: [
          ["`/api/controllers`", "model apa yang termuat, dan status tiap stage"],
          ["`/api/runtime`", "dompet operator dan mesin-mesin di belakangnya"],
          ["`/api/contributions`", "baris, unit, permintaan, dan throughput per node"],
          ["`/c/<model>/v1/chat/completions`", "inferensi"],
        ],
      },
      {
        t: "p",
        md: "Dua tugas yang sengaja ditinggalkan p4 untuk bridge: **template chat** (p4 menyerahkan prompt buram ke stage server dan tak menerapkan satu pun, sehingga model instruct akan melanjutkan teks Anda alih-alih menjawabnya) dan **blok penalaran** (dikembalikan sebagai `reasoning_content`, terpisah dari `content`, agar sesi berpikir tak diam-diam melahap anggaran token lalu menagih pembayar untuk jawaban kosong).",
      },
      {
        t: "callout",
        md: "**Bridge tak pernah dipublikasikan.** Satu-satunya autentikasinya adalah service token bersama, dan apa pun yang bisa menjangkaunya bisa menjalankan ring. Ia mengikat loopback; terowonganlah pintunya.",
      },
      { t: "h2", kick: "Jaringan p4", text: "Agen memiliki node, stage server memegang lapisan" },
      {
        t: "p",
        md: "Sebuah **agen** memiliki node-node pada satu host; sebuah **stage server** adalah satu proses yang memegang irisan lapisan model. Sebuah stage menyerahkan hasilnya ke stage berikutnya dengan meminta agennya sendiri menelepon agen stage itu **di alamat yang diiklankan agen tersebut** — jadi alamat yang diiklankan harus terjangkau dari host-host lain, dan sebaiknya jaringan tercepat yang mereka bagi. Pada rak MI250 itu berarti tautan InfiniBand, bukan LAN kantor dan tak pernah loopback.",
      },
      {
        t: "ul",
        items: [
          "**`p4-agent` dan `p4_staged_server` adalah satu rilis.** Agen yang dibangun dari pohon sumber lebih baru gagal di READY karena kapabilitas HELLO yang hilang — setelah memuat seluruh model.",
          "**Penempatan adalah artefak operator.** Lapisan mana duduk di GPU mana, pada load generation berapa, berasal dari rencana penempatan; bridge menjawab `409` kepada siapa pun yang memintanya melayani, dan watchdog gateway menyatakannya sekali lalu berhenti bertanya.",
          "**Sebuah pipeline butuh setidaknya dua stage.** Perintah sesi menolak pipeline satu-stage.",
        ],
      },
      { t: "h2", kick: "Relay", text: "Alamat yang bisa ditelepon untuk sebuah laptop" },
      {
        t: "p",
        md: "Node tepi — aplikasi desktop, sebuah ponsel — tak punya alamat yang bisa ditelepon siapa pun. **Relay** memberikannya: node menelepon keluar, membuktikan pasangan kunci dompetnya lewat tantangan ed25519, dan sejak itu terjangkau melalui relay. Relay adalah batas autentikasi dan tak pernah mengurai payload. Installer desktop mengirimkan agen p4 berdampingan dengan aplikasinya, jadi bergabung bukan instalasi kedua.",
      },
      { t: "h2", kick: "Penyelesaian", text: "Kredit mengikuti partisipasi" },
      {
        t: "p",
        md: "Setiap stage melaporkan baris token yang dijalankannya. Bridge mengakumulasinya per node, dan gateway mem-poll `/api/contributions` setiap 30 detik lalu mengkreditkan dompet yang disebut bridge, sebagai `rows / 1000` unit yang diskalakan tingkat performa node. **Dalam sebuah pipeline setiap stage melihat baris yang sama**, jadi ring empat-stage membayar keempat stage-nya sama rata tak peduli berapa lapisan yang dipegang masing-masing — kredit mengikuti partisipasi, bukan porsi bobot. Sharding pakar, tempat node memegang pecahan berbeda dari satu lapisan yang sama, adalah kasus yang nanti menuntut ini ditinjau ulang.",
      },
      { t: "h2", kick: "P4 Studio", text: "Apa yang ditandai diagram sebagai usulan" },
      {
        t: "p",
        md: "**P4 Studio** adalah konsol operator milik p4 sendiri. Umpan observabilitas per-permintaan yang dibutuhkannya dari para agen masih berupa usulan di upstream, bukan sesuatu yang berjalan di sini — karena itulah diagram menggambarnya putus-putus, bersama shard pakar yang dilayani dari node tepi, yang sudah dirancang dan belum berjalan.",
      },
    ],
  },
  bridge: {
    title: "Bridge",
    summary: "Wajah HTTP mesin inferensi: apa yang termuat, siapa yang berkontribusi, dan completion — dan tidak lebih dari itu.",
    blocks: [
      {
        t: "p",
        md: "**Bridge** adalah satu-satunya hal yang diajak bicara gateway penyelesaian untuk inferensi. Ia adalah sebuah **OUTER** dalam istilah p4: ia memasang sesi melintasi stage-stage model, mengirim permintaan ke stage kepala, mengumpulkan aliran token, lalu melaporkan kontribusi tiap node. Ia tak memiliki penempatan, tak memiliki penjadwalan, dan tak menyimpan status apa pun di luar katalog apa yang termuat — sengaja dibuat kecil, karena segala yang tidak diputuskannya adalah sesuatu yang tak bisa menyimpang.",
      },
      { t: "h2", kick: "Kontraknya", text: "Empat rute, satu token" },
      {
        t: "table",
        head: ["Rute", "Apa yang dijawabnya"],
        rows: [
          ["`/api/controllers`", "model apa yang termuat, dan status tiap stage"],
          ["`/api/runtime`", "dompet operator dan mesin-mesin di belakangnya"],
          ["`/api/contributions`", "baris, unit, permintaan, dan throughput per node"],
          ["`/c/<model>/v1/chat/completions`", "inferensi"],
        ],
      },
      {
        t: "p",
        md: "Setiap rute kecuali `/api/health` memerlukan service token bersama, dikirim sebagai `X-Kvasir-Service-Token`. Token itu adalah **satu-satunya** yang berdiri di antara internet terbuka dan pemakaian ring secara cuma-cuma, dan itulah sebabnya bridge mengikat loopback serta dijangkau lewat terowongan alih-alih dipublikasikan.",
      },
      { t: "h2", kick: "Yang ditinggalkan p4 untuknya", text: "Dua tugas yang tak akan dikerjakan mesinnya" },
      {
        t: "ul",
        items: [
          "**Template chat.** p4 menyerahkan prompt buram ke stage server dan tak menerapkan format giliran apa pun. Bridge merender format milik model — dibaca dari GGUF dan disebut di katalog sebagai `prompt_format`. Lewati itu dan model instruct akan melanjutkan teks Anda alih-alih menjawabnya, tak pernah memancarkan token akhir-gilirannya, dan setiap kali berjalan sampai batas token.",
          "**Blok penalaran.** Model penalaran membuka jawabannya dengan berpikir. Bridge mengembalikan itu sebagai `reasoning_content`, terpisah dari `content`, dan menghormati `enable_thinking: false` dengan menutup bloknya di dalam prompt — jika tidak, sesi berpikir yang panjang bisa melahap seluruh anggaran dan menyerahkan jawaban kosong kepada pemanggil yang sudah membayarnya.",
        ],
      },
      { t: "h2", kick: "Penempatan bukan tugasnya", text: "Mengapa ia menjawab 409" },
      {
        t: "p",
        md: "Meminta bridge melayani sebuah model menghasilkan **409**. Lapisan mana duduk di GPU mana, pada load generation berapa, berasal dari rencana penempatan yang ditulis dan dimuat seorang operator; tak ada muat-ulang jarak jauh yang bisa dilakukan. Watchdog ring gateway mempelajari ini sekali lalu berhenti bertanya alih-alih mencoba ulang sesuatu yang memang tak bisa berhasil.",
      },
      {
        t: "callout",
        md: "**Penghitung kontribusi hidup di memori.** Restart bridge menghilangkan apa pun yang belum di-poll gateway — gateway mem-poll setiap 30 detik — dan gateway mengatur ulang basis alih-alih menghitung ganda saat sebuah penghitung mundur. Node yang pemiliknya tak dikenal bridge dilewati **secara diam-diam**, sehingga dompet operator yang belum disetel terbaca sebagai \"mesin-mesin ini tak memperoleh apa pun\".",
      },
    ],
  },
  gateway: {
    title: "Gateway",
    summary: "Titik masuk publik: API kompatibel OpenAI/Anthropic dan penyelesaian bayar-per-inferensi dalam KVR.",
    blocks: [
      {
        t: "p",
        md: "**Gateway** adalah tempat pengembang bertemu jaringan. Setiap controller mengekspos endpoint kompatibel OpenAI (`/v1/chat/completions`, `/v1/models`) dan kompatibel Anthropic (`/anthropic/v1/messages`, `/anthropic/v1/models`), semuanya ditopang model termuat yang sama — klien yang ada berfungsi hanya dengan mengganti base URL dan kunci.",
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
        md: "Pemakaian diselesaikan dalam KVR lewat alur tiga langkah — **quote → payment → inference** — sehingga permintaan diberi harga sebelum dijalankan dan node yang melayaninya dikreditkan sesudahnya. Gateway juga mengagregasi **katalog model langsung** dari setiap bridge yang terjangkau, sehingga `/v1/models` mencerminkan apa yang benar-benar bisa dilayani jaringan saat ini.",
      },
      {
        t: "ul",
        items: [
          "Host gateway memperoleh **imbalan uptime per jam** karena menjaga titik masuk tetap daring, plus **bonus ×1.5** pada setiap inferensi yang ikut mereka layani.",
          "Peran gateway **ditetapkan oleh jaringan, bukan diklaim sendiri**: sebuah node tidak bisa menyetel flag gateway atau bridge-nya sendiri, dan uptime hanya dikreditkan selama gateway melihatnya menjawab.",
          "Deployment publik melindungi akses operator dengan **SIWS + 2FA**; bridge polos dirancang hanya untuk host tepercaya / LAN / VPN.",
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
          "Node terdaftar di bawah dompet pemiliknya; imbalan dibayarkan ke dompet itu. Empat dompet pemilik terpisah, masing-masing memperoleh porsi lapisannya, telah diverifikasi ujung-ke-ujung pada armada uji milik satu operator.",
          "Data kapabilitas (backend, presisi akumulasi, anggaran sumber daya) menentukan apa yang boleh ditempatkan planner di sebuah node — dan, di swarm, rank mana yang boleh dilayaninya.",
          "Node yang tak dapat menyediakan pemantauan sumber daya dikecualikan dari pemuatan adaptif alih-alih dipercaya membabi buta.",
        ],
      },
    ],
  },

  p4: {
    title: "p4",
    summary: "Mesin di balik Kvasir: protokol beralamat-peristiwa tempat agen memiliki node, stage server memegang lapisan, dan penempatan adalah sesuatu yang dinyatakan operator alih-alih ditebak jaringan.",
    blocks: [
      {
        t: "p",
        md: "**p4** menjalankan satu model di beberapa mesin dengan memotongnya menjadi **stage** — irisan lapisan yang bersambung — dan memberi tiap stage prosesnya sendiri. Sebuah **agen** memiliki node-node pada satu host: ia memunculkan stage server, merutekan peristiwa di antara mereka, dan bertanggung jawab atas siklus hidupnya. Tak ada penjadwal yang memutuskan apa ditaruh di mana; operator menulis rencana penempatan, memuatnya, dan jaringan lalu melayani persis itu.",
      },
      {
        t: "code",
        caption: "Jalur permintaan melalui deployment p4.",
        code: `browser / SDK
  → gateway :8791              # payment, settlement, the wallet app
  → bridge :19000              # OUTER: session, submit, gather
  → p4 agent                   # owns this host's nodes
  → stage servers              # one process per layer slice`,
      },
      { t: "h2", kick: "Pengalamatan", text: "Sebuah stage menelepon agen stage berikutnya" },
      {
        t: "p",
        md: "Ketika sebuah stage menyelesaikan lapisannya, ia menyerahkan hasilnya ke stage berikutnya dengan meminta agennya sendiri membuka koneksi ke **agen stage itu, di alamat yang diiklankan agen tersebut**. Karena itu alamat yang diiklankan bukan sekadar kosmetik: ia harus terjangkau dari setiap host lain di ring, dan sebaiknya menyebut jaringan tercepat yang mereka bagi. Iklankan loopback, dan ring dua-host diam-diam menelepon dirinya sendiri.",
      },
      { t: "h2", kick: "Siklus hidup", text: "Satu angka mengikat satu pemuatan" },
      {
        t: "ul",
        items: [
          "**Load generation dipilih oleh siapa pun yang memuat** dan dibandingkan untuk kesetaraan persis pada setiap sesi, inferensi, penyelesaian, dan unload. Angka itu tak tercatat di mana pun pada mesin-mesinnya, jadi pemuat menuliskannya ke disk *sebelum* perintah pertama berangkat — tanpa itu, model yang sudah termuat bahkan tak bisa diturunkan.",
          "**Generation sebuah node dan load generation adalah angka yang sama.** Adapter memeriksa generation sumber pada tanda terima pelepasan terhadap pemuatan yang menaunginya dan menghentikan node bila keduanya berbeda, sehingga ring yang dimuat dengan dua angka berbeda melayani satu permintaan lalu kehilangan kepalanya.",
          "**Jurnal operasional wajib ada** sebelum sebuah model mau dimuat sama sekali: itu adalah catatan penerimaan yang membuat sebuah pemuatan aman-diulang, bukan alat bantu debug.",
        ],
      },
      { t: "h2", kick: "Yang tidak dilakukannya", text: "Kelalaian yang disengaja" },
      {
        t: "p",
        md: "p4 tak menerapkan **template chat** apa pun — ia meneruskan prompt buram dan mengharapkan pemanggil sudah merender format giliran model. Ia tak membuat **keputusan penempatan**. Dan ia tak punya gagasan tentang siapa yang harus dibayar: stage melaporkan baris token yang dijalankannya, dan penyelesaian adalah kontrak pihak lain. Masing-masing adalah sambungan yang diisi Kvasir di [bridge](/wiki/bridge), yang menjaga mesinnya tetap cukup sempit untuk mengikuti upstream.",
      },
      {
        t: "callout",
        md: "**Agen dan stage server native adalah satu rilis.** Agen yang dibangun dari pohon sumber lebih baru gagal di READY karena kapabilitas yang hilang pada HELLO stage server — setelah memuat seluruh model. Bangun keduanya dari checkout yang sama.",
      },
    ],
  },
  "in-flight-ring": {
    title: "Ring in-flight",
    summary: "Pipeline yang tak pernah dikosongkan: beberapa permintaan menempati stage yang berbeda pada saat bersamaan, jadi tak ada stage yang menunggu permintaan di depannya selesai.",
    blocks: [
      {
        t: "p",
        md: "Kvasir melayani sebuah model sebagai **pipeline stage**, masing-masing memegang irisan lapisan yang bersambung. Sebuah stage menjalankan lapisannya lalu meneruskan batasnya — sebuah hidden state, bukan bobot — ke stage berikutnya. Tak ada stage yang memegang seluruh model, dan tak ada apa pun yang duduk di tengah jalur data: bridge mengirim ke kepala dan membaca dari ekor, sementara stage-stage saling menyerahkan hasil lewat agen mereka sendiri.",
      },
      { t: "h2", kick: "Bagian in-flight-nya", text: "Mengapa pipeline yang dikosongkan menyia-nyiakan sebagian besar mesin" },
      {
        t: "p",
        md: "Jika sebuah pipeline menuntaskan satu permintaan sebelum menerima yang berikutnya, setiap stage kecuali satu menganggur pada tiap saat — ring empat-stage berjalan pada seperempat perangkat kerasnya. Desain **in-flight** menjaga beberapa permintaan tetap bergerak sekaligus: sementara stage 3 mendekode satu permintaan, stage 0 sudah melakukan prefill untuk yang lain. Stage melaporkan berapa lama mereka menahan sebuah batch dan berapa lama mereka tak punya apa pun untuk dikirim, sehingga ring yang kelaparan terlihat berbeda dari ring yang jenuh.",
      },
      {
        t: "code",
        caption: "Empat stage, tiga permintaan, satu saat dalam waktu.",
        code: `           stage 0        stage 1        stage 2        stage 3
           layers 0-11    12-22          23-33          34-44

request A                                              decode
request B                 decode
request C  prefill

boundaries pass →  agent to agent, never through the caller`,
      },
      { t: "h2", kick: "Keanggotaan", text: "Apa persisnya sebuah batch" },
      {
        t: "p",
        md: "Baris dari permintaan yang berbeda dikemas menjadi satu batch fisik, dan keanggotaan persis itu diteruskan ke setiap stage hilir alih-alih diputuskan ulang tiap lompatan. Itulah yang memungkinkan satu prefill dan beberapa dekode berbagi satu lintasan, dan itulah sebabnya ukuran sebuah batch adalah properti dari pemuatan: rencana menyatakan lebar baris dan micro-batch di muka, dan lebar-lebar itu menetapkan hasil terbesar yang pernah bisa dikembalikan sebuah stage.",
      },
      {
        t: "callout",
        md: "**Sebuah pipeline butuh setidaknya dua stage.** Pipeline satu-stage ditolak mentah-mentah — kepala dan ekor adalah peran yang berbeda, dan satu node yang meruntuhkan keduanya adalah mesin yang lain, bukan ring yang lebih kecil.",
      },
      {
        t: "p",
        md: "Ring adalah jalur **latensi**, dan granularitasnya adalah lapisan. Sharding pakar menghapus lantai itu dengan memotong di dalam sebuah lapisan, dan menyambung ke jalinan penyajian yang sama.",
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
        md: "Model **Mixture-of-Experts** mengganti FFN tunggal tiap lapisan dengan bank FFN pakar independen plus **router** yang memilih beberapa per token. Step-3.7-Flash, MoE 428B yang dilayani hari ini, punya 288 pakar per lapisan dengan perutean top-8. Qwen3.5-122B-A10B adalah contoh terperinci di bawah, karena ia yang angka-angkanya terukur ujung-ke-ujung:",
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
      {
        t: "callout",
        md: "**Status mesin.** Sharding pada butiran pakar dibangun dan didemonstrasikan di mesin Kvasir sebelumnya, dan hasil-hasil di bawah ini berasal dari kerja itu. Mesin saat ini, [p4](/wiki/p4), melayani pada butiran lapisan hari ini; memindahkan sharding pakar ke atasnya sudah dirancang dan sedang dikerjakan. Di mana sebuah detail menyebut nama perkakas atau rute, itu adalah yang berjalan di mesin sebelumnya.",
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
        t: "callout",
        md: "**Status mesin.** Sharding pada butiran pakar dibangun dan didemonstrasikan di mesin Kvasir sebelumnya, dan hasil-hasil di bawah ini berasal dari kerja itu. Mesin saat ini, [p4](/wiki/p4), melayani pada butiran lapisan hari ini; memindahkan sharding pakar ke atasnya sudah dirancang dan sedang dikerjakan. Di mana sebuah detail menyebut nama perkakas atau rute, itu adalah yang berjalan di mesin sebelumnya.",
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
        md: "**GGUF** adalah format model satu-berkas dari ekosistem mesin inferensi: metadata (arsitektur, jumlah lapisan, dimensi, kuantisasi) plus tensor sebagai byte terkuantisasi mentah (mis. Q4_K_M). Rencana penempatan ditulis terhadap metadata itu — rentang lapisan, penugasan perangkat, dan perkiraan ukuran; sisi penyajian mengiris byte tensor untuk menghasilkan unduhan.",
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
          "**Sisi perolehan** — unit kontribusi × porsi lapisan × tingkat performa untuk komputasi; uptime per jam untuk peran bridge/gateway.",
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
infra      : bridge uptime/hr > gateway uptime/hr  (summed on top)`,
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
          "Peran **bertumpuk** — satu mesin bisa jadi komputasi + gateway + bridge, dan alirannya dijumlahkan.",
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
    summary: "Mengunci KVR di vault. Ini tidak lagi menentukan peran operator, dan tidak ada yang mensyaratkannya.",
    blocks: [
      {
        t: "p",
        md: "Staking mengunci KVR di vault lewat panel staking dompet. Dulu inilah gerbang ke peran operator — bridge atau gateway memerlukan stake 100.000 KVR — dan **syarat itu sudah tidak ada lagi**. Tidak ada stake yang dibutuhkan untuk menjalankan node apa pun, dan dompet yang sama sekali tidak memegang KVR pun bisa mendaftarkan satu node dan memperoleh imbalan. Kedua peran infrastruktur itu kini ditetapkan oleh jaringan, kendali yang lebih kuat daripada sekadar harga: pemeriksaan lama membaca saldo dompet sekali saat pendaftaran, tak pernah menguncinya dan tak pernah menengoknya lagi, sehingga 100.000 KVR yang sama bisa mendaftarkan berapa pun node lalu dipindahkan.",
      },
      {
        t: "ul",
        items: [
          "Staking dilakukan di panel staking dasbor dompet: masukkan jumlah, **Stake**, dan posisi itu tersimpan di vault sampai Anda menariknya kembali.",
          "Ini bukan syarat untuk apa pun. Imbalan node berasal dari pekerjaan yang benar-benar dilakukan node, plus uptime terverifikasi untuk peran infrastruktur — bukan dari memegang saldo.",
          "Tingkat staking devnet saat ini **0%**, jadi sebuah posisi tidak menghasilkan apa pun dengan sendirinya. Anggap panel itu sebagai mekanisme yang tersedia, bukan cara untuk memperoleh penghasilan.",
          "Di devnet, KVR yang di-stake disimpan di vault staking; jumlah yang di-stake dan imbalan node terlihat di panel staking.",
          "KVR devnet untuk staking berasal dari faucet distribusi; SOL devnet untuk biaya berasal dari faucet publik.",
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
        md: "Kvasir Wallet **non-kustodial sejak desain**: frasa pemulihan 12 kata dan kunci hanya disimpan di perangkat milik pengguna, tak pernah pada operator. Imbalan diselesaikan di Solana langsung ke dompet pemilik tiap node — terverifikasi pada armada uji di empat dompet pemilik berbeda, masing-masing memperoleh porsi lapisannya sendiri. Staking bekerja secara berbeda di devnet: KVR yang di-stake disimpan di treasury gateway dan dicatat di buku besarnya hingga program staking on-chain dirilis.",
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
        md: "Untuk deployment publik, akses operator ke gateway diautentikasi dengan **Sign-In With Solana**: dompet operator menandatangani nonce yang diterbitkan server, membuktikan kepemilikan tanpa kata sandi atau kredensial yang dititipkan. Di atasnya, **2FA TOTP** dan kode cadangan sekali-pakai melindungi sesi.",
      },
      {
        t: "ul",
        items: [
          "**Tak ada kata sandi di mana pun** — kunci dompet adalah identitasnya dan nonce mencegah replay; tak ada apa pun di sisi server yang bisa di-phishing atau bocor.",
          "**Pendaftaran TOTP per dompet** dipersistenkan di buku besar gateway, jadi 2FA bertahan melewati restart.",
          "**Kode cadangan sekali pakai** — tiap kode terpakai saat login, untuk pemulihan saat perangkat autentikator tak tersedia.",
          "**Cakupan dinyatakan jujur** — bridge dan port-port mesin inferensi mengasumsikan host tepercaya / LAN / VPN; SIWS + 2FA adalah lapisan yang membuat domain *publik* aman untuk diekspos.",
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
        md: "Karena inferensi **harus** dibayar dalam KVR, token terikat pada pemakaian nyata — utilitas, bukan spekulasi. Pemakaian mendanai KVR yang diperoleh node, yang menjaga kontribusi tetap menarik, yang menumbuhkan kapasitas, yang menurunkan harga dan latensi, yang menarik lebih banyak pemakaian. Keunggulan paling tajam Kvasir mengetatkan lingkar itu lebih jauh lagi: seorang peserta bisa menjadi **konsumen dan pemasok sekaligus** (seorang *prosumer*), sehingga kedua sisi kerap tumbuh di dalam orang yang sama.",
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
        md: "Kvasir sudah mengganjar **kerja nyata** (KVR per token yang dilayani × porsi lapisan, bukan sekadar kehadiran) dan membayar ke dompet milik tiap node, yang merupakan bagian tersulit dari membuat imbalan berbasis pendapatan menjadi jujur. Sisanya — harga yang digerakkan utilisasi dan peruncingan emisi→pendapatan — adalah peta jalan ekonomi yang mengubah \"lebih banyak node → lebih murah\" dari intuisi menjadi aturan yang ditegakkan protokol. Entri **Harga inferensi** membahas sisi harga; **Unit kontribusi** membahas bagaimana kerja menjadi imbalan.",
      },
    ],
  },
  "inference-pricing": {
    title: "Harga inferensi",
    summary: "Berapa biaya sebuah inferensi dalam KVR hari ini, mengapa jaringan terdesentralisasi lebih murah secara struktural, dan bagaimana harga dirancang turun seiring pasokan tumbuh.",
    blocks: [
      {
        t: "p",
        md: "Akses ke jaringan bersifat **bayar-per-inferensi**: gateway mengutip harga KVR untuk permintaan Anda, dompet Anda membayarnya di on-chain, dan barulah ring menjalankan model. Penetapan harga adalah rumus kecil yang transparan — lantai per permintaan plus tarif per token — dikutip di muka dan diselesaikan atas pemakaian token **sebenarnya** setelah generasi.",
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
      {
        t: "callout",
        md: "**Status mesin.** Sharding pada butiran pakar dibangun dan didemonstrasikan di mesin Kvasir sebelumnya, dan hasil-hasil di bawah ini berasal dari kerja itu. Mesin saat ini, [p4](/wiki/p4), melayani pada butiran lapisan hari ini; memindahkan sharding pakar ke atasnya sudah dirancang dan sedang dikerjakan. Di mana sebuah detail menyebut nama perkakas atau rute, itu adalah yang berjalan di mesin sebelumnya.",
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
          "**Imbalan per kerja.** Kerja yang dijembatani terakumulasi ke buku besar kontribusi bridge; gateway meng-delta-kredit KVR ke dompet **milik Anda sendiri**. Anda perlu alamat dompet untuk dibayar.",
        ],
      },
      {
        t: "callout",
        md: "Worker berbicara dengan protokol dispatch yang sama seperti GPU di dalam pusat data — `(n_used, n_tokens, cur, sel) → experts` lewat satu stream berumur panjang. Worker shard-parsial cukup menyetel `n_used = 1`. Keseragaman itulah alasan ponsel, kotak CPU, dan kartu Blackwell menjadi anggota yang dapat dipertukarkan dari swarm yang sama.",
      },
    ],
  },
  "bridge-operations": {
    title: "Mengoperasikan bridge",
    summary: "Catatan operator untuk menjalankan bridge dan ring di belakangnya: muat sebuah rencana, selamat dari restart, jaga kontribusi tetap mengalir, dan jauhkan mesinnya dari internet.",
    blocks: [
      {
        t: "p",
        md: "**Gateway** (titik masuk publik) dan **bridge** (wajah mesin) adalah dua layanan berumur panjang yang dijaga tetap sehat oleh operator, dengan agen-agen p4 dan stage server mereka di belakangnya. Mesinnya sama sekali tak berbicara autentikasi — ia mengasumsikan mesin-mesin yang bisa saling menjangkau memang dimaksudkan begitu — sehingga semua yang publik memusat pada gateway, dan bridge dijangkau lewat terowongan alih-alih dipublikasikan.",
      },
      { t: "h2", kick: "Memuat", text: "Sebuah rencana, dan angka yang menaunginya" },
      {
        t: "ul",
        items: [
          "**Penempatan adalah rencana yang Anda tulis**, bukan permintaan yang Anda ajukan: bridge menjawab `409` kepada siapa pun yang memintanya melayani. Hasilkan rencananya, jalankan uji-kering, lalu muat dengan `--confirm`.",
          "**Load generation ditulis ke disk sebelum perintah pertama berangkat.** Ia dipilih oleh pemuat, diperiksa untuk kesetaraan persis pada setiap sesi dan saat unload, dan tak tercatat di mana pun pada mesin-mesinnya — hilangkan angka itu dan model yang sudah termuat bahkan tak bisa diturunkan.",
          "**Generation node adalah angka yang sama itu.** Muat sebuah ring dengan dua angka berbeda dan ia melayani persis satu permintaan sebelum kepalanya berhenti; sesi berikutnya menggantung setengah-termuat. Pakai nilai baru tiap pemuatan, atau registrasi yang ditinggalkan percobaan gagal akan bertabrakan dengannya.",
          "**Agen menolak memuat tanpa jurnal operasionalnya.** Itu adalah catatan penerimaan yang dibutuhkan pemuatan aman-diulang, bukan flag debug.",
        ],
      },
      { t: "h2", kick: "Selamat dari restart", text: "Apa yang kembali dan apa yang tidak" },
      {
        t: "ul",
        items: [
          "**Restart agen menjatuhkan node-nya.** Stage server hanya ada saat runtime; modelnya harus dimuat lagi dari rencana. Itu prosedur pemulihannya, bukan kegagalan.",
          "**Gateway tak akan memuat ulang untuk Anda.** Watchdog ring-nya menyadari sebuah model berhenti melayani, belajar dari `409` bridge bahwa penempatan bersifat eksternal, menyatakannya sekali, lalu berhenti bertanya.",
          "**Penghitung kontribusi hidup di memori bridge.** Gateway mem-poll setiap 30 dtk dan meng-delta-kredit; sebuah restart hanya menghilangkan apa yang belum sempat di-poll, dan gateway mengatur ulang basis alih-alih membayar ganda saat sebuah penghitung mundur.",
        ],
      },
      { t: "h2", kick: "Kunci rapat", text: "Mesinnya tidak menghadap internet" },
      {
        t: "ul",
        items: [
          "Ikat bridge ke loopback dan beri ia service token. Tanpa token itu ia tak mengautentikasi siapa pun, dan apa pun yang menjangkaunya bisa menjalankan ring secara cuma-cuma — ia menyatakannya saat start alih-alih membiarkan Anda menemukannya belakangan.",
          "Agen mengiklankan alamat yang ditelepon agen lain. Pakai jaringan tercepat yang dibagi host-host itu, jangan pernah loopback antar-host, dan jauhkan jaringan itu dari internet publik.",
          "Jika sesuatu harus berada di IP publik, ingat bahwa port terbit Docker **di-DNAT sebelum rantai INPUT**, jadi aturan pada `dport` takkan cocok. Filter di rantai `DOCKER-USER` memakai port tujuan asli conntrack (`--ctorigdstport`), dan persistenkan dengan systemd oneshot yang diurutkan `After=docker.service`.",
        ],
      },
      { t: "h2", kick: "Jebakan", text: "Dua yang memakan waktu sungguhan" },
      {
        t: "ul",
        items: [
          "**Dompet operator yang belum disetel terbaca sebagai nol perolehan.** Gateway melewati setiap baris kontribusi tanpa pemilik dan tak mencatat apa pun. Node-nya tampak menganggur padahal sedang melayani.",
          "**`pkill` mencocokkan baris perintahnya sendiri.** `ssh host 'pkill -f server.js; ...'` membunuh shell yang menjalankannya. Taruh polanya di sebuah berkas skrip alih-alih di perintah jarak jauh, pakai kelas karakter (`server[.]js`), dan ingat bahwa proses yang dijalankan sebagai `node server.js` polos tak membawa path untuk dicocokkan — temukan lewat port dengarnya saja.",
        ],
      },
    ],
  },
  "wan-interconnect": {
    title: "Interkoneksi WAN (optik 200G)",
    summary: "Bagaimana situs komputasi saling terhubung pada 200 Gb/s melintasi ruangan, kampus, atau kota: optik mana pada jarak mana, apa yang dicolok di mana, dan apa yang dibutuhkan untuk benar-benar mencapai line rate.",
    blocks: [
      {
        t: "p",
        md: "Ketika dua situs sama-sama punya rute publik, bidang data dispatch-pakar sebaiknya berupa **tautan langsung** — relay diperuntukkan bagi edge yang tak punya alamat sendiri. Entri ini adalah resep konkret untuk menjadikan tautan langsung itu berkelas 200 Gb/s dengan komponen katalog. Satu aturan menata semuanya: **serat adalah kaca netral-kecepatan; kecepatan berada pada pluggable di tiap ujung.**",
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
          "**Sisi NIC** — kartu kelas ConnectX-6/7 mengekspos cage QSFP56; DAC/AOC/FR4/LR4/ER4 semuanya duduk langsung di NIC. Host kelas GB10 sudah punya dua port QSFP 200 GbE di papan, jadi tautan dua-situs butuh tepat satu kabel dan nol perangkat keras baru.",
          "**Sisi switch** — optik koheren ZR+ berformat QSFP-DD dan berada di switch atau router; NIC situs itu lalu bergabung ke switch tersebut pada 200G lewat DAC pendek. Pakai tingkat ini saat situs seberang berjarak puluhan kilometer.",
          "**Serat itu sendiri** — pasangan LC dupleks single-mode standar (G.652), disewa sebagai dark fiber per untai. Kaca yang sama membawa 100G hari ini dan 400G nanti; peningkatan cukup tukar modul, tak pernah pekerjaan sipil.",
          "**Di luar ~120 km** — Anda berhenti membeli komponen dan mulai menyewa panjang gelombang dari carrier; batas demarkasinya adalah handoff Ethernet di switch Anda.",
        ],
      },
      {
        t: "code",
        caption: "Tiga rakitan acuan, termurah lebih dulu.",
        code: `two-site bench  : site A qsfp0 ──QSFP56 DAC 1m── site B qsfp0
campus pair     : site A [LR4] ──dark fiber, ≤10km── [LR4] site B
metro federation: site ──DAC── switch [ZR+ @200G] ──SMF ≤120km── [ZR+] switch ──DAC── site`,
      },
      { t: "h2", kick: "Langkah 3 · benar-benar mencapai 200G", text: "Line rate adalah konfigurasi, bukan pembelian" },
      {
        t: "ul",
        items: [
          "Pakai **RDMA (RoCE)** untuk stream dispatch bila tersedia — host kelas GB10 mengumpani NIC lewat tautan PCIe terbelah, dan kecepatan penuh terukur (~185–190 Gb/s) muncul di bawah RoCE dengan topologi yang terpetakan benar; jalur yang salah-petakan terbatas mendekati separuh laju, dan TCP polos tanpa penyetelan mendarat jauh lebih rendah.",
          "Aktifkan **jumbo frame (MTU 9000)** ujung-ke-ujung dan pertahankan `TCP_NODELAY` pada soket dispatch (bridge sudah menyetelnya).",
          "Bersiaplah untuk *memverifikasi*, bukan mengasumsikan: jalankan perftest antar-situs setelah tiap perubahan fisik — beda antara 95 dan 190 Gb/s tak terlihat sampai diukur.",
          "Jaga **relay 443 sebagai jalur cadangan** — kebijakan dial adalah langsung-dulu untuk peer publik, relay untuk NAT. Tugas relay adalah jangkauan, tugas tautan langsung adalah kecepatan.",
        ],
      },
      {
        t: "p",
        md: "Mengapa ini penting bagi arsitektur: latensi dekode dibatasi oleh waktu bolak-balik (~5 µs/km di serat — fisika, tak terpengaruh bandwidth), jadi pipa gemuk membeli **kecepatan prefill, throughput dispatch berkelompok, dan distribusi irisan-pakar nyaris seketika**, bukan latensi per-token yang lebih rendah. Itulah persis peran tingkat-situs dalam desain dua-tingkat: kapasitas di tingkat pipa-gemuk, jangkauan di tingkat relay.",
      },
    ],
  },
  "load-adaptive-scaling": {
    title: "Penskalaan adaptif-beban",
    summary: "Jalur penyajian MoE Kvasir tumbuh dan menyusut seiring lalu lintas: koordinator melibatkan kembali worker terbukti di bawah kejenuhan, dan bridge merekrut node menganggur dengan menaikkan permintaan pakar — semuanya berbasis-tarik, sehingga perangkat ber-NAT pun ikut bergabung.",
    blocks: [
      {
        t: "p",
        md: "Jalur penyajian MoE Kvasir menskala secara elastis seiring beban, dalam dua lapisan yang saling bekerja sama. Saat sepi, koordinator melayani semuanya secara lokal demi jalur tercepat per token; saat jenuh, dua lapisan di bawah menumbuhkan swarm — dan menyusutkannya lagi saat lonjakan berlalu.",
      },
      {
        t: "callout",
        md: "**Status mesin.** Sharding pada butiran pakar dibangun dan didemonstrasikan di mesin Kvasir sebelumnya, dan hasil-hasil di bawah ini berasal dari kerja itu. Mesin saat ini, [p4](/wiki/p4), melayani pada butiran lapisan hari ini; memindahkan sharding pakar ke atasnya sudah dirancang dan sedang dikerjakan. Di mana sebuah detail menyebut nama perkakas atau rute, itu adalah yang berjalan di mesin sebelumnya.",
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
      { t: "h2", kick: "Lapisan 2", text: "Sisi bidang kendali: rekrutmen adaptif-beban" },
      {
        t: "p",
        md: "Bidang kendali mengawasi setiap koordinator MoE dan menumbuhkan kumpulan worker saat diperlukan:",
      },
      {
        t: "ul",
        items: [
          "Sebuah loop latar menjajaki slot tiap koordinator dan mencatat kejenuhan per model.",
          "Selama sebuah model jenuh, **target replika-pakar efektifnya** dinaikkan (base + boost). Pasar cakupan lalu membaca pakar-pakar yang sudah tercakup sebagai langka kembali, dan sebuah model **tanpa** worker aktif di-seed dari metadata GGUF-nya (jumlah pakar) sehingga permintaan terlihat bahkan dari nol.",
          "Node yang menganggur menjajaki pasar permintaan (`/api/expert-volunteer`) dan diberi sepotong `(layer, expert-range)` untuk dilayani. Mereka mengunduh potongan itu, menyambung ke relay, dan mendaftarkan cakupan; bidang kendali secara otomatis mengaitkan mereka ke peta dispatch koordinator.",
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
