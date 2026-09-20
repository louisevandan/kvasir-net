/* Bahasa Indonesia — terjemahan blog teknis. Struktur (slug, kategori, urutan
   blok, kode, posisi img) mencerminkan persis articles.ts (sumber bahasa
   Inggris); istilah teknis, pengenal, dan angka dipertahankan. */
import type { TechTranslation } from "./articles";

export const idTech: Record<string, TechTranslation> = {
  "a-dialable-address-for-a-laptop": {
    title: "Alamat yang Bisa Dihubungi untuk Sebuah Laptop",
    dek: "p4 menjangkau sebuah node dengan menghubunginya, dan mesin di balik NAT tidak punya alamat untuk dihubungi. Sebagian besar mesin berada di balik NAT. Inilah yang diperlukan agar satu di antaranya tetap bisa bergabung, beserta pengukuran pada hari itu.",
    blocks: [
      {
        t: "callout",
        md: "**Hambatannya bukan usaha, melainkan arah.** p4 mengantarkan pekerjaan dengan membuka koneksi TCP *ke* sebuah node. Koneksi yang dibuka sendiri oleh node hanya untuk tanda terima — frame `Data` yang tiba di sana dijawab dengan `peer_closed(\"unexpected frame on outbound hop\")`. Sebuah laptop tidak bisa membuka terowongan keluar lalu menerima pekerjaan lewat terowongan itu. Tidak dengan niat baik, tidak dengan percobaan ulang, tidak dengan klien yang lebih baik.",
      },
      { t: "h2", kick: "Kendalanya", text: "Mesin seorang kontributor pada dasarnya tak terjangkau" },
      {
        t: "p",
        md: "Mesin di rumah duduk di balik penerjemahan alamat jaringan. Ia bisa menjangkau internet; internet tidak bisa menjangkaunya. Armada kami sendiri memperlihatkan kedua sisi: GB10 di kantor memegang alamat publik secara langsung, sementara dua MI250 berada di `192.168.20.x` di balik satu jalan keluar bersama. MI250 bukan kekecualian — begitulah rupa mesin seorang kontributor.",
      },
      {
        t: "p",
        md: "Di p4 tidak ada tabel rute, tidak ada layanan pertemuan, tidak ada hole punching. `deliver_outbound` mengambil alamat dari amplop peristiwa lalu memanggil `connect`. Hanya itu, dan di dalam rak itu rancangan yang masuk akal. Hanya saja bukan rancangan yang bisa dipenuhi sebuah laptop.",
      },
      { t: "h2", kick: "Jawabannya", text: "Satu koneksi keluar, dijaga tetap terbuka" },
      {
        t: "p",
        md: "Relay memegang alamat publik atas nama node. Node memelihara satu koneksi keluar ke sana dan tidak mendengarkan apa pun. Ketika seseorang menghubungi alamat publik itu, byte-byte tersebut turun melalui koneksi yang sudah dipegang node. Kedua ujung p4 melihat soket biasa menuju alamat biasa, dan tak satu pun tahu bahwa relay itu ada.",
      },
      {
        t: "code",
        caption: "Apa yang dilihat penghubung, dan apa yang benar-benar dijalankan node",
        code: "caller  ->  tcp://relay:43100        # alamat p4 biasa\n            |\n            +-- relay      publik; meneruskan byte, tidak mengurainya\n                  |\n                  +-- tunnel   satu koneksi keluar yang dibuka node\n                        |\n                        +-- p4-agent  127.0.0.1:42031, hanya mendengarkan loopback",
      },
      {
        t: "p",
        md: "Relay tidak pernah membaca p4. Muatan diteruskan byte demi byte dan tidak pernah diurai: ia tidak bisa membedakan `LOAD` dari `INSPECT`, dan memang tidak boleh bisa — begitu ia memahami lalu lintas, ia menjadi sesuatu yang mampu mengubahnya.",
      },
      { t: "h2", kick: "Mengapa ini sekaligus batas keamanan", text: "Penerusan porta tidak pernah menjadi pilihan" },
      {
        t: "p",
        md: "p4 tidak memiliki autentikasi apa pun. Tanpa TLS, tanpa token, tanpa daftar izin — host mana pun yang bisa menjangkau porta sebuah agen dapat mengirim `NODE_LOAD`, `NODE_UNLOAD`, dan `INSPECT`. Meminta kontributor meneruskan porta pada router rumahnya ke sana tidak dapat dibenarkan, dan itulah sebabnya solusi yang paling jelas justru yang keliru.",
      },
      {
        t: "p",
        md: "Melalui relay, node tidak mendengarkan apa pun. Ia memegang satu koneksi keluar dan membuktikan dompet operator sebelum koneksi itu membawa apa pun — tanda tangan ed25519 atas base58 yang sama seperti yang sudah dipakai gerbang penyelesaian, sehingga identitas yang didaftarkan node adalah identitas yang menjadi kunci buku besar imbalan. Keterjangkauan dan autentikasi ternyata punya jawaban yang sama.",
      },
      { t: "h2", kick: "Hari saat ia bekerja", text: "Diukur, bukan diklaim" },
      {
        t: "stats",
        items: [
          { n: "6 ms", l: "dari host armada ke Mac di balik NAT" },
          { n: "28 ms", l: "hingga agen menerapkan aturannya sendiri" },
          { n: "512 KiB", l: "kembali identik byte demi byte" },
          { n: "0", l: "porta yang dibuka pada laptop" },
        ],
      },
      {
        t: "p",
        md: "Sebuah host MI250 di pusat data menghubungi `tcp://34.50.62.159:43100` dan mencapai agen p4 yang berjalan di MacBook di balik NAT dalam 6 ms. Agen menutup koneksi 28 ms kemudian karena frame pertama bukan `Hello` — aturan protokolnya sendiri, diterapkan olehnya sendiri, dari mesin yang sesaat sebelumnya sama sekali tak bisa dihubungi. Penolakan itulah buktinya: relay tidak mengurai muatan, jadi ia tidak mungkin menghasilkannya.",
      },
      {
        t: "table",
        head: ["Pemeriksaan", "Hasil"],
        rows: [
          ["Pulang-pergi 512 KiB", "identik byte demi byte, 177 ms"],
          ["Pulang-pergi 256 KiB", "identik byte demi byte, 108 ms"],
          ["Tiga penghubung serentak", "tidak ada aliran yang bersilangan"],
          ["Pendaftaran tanpa tanda tangan", "ditolak"],
          ["Tantangan yang diulang", "ditolak"],
        ],
      },
      { t: "h2", kick: "Separuh lainnya", text: "Sebuah node perlu sesuatu untuk dijalankan" },
      {
        t: "p",
        md: "Keterjangkauan tidak berarti tanpa mesin di belakangnya, dan aplikasi desktop tidak pernah menyertakannya: ia mencari biner `p4-agent` di tiga direktori build dan hanya menemukannya di mesin tempat seseorang sudah mengompilasi p4 secara manual. Kini aplikasi membawa binernya sendiri, dibangun untuk setiap platform tempat ia dirilis.",
      },
      {
        t: "ul",
        items: [
          "macOS: biner universal dengan kedua arsitektur digabungkan. Aplikasi Mac dikemas sebagai `universal` dan sumber daya disalin apa adanya ke kedua irisan, sehingga biner arm64 saja akan memberi pengguna Mac Intel sebuah aplikasi yang tampak lengkap namun tak bisa menyalakan node — kegagalan yang hanya terlihat pada perangkat keras yang tidak dimiliki pengembang.",
          "Windows: dikompilasi silang, dan pencariannya tahu harus meminta `p4-agent.exe`. Melupakan ekstensi itulah cara sebuah build Windows merilis agen yang kemudian tak dapat ditemukannya.",
          "Pembangunan agen berjalan sebelum setiap pengemasan, sehingga rilis tanpa agen tidak mungkin terbentuk.",
        ],
      },
      {
        t: "callout",
        md: "**Yang belum bisa.** Sebuah node kini dapat berjalan dan dihubungi. Ia tetap tidak memperoleh apa pun dari inferensi: mesin memancarkan catatan kerja per tahap tetapi tidak ada yang mengumpulkannya, dan titik akhir kontribusi pada gerbang — yang dengan benar dikunci agar tidak ada node yang bisa mengkredit dirinya sendiri — belum pernah dipanggil. Berjalan dan dibayar adalah dua persoalan berbeda; yang terpecahkan baru yang pertama.",
      },
    ],
  },
  "expert-sharded-swarm-design": {
    title: "Inferensi swarm dengan sharding pakar: desainnya",
    dek: "86% dari MoE 122B adalah 12.544 pakar independen berukuran 5.3 MB. Iris model pada butiran itu dan ponsel dapat memikul bagian nyata dari inferensi frontier.",
    blocks: [
      {
        t: "callout",
        md: "**Tesisnya:** 86% bobot Qwen3.5-122B adalah 12.544 pakar 5.3 MB yang saling independen. Shard pada butiran pakar dan perangkat lemah memikul \"8–64 pakar (42–340 MB)\" alih-alih \"satu lapisan 1.4 GB\" — persis unit yang benar-benar bisa dipegang ponsel. MoE adalah substrat alami sebuah swarm.",
      },
      { t: "img", src: "/blog/expert-sharded-swarm-design.jpg", alt: "Blueprint of a MoE model carved into expert bundles flowing to a swarm of devices" },
      { t: "h2", kick: "Substrat · Qwen3.5-122B-A10B (Q4_K_M)", text: "Bobotnya sudah terkemas dalam unit seukuran swarm" },
      {
        t: "stats",
        items: [
          { n: "49", l: "lapisan" },
          { n: "256", l: "pakar / lapisan" },
          { n: "8", l: "aktif / token" },
          { n: "5.3 MB", l: "satu pakar (Q4)" },
          { n: "12,544", l: "total pakar" },
          { n: "86%", l: "bobot pada pakar" },
          { n: "3072", l: "n_embd" },
          { n: "ne[2]", l: "dim pakar = terluar" },
        ],
      },
      {
        t: "p",
        md: "Indeks pakar adalah **dimensi terluar** dari setiap tensor MoE, sehingga tiap pakar adalah lempengan bersambung yang selaras blok-kuantisasi. Mini-GGUF hasil irisan pakar adalah salinan rentang byte yang bersih — tanpa dekuantisasi, tanpa pengemasan ulang.",
      },
      { t: "h2", kick: "Dua peran", text: "Stage backbone × worker pakar" },
      {
        t: "ul",
        items: [
          "**Stage backbone (node kuat):** attention + cache KV, semua norm, **router**, pakar bersama, dan combine residual — seluruh jalur padat. Ia juga menyimpan semua pakar sebagai replika cadangan (offload RAM), yang memberi swarm toleransi churn.",
          "**Worker pakar (ponsel):** bukan transformer. Tanpa attention, tanpa KV, tanpa sampler — fungsi murni `(hidden, local_ids) → out` dari tiga mat-mul, hanya memuat irisan pakarnya sendiri. Muat di anggaran mana pun, hingga ponsel 4 GB.",
        ],
      },
      {
        t: "code",
        caption: "Titik potong: router berjalan sekali di backbone, dengan otoritas.",
        code: `cur   = ffn_norm(x)                       # backbone
ids,p = top_k(softmax(cur @ router), 8)   # backbone — authoritative
── dispatch selected experts to owner nodes ──
send  (cur rows, local_ids)  →  worker    # ~6 KB per decode step
recv  expert_out             ←  worker
x = x + combine(p, partials) + shared(cur)  # backbone — numerically exact`,
      },
      {
        t: "p",
        md: "Karena router berjalan **tepat sekali** di backbone, tiap pakar terpilih dihitung tepat sekali oleh node yang memilikinya. **Tidak ada aproksimasi** — sharding hanya memindahkan tempat mat-mul terjadi.",
      },
      { t: "h2", kick: "Bukan subsistem baru", text: "Swarm = pasar imbalan yang terbukti, dengan butiran lebih halus" },
      {
        t: "p",
        md: "Kvasir sudah menjalankan pasar kelangkaan otonom untuk shard **lapisan**, terverifikasi di perangkat nyata: ponsel di balik NAT mem-poll peta permintaan, mendaftar mandiri ke segmen **berimbalan tertinggi**, mengunduh sebagian hanya jendela itu, memuatnya di GPU Adreno-nya, dan menuntaskan inferensi ring — sambil memperoleh imbalan kontribusi. Sharding pakar memakai ulang semuanya — peta cakupan, pendaftaran mandiri imbalan-maksimum, unduhan parsial, imbalan per node — hanya mengganti unit cakupan dari *rentang lapisan* menjadi *(lapisan, rentang pakar)*.",
      },
      { t: "h2", kick: "Dua inovasi yang sudah terverifikasi di perangkat", text: "Partisipasi bobot parsial + relay 443" },
      {
        t: "ul",
        items: [
          "**Unduhan bobot parsial berbasis imbalan:** susunan RPC/TP/PP konvensional mengirim checkpoint penuh ke tiap rank dan scheduler mendikte penempatan. Di Kvasir sebuah node hanya mengunduh **irisan yang akan dihitungnya**, dan memilih irisan itu **sendiri, berdasarkan imbalan** — mini-GGUF stage 254 MB versus model penuh 77.6 GB. Beginilah ponsel 4 GB bergabung dengan model yang jauh lebih besar dari dirinya.",
          "**Bidang data relay 443:** edge Cloudflare yang hanya 80/443 plus NAT operator berarti tak ada sambungan langsung di kedua arah. Jembatan WebSocket per edge dengan preamble peran 1 byte membuat **kedua sisi menelepon keluar** (ponsel membuka nol port masuk). Mendaratkannya berarti memperbaiki tiga bug nyata — kesepakatan sidik jari build, autentikasi unduhan node-token, dan bug panjang frame `Int.ushr` Kotlin yang diam-diam merusak tiap frame ≥ 64 KiB (`ushr` hanya memakai 5 bit terendah dari shift; `len ushr 56` menjadi `len ushr 24`) — diperbaiki dengan beralih ke shift `Long`.",
        ],
      },
      { t: "h2", kick: "Inti yang jujur", text: "Jalinan throughput, bukan decoder latensi-rendah" },
      {
        t: "p",
        md: "Dekode adalah 49 lapisan serial, dan satu perjalanan pulang-pergi internet per lapisan memakan 2.5–10 dtk per token. Maka arena swarm adalah **melayani model yang tak bisa di-host siapa pun sendirian**, diukur dengan throughput agregat: dispatch berkelompok mengamortisasi RTT, backbone memelihara cache hot-expert, dan permintaan dirutekan ke replika terdekat. Jalur latensi-rendah tetap milik ring pipeline.",
      },
      { t: "h2", kick: "Peta jalan", text: "M0 → M4" },
      {
        t: "ul",
        items: [
          "**M0** — offload RAM pakar backbone: jalankan 122B di satu koordinator, tanpa bedah graf.",
          "**M1** — bukti expert-parallel satu-host: mini-GGUF irisan pakar + runtime worker + dispatch, logits persis sama dengan monolitik.",
          "**M2** — worker ponsel LAN + NAT menghitung pakar 122B nyata melalui relay 443.",
          "**M3** — pasar cakupan butiran-pakar dengan replika dan fallback churn.",
          "**M4** — throughput: dispatch berkelompok + cache hot-expert, tokens/s berskala dengan jumlah worker.",
        ],
      },
    ],
  },
  "swarm-verified-and-keystone": {
    title: "Dari cetak biru ke perangkat keras: yang terverifikasi, dan batu kunci",
    dek: "Rekap kampanye verifikasi — desain hingga inti M2 dibuktikan pada 122B nyata — dan satu keping integrasi yang membuka sisanya.",
    blocks: [
      {
        t: "p",
        md: "Beberapa pekan terakhir, bagian-bagian sulit dan baru dari swarm pakar dibuktikan satu per satu pada **Qwen3.5-122B** sungguhan — bukan simulasi, bukan ukuran mainan. Inilah jejak verifikasi sejauh ini, dan satu-satunya batu kunci yang tersisa.",
      },
      { t: "img", src: "/blog/swarm-verified-and-keystone.jpg", alt: "A verification trail of stamped checkpoints ending at a keystone being placed" },
      { t: "h2", kick: "Jejaknya · semua terverifikasi pada 122B nyata", text: "Yang sudah mendarat" },
      {
        t: "ul",
        items: [
          "**Desain (5 revisi sejak cetak biru)** — arsitektur EP, partisipasi otonom bobot parsial, relay, spesifikasi kernel worker, dan ekuivalensi numerik lintas-backend yang dikodifikasi sebagai teknologi inti. Otoritas router ditulis sebagai invarian koherensi.",
          "**M0 — offload RAM pakar backbone (planner terverifikasi):** 122B *feasible* di satu koordinator 64 GB — 10 lapisan pakar di-offload ke RAM, VRAM 62.6 GiB / RAM 14.2 GiB, dikawatkan via `--override-tensor`.",
          "**M1 — jalur data irisan pakar:** pengirisan mini-GGUF per pakar (lempengan ne[2], salinan byte tanpa dekuantisasi) + endpoint unduhan `/expert-shard`.",
          "**M1 — oracle numerik:** dispatch + combine == monolitik dengan **max|Δ| = 3.6e-12** pada pakar layer-0 nyata — sharding adalah pengelompokan ulang eksak dari jumlah berbobot yang sama.",
          "**M1 — worker C++ di perangkat keras:** `linkcpp-expert-worker` (ggml/gguf murni) dibangun dan dijalankan di ROCm, **cosine 0.99995** vs oracle; router → dua worker C++ → combine menyamai monolitik pada cosine 0.9997–0.9999.",
          "**Inti M2 — ponsel menghitung pakar 122B nyata:** cross-build Android, dijalankan di SM-S938N, **cosine 0.99992** vs oracle.",
          "**Numerik — matriks ekuivalensi 3 backend:** komputasi 122B yang sama di ROCm × ARM CPU ponsel × numpy — ROCm↔ponsel cosine 0.99990, ROCm↔numpy 0.99996, ponsel↔numpy 0.99992. Semua ekuivalen, tak satu pun bit-identik.",
        ],
      },
      { t: "h2", kick: "Batu kunci", text: "Dispatch backbone, terintegrasi ke dekode langsung" },
      {
        t: "callout",
        md: "Setiap **komponen** — irisan, worker, logika dispatch/combine, ekuivalensi numerik, komputasi ponsel — sudah terverifikasi di perangkat. Yang tersisa adalah mengawatkannya **di dalam dekode mesin inferensi sungguhan**: hook `build_moe_ffn` yang men-dispatch pakar ke node pemiliknya di tengah graf. Itu memerlukan modifikasi submodule mesin inferensi yang dipatok dan beberapa siklus build-verifikasi. Begitu batu kunci ini berdiri, **integrasi relay M2, pasar cakupan pakar M3, dan throughput berkelompok M4** terbuka berurutan — semuanya bergantung pada dispatch ini.",
      },
      {
        t: "p",
        md: "Batu kunci itu kini telah mendarat: tulisan lanjutan tentang M2, M3, M4 dan demo ponsel langsung adalah hasil dari integrasi inilah tepatnya.",
      },
    ],
  },
  "cross-backend-numerical-equivalence": {
    title: "Ekuivalensi numerik lintas backend heterogen",
    dek: "CUDA, ROCm, Adreno, dan CPU tak akan pernah sepakat bit-demi-bit. Bahwa swarm tetap menghasilkan satu model koheren adalah sifat yang dirancang, bukan keberuntungan.",
    blocks: [
      { t: "h2", kick: "Pembedaan kunci", text: "Eksak vs ekuivalen — dua sifat berbeda" },
      {
        t: "ul",
        items: [
          "**Dalam satu backend — eksak (3.6e-12):** membagi pakar antar node lalu menggabungkannya adalah jumlah berbobot yang sama dikelompokkan ulang; satu-satunya perbedaan adalah urutan akumulasi floating-point. Terverifikasi oracle.",
          "**Antar backend — ekuivalen (1e-3…1e-6):** operasi yang sama di perangkat keras berbeda membawa galat relatif per-op sekitar 1e-3–1e-6, dan tak pernah nol. **Swarm hidup di rezim ini.**",
        ],
      },
      {
        t: "p",
        md: "\"Eksak\" adalah yang dijamin dekomposisi di dalam satu perangkat. \"Ekuivalen\" adalah yang diberikan perangkat keras heterogen. Tugas swarm adalah mencegah ekuivalensi menumpuk menjadi divergensi.",
      },
      { t: "h2", kick: "Terukur · 122B nyata, tiga backend", text: "Bukan teori — diukur di perangkat keras" },
      {
        t: "p",
        md: "FFN pakar layer-0 Qwen3.5-122B yang sama, dihitung oleh `linkcpp-expert-worker` di MI250 (**ROCm**), **ARM CPU** ponsel (SM-S938N), dan referensi **numpy** x86 — masukan sama, bobot sama, set instruksi dan urutan reduksi berbeda:",
      },
      { t: "img", src: "/blog/cross-backend-numerical-equivalence.jpg", alt: "Three backends feeding one comparator where their waveforms overlap within tolerance" },
      {
        t: "table",
        head: ["Pasangan backend", "max|Δ|", "cosine"],
        rows: [
          ["ROCm (GPU) vs numpy (x86)", "7.9e-7", "0.99996"],
          ["ARM CPU ponsel vs numpy (x86)", "1.4e-6", "0.99992"],
          ["GPU ROCm vs ARM CPU ponsel", "1.5e-6", "0.99990"],
        ],
      },
      {
        t: "p",
        md: "Tiga set instruksi, satu komputasi — setiap pasangan ekuivalen (cosine ≈ 0.9999), tak ada pasangan yang bit-identik (Δ ≈ 1e-6). Residunya kecil **karena otoritas router mematok masukan dan pemilihan pakar**.",
      },
      {
        t: "p",
        md: "Jalan berikutnya di perangkat keras **NVIDIA GB10 Grace Blackwell** nyata menutup matriks pada backend terakhir: CUDA ↔ ROCm mendarat pada **cosine 1.0000000000** (max abs 3.5e-10, praktis bit-identik, karena kedua backend GPU berbagi sumber kernel), dan CUDA ↔ Grace ARM CPU pada cosine 0.99975 — pola GPU↔CPU yang sama seperti di atas.",
      },
      { t: "h2", kick: "Mengapa backend berbeda", text: "Penjumlahan floating-point tidak asosiatif" },
      {
        t: "ul",
        items: [
          "**Urutan reduksi matmul** — tensor core, tile MFMA, workgroup OpenCL, dan lane SIMD mengakumulasi dalam urutan dan pengubinan berbeda.",
          "**Fusi FMA** — `a*b+c` dibulatkan sekali (FMA) atau dua kali, difusikan berbeda per backend.",
          "**Presisi akumulasi** — penyimpanan F16/BF16 dengan akumulator F32 vs F16 (tuas terbesar divergensi).",
          "**Aproksimasi transendental** — varian polinomial/tabel dari exp (softmax), silu/sigmoid (swiglu), rsqrt (norm).",
          "**Jalur dequant + matmul** — dekuantisasi-lalu-matmul vs kernel terkuantisasi terfusi membulatkan nilai antara secara berbeda.",
          "**Kernel non-deterministik** — reduksi atomic/split-K bisa berbeda antar-run di perangkat yang sama.",
        ],
      },
      { t: "p", md: "Tak satu pun dari ini adalah bug. Ini harga yang dibayar jalur cepat tiap akselerator." },
      { t: "h2", kick: "Mengapa tetap berhasil", text: "Satu otoritas untuk keputusan, presisi cukup untuk akumulasi" },
      {
        t: "callout",
        md: "**OTORITAS ROUTER — invarian inti.** Satu-satunya keputusan diskret dalam jaringan adalah perutean MoE (top-8 dari 256). Jika tiap backend menjalankan ulang router, token perbatasan akan memilih **pakar yang berbeda** dan benar-benar divergen. Kvasir menjalankan router **sekali, di backbone**, dan hanya mengirim id pakar terpilih ke worker. Swarm heterogen boleh berbeda pada *besaran* keluaran tiap pakar — tak pernah berbeda pada *pakar mana yang berjalan*. Ini mengubah divergensi diskret katastrofik menjadi galat kontinu terbatas, dan merupakan aturan koherensi sharding pakar heterogen.",
      },
      {
        t: "ul",
        items: [
          "**Argmax diskret:** dekode adalah argmax atas logits. Goyangan 1e-3 hanya membalik token bila dua kandidat berjarak kurang dari 1e-3 — di sebagian besar posisi marginnya jauh lebih besar, sehingga **token keluar identik**; pembalikan langka terjadi di posisi seambigu seed yang berbeda.",
          "**Combine adalah penjumlahan:** hasil parsial melebur sebagai **jumlah** berbobot probabilitas. Galat independen ~1e-4 bertambah secara inkoheren — tumbuh seperti √k, bukan k — dan tak ada pembatalan nilai besar, sehingga residu tetap terkondisi baik.",
        ],
      },
      { t: "h2", kick: "Di mana bisa rusak · dan aturan yang menghentikannya", text: "Mode divergensi dan pertahanan" },
      {
        t: "table",
        head: ["Mode divergensi", "Mekanisme", "Aturan"],
        rows: [
          ["Ketidakcocokan perutean", "Backend memilih top-8 berbeda untuk token perbatasan", "Otoritas router — diputuskan sekali di backbone, id di-dispatch"],
          ["Percabangan lintasan", "Goyangan logit per token akhirnya membalik satu token; urutan bercabang seperti seed baru", "Dekode/sampling dipatok ke satu node"],
          ["Akumulasi kedalaman", "49 lapisan × ~1e-4 masing-masing → hingga 1e-2 di logits akhir", "Akumulasi F32 di perbatasan dan combine"],
          ["Non-determinisme diri", "Kernel atomic/split-K bervariasi antar-run", "Kernel combine deterministik; verifikasi memakai toleransi"],
          ["Ketidakcocokan presisi", "Satu node mengakumulasi F16, yang lain F32", "Presisi akumulasi diiklankan sebagai kapabilitas; node F32 diutamakan untuk rank keluaran"],
        ],
      },
      { t: "h2", kick: "Ekuivalensi adalah angka", text: "Protokol pengukuran" },
      {
        t: "ul",
        items: [
          "**Delta per-op** — masukan sama, galat relatif A vs B pada matmul, swiglu, softmax, norm.",
          "**Drift perbatasan lapisan** — delta residual setelah satu lapisan, ditumpuk untuk melihat apakah kedalaman terakumulasi sebagai √L atau L.",
          "**Divergensi logit ujung-ke-ujung** — L∞, L2, dan **divergensi KL** atas forward penuh.",
          "**Kesepakatan keputusan** — kesepakatan token top-1 plus kesepakatan perutean top-8 (memvalidasi mengapa otoritas router diperlukan).",
          "**Stabilitas generasi** — greedy N token; indeks pertama tempat A dan B berbeda.",
          "**Tingkat tugas** — delta perplexity dan skor evaluasi: satu-satunya metrik yang benar-benar dirasakan pengguna.",
        ],
      },
      {
        t: "p",
        md: "Kelulusan adalah **toleransi** — \"kesepakatan top-1 ≥ 99.x%, KL ≤ ε\". Node di luar toleransi ditandai tak layak untuk rank sensitif, bukan ditolak mentah-mentah.",
      },
      { t: "h2", kick: "Mengapa ini teknologi inti swarm", text: "Kesepakatan bit mustahil — dan tak perlu" },
      {
        t: "p",
        md: "Klaster homogen bisa mengasumsikan ketepatan bit; swarm tidak — premisnya adalah *perangkat keras apa pun yang datang*. Maka Kvasir memperlakukan ekuivalensi numerik persis seperti kompatibilitas protokol: **kontrak kelas satu yang terukur**. Backend dan presisi akumulasi diiklankan sebagai kapabilitas node, otoritas router ditegakkan sebagai invarian, dan setiap verifikasi memakai toleransi alih-alih kesetaraan bit. **Ekuivalensi numerik terukur + keputusan diskret otoritas tunggal** — itulah yang membuat satu model berjalan di setiap GPU di bumi sekaligus. Itulah swarm.",
      },
    ],
  },
  "blackwell-joins-the-swarm": {
    title: "NVIDIA Blackwell bergabung ke swarm",
    dek: "Sebuah GB10 Grace Blackwell menghitung irisan FFN pakar 122B nyata di CUDA dan menyamai AMD ROCm bit demi bit (cosine 1.0000000000), serta Grace ARM CPU dalam toleransi. Matriks lintas-backend telah lengkap.",
    blocks: [
      {
        t: "p",
        md: "Premis sebuah swarm adalah *perangkat keras apa pun yang datang*. Ekuivalensi numerik — bukti bahwa worker CUDA, ROCm, Adreno, dan CPU semuanya mengeluarkan token yang sama — sudah terukur di ROCm, ARM ponsel, dan numpy. NVIDIA adalah jalur **default dan paling teroptimasi** di ggml/mesin inferensi standar, tapi ia satu-satunya backend yang matriksnya belum ditutup. Menjalankan perangkat keras Blackwell nyata menutupnya.",
      },
      {
        t: "callout",
        md: "**GB10 Blackwell CUDA ↔ MI250 ROCm gfx90a: cosine 1.0000000000** — max abs diff 3.5×10⁻¹⁰. Pada irisan pakar layer-0 Qwen3.5-122B nyata yang sama, dua backend GPU praktis bit-identik.",
      },
      { t: "img", src: "/blog/blackwell-joins-the-swarm.jpg", alt: "A new GPU docking into an almost-complete matrix of backend-comparison cells, its waveform snapping into overlap with a red GPU's" },
      { t: "h2", kick: "Terukur · Qwen3.5-122B-A10B nyata, irisan pakar layer-0", text: "Matriks lintas-backend" },
      {
        t: "table",
        head: ["Perbandingan", "Perangkat keras", "cosine", "max abs"],
        rows: [
          ["CUDA ↔ ROCm", "GB10 Blackwell ↔ MI250 gfx90a", "1.0000000000", "3.5e-10"],
          ["CUDA ↔ CPU", "GB10 Blackwell ↔ Grace ARM", "0.9997525825", "2.6e-05"],
          ["CPU ↔ ROCm", "Grace ARM ↔ MI250 gfx90a", "0.9997525823", "2.6e-05"],
        ],
      },
      {
        t: "p",
        md: "Dua backend GPU (CUDA, ROCm) berbagi sumber kernel, sehingga mendarat **praktis bit-identik** (10⁻¹⁰). GPU↔CPU membawa gangguan ~10⁻³ per op karena urutan akumulasi berbeda, tapi tetap ekuivalen pada **cosine 0.99975** — pola yang sama dengan ROCm↔ARM-ponsel 0.99992 sebelumnya. Prinsip otoritas router kembali berlaku di NVIDIA: **keputusan diskret (argmax, pemilihan pakar) invarian di atas gangguan kontinu ini.**",
      },
      { t: "h2", kick: "Penyiapan", text: "Apa yang berjalan, di atas apa" },
      {
        t: "ul",
        items: [
          "**Perangkat** — NVIDIA GB10 (Grace Blackwell), aarch64, compute 12.1 / sm_121a, 124,5 GB memori terpadu.",
          "**Toolkit** — CUDA 13.0.88 · gcc 13.3 · ggml 0.15.3; expert-worker ggml/gguf murni dibangun dengan kernel Blackwell.",
          "**Model** — Qwen3.5-122B-A10B-Q4_K_M, layer-0 semua pakar (256 pakar, n_embd 3072, n_ff 1024, Q4_K/Q6_K).",
          "**Metode** — irisan L0 1,58 GB dialirkan MI250 → GB10 (perbandingan tanpa rugi); masukan yang sama (h/ids) dijalankan lewat CUDA, CPU, dan ROCm; vektor keluaran float32 (36.864) dibandingkan dengan cosine, L2 relatif, dan max-abs.",
        ],
      },
      {
        t: "callout",
        md: "**Satu jebakan perangkat nyata:** GPU terintegrasi GB10 diklasifikasikan ggml sebagai tipe perangkat `ACCEL`, bukan `GPU` — sehingga `init_by_type(GPU)` tak menemukan apa pun. Diperbaiki dengan memilih perangkat non-CPU pertama alih-alih mengeraskan tipe GPU.",
      },
      { t: "h2", kick: "Mengapa penting", text: "Matriks telah ditutup" },
      {
        t: "p",
        md: "Agar worker heterogen melayani satu model, mesin CUDA dan mesin ROCm harus **dapat dipertukarkan**, dan GPU serta CPU harus **ekuivalen secara numerik**. Dengan Blackwell terukur, keduanya berlaku di seluruh matriks backend: worker CUDA↔ROCm bisa saling menggantikan, dan worker GPU↔CPU sepakat dalam toleransi yang terbatas dan terkondisi baik. Akselerator paling umum di bumi kini adalah warga swarm yang terverifikasi.",
      },
    ],
  },
  "securing-the-kvr-money-path": {
    title: "Memperkeras jalur uang: keamanan transaksi di settlement KVR",
    dek: "Tiga kelas kerentanan nyata — replay tanda tangan pembayaran, pencetakan imbalan tanpa autentikasi, dan double-spend karena race — ditemukan, dieksploitasi dalam tes, dan ditutup di layanan settlement gateway.",
    blocks: [
      {
        t: "p",
        md: "Di sebuah DePIN, jalur uang sama bermusuhannya dengan jalur komputasi: setiap endpoint yang mengkredit KVR cepat atau lambat akan dijajal oleh seseorang yang ingin KVR tanpa bekerja. Pemeriksaan keamanan atas layanan settlement gateway — proses yang memverifikasi pembayaran on-chain dan mengkredit stake, imbalan node, serta biaya inferensi — menemukan dan menutup **tiga kelas kerentanan nyata**. Masing-masing didemonstrasikan dengan tes bergaya exploit sebelum perbaikan dan diverifikasi ulang sesudahnya.",
      },
      { t: "h2", kick: "Model kepercayaan", text: "Verifikasi fakta on-chain, bukan klaim klien" },
      {
        t: "p",
        md: "Model kustodi Kvasir menaruh kunci pada pengguna: dompet menandatangani transaksi, Solana mencatatnya, dan tugas layanan settlement adalah **memverifikasi apa yang benar-benar terjadi di chain** sebelum menyentuh saldo. Pembayaran mengikuti *quote → payment → inference*, dengan setiap tanda tangan transaksi yang terpakai dicatat di registri sekali-pakai `usedSignatures` sehingga tak pernah bisa diajukan dua kali. Itu menjadikan layanan settlement titik sempit — dan aturan yang tak boleh dilanggarnya: hanya kreditkan yang dibuktikan chain, jangan pernah yang diklaim klien. Layanan ini memang memegang dana di devnet: KVR yang di-stake dan kredit prabayar berada di treasury-nya dan dicatat di buku besarnya hingga program staking on-chain dirilis.",
      },
      { t: "img", src: "/blog/securing-the-kvr-money-path.jpg", alt: "A settlement vault guarded by three locks: sender binding, trusted reporter, and a serialization gate" },
      { t: "h2", kick: "Perbaikan #1 · pengikatan pengirim", text: "Ikat pembayaran ke pembayarnya" },
      {
        t: "p",
        md: "Tanda tangan Solana bersifat **publik**. Jalur verifikasi stake hanya memeriksa bahwa vault *menerima* KVR yang diharapkan — tak pernah *siapa yang mengirimnya*. Penyerang bisa mengamati transfer KVR→vault milik korban di devnet, lalu mengajukan `{owner: penyerang, signature: milik korban}`: pemeriksaan penerimaan vault lolos, pokok dikreditkan ke penyerang, dan satu unstake kemudian dananya jadi milik penyerang. Pencurian langsung, hanya bermodal block explorer.",
      },
      {
        t: "code",
        caption: "Perbaikannya: KVR harus didebit dari akun token yang dimiliki owner yang dikreditkan.",
        code: `verifyStakeTransfer(signature, owner, amount):
  delta(vault)  >= amount            # vault actually received it (old check)
  Σ debits from token accounts
    whose owner == credited owner    # NEW — sender binding
                >= amount            # summed across that owner's accounts
  # inference path (no owner): bound by private requestId
  # + one-shot usedSignatures instead`,
      },
      { t: "h2", kick: "Perbaikan #2 · pelapor tepercaya", text: "Imbalan hanya dari sumber terautentikasi" },
      {
        t: "p",
        md: "Endpoint imbalan node mencetak KVR yang dapat diklaim dari **masukan yang dilaporkan sendiri**: `POST /api/node/contribution` mengkredit `units` apa pun yang diklaim klien — `units: 1e9` plus satu panggilan claim bisa menguras vault — dan register/heartbeat mempercayai peran hub/gateway yang diaku sendiri (imbalan infra per jam) serta skor performa (pengali imbalan). Perbaikannya menempatkan setiap pernyataan yang memengaruhi imbalan di balik **pelapor tepercaya**: hanya token layanan M2M yang dipakai poll kontribusi hub, atau admin terautentikasi, yang boleh menyatakan units, peran infra, atau tingkat performa — ditegakkan bahkan dalam mode LAN terbuka, karena ini mencetak KVR. Perbandingan token berjalan waktu-konstan, dan penautan dompet↔node tetap bebas; ia hanya tak bisa lagi menyatakan imbalannya sendiri.",
      },
      { t: "h2", kick: "Perbaikan #3 · serialisasi settlement", text: "Satu penulis untuk tiap saldo" },
      {
        t: "p",
        md: "Status settlement adalah read-modify-write tanpa kunci, dan setiap operasi uang *await* pembayaran atau verifikasi on-chain di tengah jalan — menyerahkan event loop dengan saldo basi di tangan. Dua claim serentak bisa membaca saldo tertunda 100 KVR yang sama dan sama-sama membayarkannya. Bukan teori: tes exploit menunjukkan **tiga claim serentak membayar 300 untuk saldo 100**.",
      },
      {
        t: "code",
        caption: "Serialisasi asinkron per kunci: operasi uang dengan kunci sama berjalan ketat berurutan.",
        code: `withLock(key, fn)         # per-key promise chain, self-cleaning map
  stake / unstake / claim  → keyed by owner
  inference settlement     → keyed by requestId
inside the lock:
  usedSignatures check + credit   # no same-signature double-credit
  pay out FIRST, then debit       # failed payout leaves balance intact`,
      },
      { t: "h2", kick: "Pertahanan berlapis", text: "Posisi tiap lapisan sekarang" },
      {
        t: "table",
        head: ["Lapisan", "Mekanisme"],
        rows: [
          ["Identitas", "Login tanda tangan dompet SIWS atas nonce server + 2FA TOTP + kode cadangan sekali-pakai"],
          ["Transport", "Node token turunan dompet pada unduhan shard; kesepakatan sidik jari build di relay"],
          ["Pembayaran", "Pengikatan pengirim pada transfer stake; usedSignatures sekali-pakai; requestId privat pada inferensi"],
          ["Settlement", "Kunci per-kunci di setiap penulisan saldo; bayar dulu baru debit; pengajuan ulang idempoten"],
          ["Pelaporan", "Fakta yang memengaruhi imbalan hanya dari token layanan M2M atau admin, dibandingkan waktu-konstan"],
          ["Kustodi", "Dompet non-kustodial — layanan hanya bisa memindahkan isi vault, tak pernah kunci pengguna"],
        ],
      },
      { t: "h2", kick: "Diukur, bukan diasumsikan", text: "Setiap perbaikan membawa tes exploit-nya sendiri" },
      {
        t: "ul",
        items: [
          "Memutar ulang tanda tangan transfer korban di bawah dompet penyerang kini ditolak (\"not sent by owner\"); stake sah, klaim-berlebih, dan pembayaran inferensi berperilaku tak berubah.",
          "Tiga claim serentak terhadap satu saldo membayar **tepat sekali**; mengajukan ulang inferensi yang sudah dibayar mengembalikan hasil sama secara idempoten.",
          "`units`, peran hub/gateway, dan tingkat performa yang diaku sendiri oleh klien tanpa autentikasi tak lagi menggerakkan satu lamport imbalan pun.",
        ],
      },
      {
        t: "p",
        md: "Benang merah ketiga perbaikan adalah satu prinsip yang diterapkan tiga cara: **chain adalah sumber kebenaran, layanan adalah verifikator, dan tiap saldo punya tepat satu penulis**. Layanan settlement masih berjalan di Solana devnet — persis tempat Anda ingin menemukan, mengeksploitasi, dan memperbaiki kelas-kelas ini sebelum mainnet menaikkan taruhannya.",
      },
    ],
  },
  "linkcpp-control-plane": {
    title: "Phase 0 — Mesinnya: linkcpp, bidang kendali untuk mesin inferensi",
    dek: "mesin inferensi membawa bidang data RPC yang cakap tapi tanpa bidang kendali. linkcpp menambahkan separuh yang hilang — penemuan, perencanaan, peluncuran, dan gateway — di sekeliling binari yang dibangun dekat dengan upstream.",
    blocks: [
      {
        t: "p",
        md: "Semua yang dijalankan Kvasir bermula di sini. **linkcpp** adalah bidang kendali bersumber tersedia (Business Source License) di sekeliling bidang data RPC mesin inferensi: ia menjalankan model AI besar di banyak GPU dan mesin memakai binari `ggml-rpc-server` / `llama-server` yang dibangun dekat dengan upstream. Bidang datanya hanya membawa sekumpulan kecil patch — semua hal lain yang ditambahkan linkcpp adalah orkestrasi.",
      },
      { t: "h2", kick: "Celahnya", text: "Bidang data tanpa bidang kendali" },
      {
        t: "p",
        md: "mesin inferensi sudah bisa membagi model antar mesin lewat RPC — tapi seseorang harus menemukan GPU, memutuskan lapisan mana ke mana, meluncurkan worker yang tepat dengan anggaran yang tepat, memeriksa bahwa tiap node berbicara protokol yang sama, dan mengekspos API yang benar-benar bisa dipanggil pengembang. Melakukannya manual untuk satu klaster merepotkan; melakukannya untuk jaringan terbuka berisi perangkat orang asing mustahil. Lapisan koordinasi itulah linkcpp.",
      },
      { t: "h2", kick: "Arsitektur", text: "Satu hub, worker berbasis upstream, gateway standar" },
      {
        t: "code",
        caption: "Alur permintaan — hub mengorkestrasi, binari berbasis upstream menghitung.",
        code: `browser / SDK
  → hub :19000                      # FastAPI control plane (single Docker image)
  → GPU-less llama-server master    # per-controller, :8080+
  → ggml-rpc-server workers         # local slots, remote units, managed agents`,
      },
      { t: "img", src: "/blog/linkcpp-control-plane.jpg", alt: "A control deck orchestrating rows of inference engines below" },
      {
        t: "ul",
        items: [
          "**Tiga cara mesin bergabung:** **slot node lokal** tetap dengan anggaran VRAM/RAM/CPU yang dapat diedit; **unit jarak jauh** — daftarkan hub lain dan impor node-nodenya; dan **agen node terkelola** — layanan khusus-worker yang bergabung lewat HTTP request/response sederhana, sengaja tanpa stream persisten agar bertahan di perutean LAN/VPN sederhana.",
          "**Gerbang kompatibilitas kelas satu:** tiap unit, node, dan agen melaporkan identitas protokol/runtime-pack plus detail backend. Ketidakcocokan unit, runtime-pack, revisi mesin inferensi, dan ABI RPC **diblokir keras sebelum bind, plan, load, atau infer** — perbedaan backend (CUDA/Metal/Vulkan/CPU) dicatat sebagai kapabilitas, bukan penolakan.",
          "**Planner** membaca metadata GGUF dan menghasilkan penempatan lapisan bersambung per node, `--tensor-split`, perkiraan VRAM KV-cache/lapisan/pakar, dan offload FFN pakar opsional ke RAM.",
          "**Gateway:** tiap controller mengekspos endpoint kompatibel OpenAI (`/v1/chat/completions`, `/v1/responses`, `/v1/models`) dan kompatibel Anthropic (`/anthropic/v1/messages|models`), ditopang model termuat yang sama — klien yang ada bekerja tanpa perubahan.",
        ],
      },
      {
        t: "p",
        md: "Pemisahan yang disengaja ini — bidang data yang tetap dekat dengan upstream di bawah bidang kendali bersumber tersedia — adalah fondasi semua yang menyusul: ring runtime, pasar lapisan, dan akhirnya swarm pakar semuanya adalah evolusi bidang kendali di atas komputasi yang sama.",
      },
    ],
  },
  "ring-topology-pipeline-inference": {
    title: "Phase 1 — Ring: inferensi pipeline tanpa master",
    dek: "Setiap perangkat hanya memuat jendela lapisannya dan meneruskan batas hidden-state kecil ke tetangganya. Tak ada node yang harus memegang seluruh model, dan ring tak memiliki master pusat.",
    blocks: [
      { t: "h2", kick: "Mengapa bukan bintang", text: "Master RPC adalah leher botol sekaligus penjaga gerbang" },
      {
        t: "p",
        md: "Pada topologi RPC klasik, satu master membuka **GGUF utuh** dan menelepon setiap worker. Bentuk itu rusak di jaringan terbuka dalam tiga hal: master harus memegang dan melayani checkpoint utuh; setiap worker harus bisa ditelepon — ponsel di balik NAT operator tidak bisa; dan master menjadi pemilik tunggal di jaringan yang seharusnya tak punya pemilik.",
      },
      { t: "h2", kick: "Ring", text: "Jendela lapisan + penerusan batas" },
      {
        t: "ul",
        items: [
          "Setiap perangkat menyimpan model yang sama tapi **hanya memuat jendela lapisan bersambungnya**, lalu membuka tepat dua tautan: satu ke pendahulu, satu ke penerus.",
          "Permintaan masuk ke ring; tiap node menjalankan lapisannya dan hanya meneruskan **batas hidden-state** ke tetangganya. Rank terakhir menyampel token dan mengirimnya balik — tanpa master pusat di jalur data, dan tak ada node yang memegang model utuh.",
          "Penempatan berasal dari **rank manifest** planner — untuk Qwen3.5-122B, 49 lapisan dibagi ke campuran GPU, CPU, NPU, dan ponsel apa pun yang muncul.",
        ],
      },
      { t: "img", src: "/blog/ring-topology-pipeline-inference.jpg", alt: "A transit-map style loop of device stations passing packet trains" },
      { t: "h2", kick: "Menjadikan perangkat lemah anggota sejati", text: "Shard parsial, GPU seluler, dan relay 443" },
      {
        t: "ul",
        items: [
          "**Unduhan shard parsial:** stage ring tak butuh checkpoint — ia butuh jendelanya. Mini-GGUF stage hanya membawa tensor-tensor itu (**254 MB berisi 26 tensor** versus model penuh 77.6 GB), sehingga ponsel menarik ~1.5 GB untuk jendela satu-lapisan alih-alih semuanya.",
          "**Jalur GPU seluler:** rute RPC ke GPU ponsel terbukti tak layak (tata letak buffer OpenCL Adreno tak selamat dari serialisasi RPC), tapi **stage ring berjalan langsung di GPU Adreno** — stage memiliki backend-nya secara lokal, jadi hanya batas yang menyeberangi kabel.",
          "**Penembusan NAT:** ponsel tak bisa menerima koneksi masuk, maka bidang data berjalan lewat **relay 443** — jembatan WebSocket per edge dengan preamble peran 1 byte yang membuat kedua ujung menelepon keluar. Ponsel membuka nol port masuk.",
          "**Pasar pendaftaran mandiri:** stage diklaim, bukan ditugaskan. Node mem-poll peta cakupan/permintaan, memilih jendela tak tercakup **berimbalan tertinggi**, mengunduh jendela itu, dan bergabung — terverifikasi ujung-ke-ujung dengan ponsel di balik NAT yang menuntaskan inferensi ring dan memperoleh kontribusinya.",
        ],
      },
      { t: "h2", kick: "Tempat ring berada", text: "Jalur latensi-rendah" },
      {
        t: "p",
        md: "Ring adalah jalur **latensi** Kvasir: batasnya kecil, lompatannya sedikit, dan dekode mengalir mengelilingi lingkaran tanpa mengumpulkan apa pun secara terpusat. Keterbatasannya adalah granularitas — unit terkecil yang bisa dibawa node adalah satu lapisan (~1.4 GB pada 122B). Menghapus lantai itulah yang dilakukan swarm pakar; ring tetap menjadi tulang punggung penyajian tempat ia tersambung.",
      },
    ],
  },
  "inside-a-122b-moe": {
    title: "Phase 2 — Di dalam MoE 122B: mengapa bobot ingin di-shard",
    dek: "Analisis tingkat tensor Qwen3.5-122B: 86% byte-nya adalah 12.544 lempengan pakar independen, masing-masing tinggal satu salinan rentang byte bersih dari berdiri sendiri.",
    blocks: [
      {
        t: "p",
        md: "Sebelum merancang apa pun, kami membongkar 122B di disk. Pertanyaannya: jika swarm perangkat lemah hendak memikul model ini, apa unit angkut alaminya? Jawabannya jatuh dari tata letak tensor GGUF itu sendiri.",
      },
      { t: "h2", kick: "Anatomi · Qwen3.5-122B-A10B (Q4_K_M)", text: "Dari apa sebenarnya satu lapisan MoE tersusun" },
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
      { t: "img", src: "/blog/inside-a-122b-moe.jpg", alt: "Anatomical cutaway of a MoE model: slim dense spine beside a huge honeycomb of experts" },
      {
        t: "p",
        md: "Tiap lapisan terbagi menjadi **jalur padat** — attention + KV, norm-norm, router (`ffn_gate_inp`), pakar bersama — dan **bank pakar**: 256 FFN independen yang disimpan sebagai tiga tensor bertumpuk (`ffn_up_exps`, `ffn_gate_exps`, `ffn_down_exps`). Jalur padat adalah minoritas byte; bank pakar adalah 86% model.",
      },
      { t: "h2", kick: "Hadiah tata letak", text: "Pakar adalah lempengan bersambung yang selaras blok" },
      {
        t: "ul",
        items: [
          "Indeks pakar adalah **dimensi ggml terluar** (`ne[2]`) dari tiap tensor pakar — pakar *e* menempati satu lempengan bersambung, selaras blok-kuantisasi, berisi byte terkuantisasi mentah.",
          "Itu membuat ekstraksi per pakar menjadi **salinan rentang byte**: `data[a:b]`, tanpa dekuantisasi, tanpa pengemasan ulang — mini-GGUF irisan pakar murah dibuat dan setia-bit.",
          "Per token hanya **8 dari 256** pakar menyala per lapisan, dipilih router — jadi saat dekode, lalu lintas pakar satu lapisan hanyalah segelintir perkalian matriks kecil atas satu vektor hidden.",
        ],
      },
      { t: "h2", kick: "Implikasinya", text: "Unit angkut turun dari 1.4 GB ke 5.3 MB" },
      {
        t: "p",
        md: "Pada butiran lapisan, minimum yang bisa dipegang node adalah ~**1.4 GB** — di luar jangkauan kebanyakan ponsel begitu aplikasi, KV, dan OS mengambil bagiannya. Pada butiran pakar, unitnya **5.3 MB**, dan kontribusi realistisnya 8–64 pakar (**42–340 MB**) — lapang di perangkat modern mana pun. Para pakar saling independen, sehingga kepemilikan bisa disebar sembarang dan diseimbangkan ulang bebas. Analisis inilah yang menjadikan sharding tingkat-pakar sebagai taruhan desain: bobotnya sudah terkemas dalam unit seukuran swarm — jaringan tinggal menghormati kemasannya.",
      },
    ],
  },
  "m0-backbone-expert-ram-offload": {
    title: "Phase 3 — Offload RAM pakar backbone (M0)",
    dek: "Alirkan FFN pakar MoE dari RAM CPU alih-alih VRAM, dan satu koordinator 64 GB menampung 122B — tanpa bedah graf.",
    blocks: [
      {
        t: "p",
        md: "FFN pakar tak harus tinggal di VRAM. Mengalirkannya dari RAM CPU membuat satu koordinator menampung model yang pakarnya melebihi VRAM-nya — fondasi yang membuat node lemah bisa bergabung dengan MoE besar.",
      },
      { t: "h2", kick: "Planner terverifikasi · GGUF 122B nyata", text: "122B muat di satu koordinator 64 GB" },
      {
        t: "p",
        md: "Sebelumnya ring menempatkan bobot hanya di VRAM, sehingga 122B (77.6 GB) **infeasible** di GCD 64 GB. Dengan aturan offload pakar, dry-run kembali **feasible**:",
      },
      {
        t: "stats",
        items: [
          { n: "feasible", l: "rencana ring 122B" },
          { n: "62.6", l: "VRAM GiB (≤ 64)" },
          { n: "14.2", l: "RAM GiB (pakar)" },
          { n: "10", l: "lapisan di-offload" },
        ],
      },
      { t: "img", src: "/blog/m0-backbone-expert-ram-offload.jpg", alt: "A coordinator siphoning expert tiles from VRAM into a RAM reservoir, stamped feasible" },
      {
        t: "code",
        caption: "Keluaran planner — format aturan -ot mesin inferensi.",
        code: `node 0  layers [0,48]  vram=62.6  ram=14.2  ot_rules=10
sample: blk\\.38\\.ffn_(up|down|gate)_(ch|)exps=CPU   # inference engine -ot format`,
      },
      { t: "h2", kick: "Apa yang dikawatkan · Python murni, tanpa rebuild C++", text: "Membawa aturan offload planner ke pemuatan nyata" },
      {
        t: "ul",
        items: [
          "**planner** — sudah mengeluarkan `ot` (aturan `-ot` gabungan koma) di tiap penempatan.",
          "**protocol.py** — bidang `StageStartRequest.ot` ditambahkan.",
          "**runtime.py** — meneruskan `ot` penempatan ke permintaan stage.",
          "**stage_service.py** — koordinator diluncurkan dengan `--override-tensor`.",
          "`linkcpp-server` meneruskan argumen tak dikenal ke llama-server standar, jadi `-ot` berlaku apa adanya.",
        ],
      },
      {
        t: "code",
        code: `# stage_service.py — coordinator branch
if request.ot:
    command += ["--override-tensor", request.ot]`,
      },
      {
        t: "p",
        md: "Diverifikasi dengan tes bolak-balik protokol `ot` plus konfirmasi bahwa perintah koordinator mengeluarkan `--override-tensor`; hub di-deploy ulang bersih tanpa regresi. Yang tersisa saat itu: pemuatan penuh 77 GB dua-node (koordinator dengan offload + ponsel memegang jendela satu-lapisan ~1.5 GB), menunggu ketersediaan server. Inti M0 — offload backbone yang membuat node lemah dapat berpartisipasi di MoE besar — sudah tuntas di tingkat kode dan planner.",
      },
    ],
  },
  "m1-expert-slice-data-path": {
    title: "Phase 4 — Jalur data irisan pakar (M1)",
    dek: "Perangkat lemah mengunduh beberapa pakar 6 MB, bukan lapisan 1.4 GB — dan komputasi sharded menyamai monolitik hingga 3.6e-12.",
    blocks: [
      { t: "h2", kick: "Terverifikasi · Qwen3.5-122B-A10B nyata", text: "Irisan pakar adalah salinan byte — tanpa dekuantisasi" },
      {
        t: "stats",
        items: [
          { n: "256→8", l: "irisan dim pakar" },
          { n: "~6.1", l: "MB / pakar (Q4+Q6)" },
          { n: "206 MB", l: "unduhan 2 lapisan × 16 pakar" },
          { n: "200", l: "HTTP, GGUF valid" },
        ],
      },
      {
        t: "p",
        md: "Tensor pakar MoE menumpuk semua pakar sepanjang dimensi ggml terluar, sehingga pembaca mengekspos `(n_expert, rows, row_bytes)` byte terkuantisasi mentah. Pakar *e* adalah lempengan bersambung selaras blok-kuantisasi — irisannya secara harfiah `data[a:b]`, tanpa dekuantisasi dan tanpa pengemasan ulang.",
      },
      {
        t: "code",
        caption: "write_expert_shard_gguf — bolak-balik yang terverifikasi.",
        code: `sliced = tensor.data[a:b]              # outermost axis = expert
writer.add_tensor(name, sliced, raw_dtype=tensor.tensor_type)
# router (ffn_gate_inp) & shared expert stay on the backbone → excluded
GET /api/proxy/models/{m}/expert-shard?layers=0:2&experts=0:16  # node-token authed`,
      },
      { t: "img", src: "/blog/m1-expert-slice-data-path.jpg", alt: "A laser slicing one expert slab into a mini-GGUF beside a perfectly level balance scale" },
      { t: "h2", kick: "Oracle numerik", text: "dispatch + combine == monolitik, persis" },
      {
        t: "p",
        md: "Dengan pakar layer-0 122B nyata (referensi terdekuantisasi), membagi pakar menjadi 4 shard, menghitung masing-masing secara terpisah lalu menggabungkan **menyamai FFN MoE monolitik**: sharding adalah pengelompokan ulang eksak dari jumlah berbobot yang sama, bukan aproksimasi.",
      },
      {
        t: "stats",
        items: [
          { n: "3.6e-12", l: "max|mono − sharded|" },
          { n: "1.2e-07", l: "galat relatif" },
          { n: "True", l: "allclose(1e-5)" },
          { n: "28/256", l: "pakar tersentuh" },
        ],
      },
      { t: "h2", kick: "Worker C++, terverifikasi di perangkat keras", text: "linkcpp-expert-worker mereproduksi oracle di ROCm" },
      {
        t: "ul",
        items: [
          "**ggml/gguf murni** (tanpa libllama): memuat irisan ke backend GPU dan menjalankan `mul_mat_id(up/gate) → swiglu → mul_mat_id(down)`.",
          "**Build + run ROCm** di MI250: 122B layer-0, pakar [0,8), 4 token.",
          "**Cosine 0.99995 vs oracle**, allclose(1e-3) = True, max|Δ| = 7.9e-7 — residu ini sendiri adalah kasus terukur pertama ekuivalensi lintas-backend (ROCm vs numpy).",
          "Jalur kode yang sama mencakup CUDA/Metal/Vulkan/CPU (`mul_mat_id`/`swiglu` adalah ggml standar; CUDA punya kernel MoE khusus).",
        ],
      },
      {
        t: "p",
        md: "Bagian tersulit dan paling berisiko — kernel worker di perangkat — terverifikasi di sini. Yang tersisa adalah orkestrasi backbone↔worker; worker adalah fungsi murni terbukti yang mengonsumsi irisan-irisan ini.",
      },
    ],
  },
  "m2-distributed-expert-dispatch": {
    title: "Phase 5 — Dispatch pakar terdistribusi (M2)",
    dek: "Dekode 122B langsung menyerahkan komputasi pakar satu lapisan ke proses worker terpisah lewat TCP — dan memprediksi token yang persis sama.",
    blocks: [
      { t: "h2", kick: "Terverifikasi · 122B nyata, dua proses", text: "Dekode backbone → TCP → worker → experts → token sama" },
      {
        t: "stats",
        items: [
          { n: "MATCH", l: "argmax OFF == ON (11751)" },
          { n: "0.99869", l: "cosine logit" },
          { n: "0", l: "rugi transport (byte-identical)" },
          { n: "2", l: "proses (backbone + worker)" },
        ],
      },
      {
        t: "p",
        md: "Worker pakar melayani irisan layer-0 sebagai **proses terpisah** (ROCm), dan callback dispatch `build_moe_ffn` milik backbone 122B mengirim `(cur, sel)` lewat TCP dan menerima keluaran pakar. Cosine logit-nya **persis nilai in-process** (0.99868775) — transportnya tanpa rugi. Komputasi swarm expert-parallel bekerja melintasi batas proses.",
      },
      { t: "img", src: "/blog/m2-distributed-expert-dispatch.jpg", alt: "Backbone and worker rooms joined by one TCP pipe, sealed with an argmax MATCH stamp" },
      {
        t: "code",
        caption: "Satu koneksi TCP berumur panjang — stream yang sama yang bisa diterowongkan ring/relay 443.",
        code: `# worker: serving as a separate process
linkcpp-expert-worker --serve 52700 --model L0_all.gguf --layer 0 --n-embd 3072
# backbone: build_moe_ffn callback dispatches to the worker
linkcpp-moe-verify 122B.gguf ... --dispatch-port 52700
  → protocol: [n_used, n_tokens] + cur + sel  →  experts`,
      },
      { t: "h2", kick: "Selesai", text: "Pipeline dispatch terdistribusi" },
      {
        t: "ul",
        items: [
          "Mode `--serve`: muat irisan, dengarkan TCP, jawab `(n_used, n_tokens, cur, sel) → experts`.",
          "`--dispatch-port`: callback backbone mengirim/menerima lewat TCP ke worker terpisah, menggantikan komputasi in-process.",
          "Diukur pada dekode 122B langsung dengan layer-0 di-dispatch keluar proses → **argmax MATCH**, cosine 0.99869 (= in-process, tanpa rugi).",
          "Inti M2 (lebih awal): ARM ponsel menghitung pakar 122B nyata pada cosine 0.99992 (cross-build Android).",
        ],
      },
      {
        t: "p",
        md: "Berikutnya dari sini: menerowongkan stream TCP yang sama lewat **relay 443** ke worker di mesin dan ponsel lain (transportnya sudah terbukti di pekerjaan ring), lalu pasar cakupan M3 dan throughput berkelompok M4.",
      },
    ],
  },
  "m3-expert-coverage-market": {
    title: "Phase 6 — Pasar cakupan pakar (M3)",
    dek: "Node lemah melihat (lapisan, rentang pakar) mana yang paling langka dan berimbalan tertinggi, lalu mengisinya sendiri — pasar lapisan yang terbukti, dengan butiran lebih halus.",
    blocks: [
      {
        t: "p",
        md: "Pasar shard lapisan Kvasir — peta permintaan, pendaftaran mandiri imbalan-maksimum, unduhan parsial, imbalan per node — sudah terverifikasi di perangkat. M3 memparametrikan ulang mekanisme yang sama pada butiran **(lapisan, rentang pakar)**, sehingga cakupan memulihkan diri menuju rentang pakar yang paling kurang-replika dan berimbalan tertinggi.",
      },
      { t: "h2", kick: "Terverifikasi · API", text: "Agregasi kelangkaan → penetapan rentang berimbalan maksimum" },
      {
        t: "p",
        md: "Tiga worker mendaftar di lapisan 0: A = [0,128), B = [128,256), C = [0,128) sebagai replika kedua, dengan `target_replicas = 2`:",
      },
      {
        t: "table",
        head: ["lapisan", "pakar", "replika", "kelangkaan"],
        rows: [
          ["0", "[0, 128)", "2", "0.0 (target tercapai)"],
          ["0", "[128, 256)", "1", "0.5 (di bawah target)"],
        ],
      },
      { t: "img", src: "/blog/m3-expert-coverage-market.jpg", alt: "A market board of expert-range tiles with scarcity heat and volunteering devices" },
      {
        t: "code",
        caption: "volunteer(max_experts=64) → memangkas rentang terlangka sesuai anggaran node.",
        code: `POST /api/expert-volunteer {"max_experts": 64}
  → {layer: 0, experts: [128, 192], scarcity: 0.5, replicas: 1, target: 2}`,
      },
      { t: "h2", kick: "Selesai · Python murni (hub)", text: "Pasar penawaran/permintaan pada butiran pakar" },
      {
        t: "ul",
        items: [
          "`POST /api/expert-coverage` — worker mengirim heartbeat kepemilikan (lapisan, rentang pakar) mereka.",
          "`GET /api/expert-demand` — agregasi replika per pakar → segmen rentang pakar bersambung dengan skor kelangkaan.",
          "`POST /api/expert-volunteer` — menetapkan rentang terlangka yang dipangkas sesuai anggaran node.",
          "Pasar lapisan yang ada (daftar mandiri · unduhan parsial · imbalan) diparametrikan ulang ke (lapisan, rentang pakar).",
        ],
      },
      {
        t: "p",
        md: "Yang menyusul di M4: dispatch berkelompok untuk permintaan bersamaan plus cache hot-expert — tokens/s sebanding jumlah worker — dan perutean replika (worker terdekat/tercepat) dengan fallback churn.",
      },
    ],
  },
  "m4-batched-dispatch-throughput": {
    title: "Phase 7 — Throughput dispatch berkelompok (M4)",
    dek: "Swarm adalah jalinan throughput, bukan permainan latensi: mengelompokkan panggilan dispatch mengamortisasi overhead per permintaan 77× per token.",
    blocks: [
      { t: "h2", kick: "Terukur · ROCm, FFN pakar, n_used = 8", text: "Batch lebih besar, tok/s per worker lebih tinggi" },
      {
        t: "table",
        head: ["batch", "tok/s per worker"],
        rows: [
          ["1", "688"],
          ["16", "4,255"],
          ["64", "8,130"],
          ["256", "30,666"],
          ["512", "53,067"],
        ],
      },
      { t: "img", src: "/blog/m4-batched-dispatch-throughput.jpg", alt: "A conveyor packing tokens into growing batch crates feeding one GPU, output meter rocketing" },
      {
        t: "p",
        md: "Dari **1.45 ms/tok** pada batch 1 ke **0.019 ms/tok** pada batch 512 — perbaikan 77× per token. Waktu per panggilan nyaris tak bergerak (1.45 → 9.6 ms) sementara batch tumbuh 512× — GPU memproses batch nyaris gratis di balik overhead tetap. Inilah **sifat jalinan-throughput** yang membuat expert-parallel praktis: dispatch berkelompok mengamortisasi RTT dan overhead per permintaan.",
      },
      { t: "h2", kick: "Selesai", text: "Throughput dispatch berkelompok" },
      {
        t: "ul",
        items: [
          "Worker `--bench`: pewaktuan compute_dispatch untuk batch 1…512 → tok/s.",
          "**53k tok/s per worker** pada batch 512 (ROCm) — pengelompokan mengamortisasi overhead.",
          "Di atasnya bertumpuk cache hot-expert dan penskalaan agregat multi-worker (perutean replika).",
        ],
      },
      {
        t: "callout",
        md: "Dengan M4, **seluruh pipeline M0 → M4 terdemonstrasi pada 122B nyata**: offload backbone · irisan pakar · worker terverifikasi · dispatch dekode langsung (argmax MATCH) · proses terdistribusi · pasar cakupan · throughput berkelompok.",
      },
    ],
  },
  "phone-joins-122b-inference": {
    title: "Sebuah ponsel bergabung ke inferensi 122B",
    dek: "Sebuah Galaxy S25 mengunduh mandiri irisan pakarnya dari hub dan menghitung pakar satu lapisan di setiap langkah dekode 122B langsung. Keluarannya benar.",
    blocks: [
      {
        t: "callout",
        md: "prompt: **\"The capital of France is\"** → dihasilkan (dengan ponsel dalam lingkar): **\" Paris.\"** — 8/8 token identik dengan run lokal.",
      },
      { t: "h2", kick: "Terukur · 122B nyata, ponsel menghitung layer-0", text: "Kebenaran + TPS" },
      {
        t: "stats",
        items: [
          { n: "8/8", l: "token identik dengan lokal" },
          { n: "4.01", l: "TPS lokal (garis dasar)" },
          { n: "3.13", l: "TPS dengan ponsel" },
          { n: "1.58 GB", l: "unduhan mandiri" },
        ],
      },
      { t: "img", src: "/blog/phone-joins-122b-inference.jpg", alt: "A phone docked to a towering 122B model, printing tokens that spell Paris" },
      {
        t: "p",
        md: "Bahkan dengan ponsel menghitung pakar layer-0 untuk setiap token, **token yang dihasilkan persis sama dengan lokal** — jawaban benar \"Paris.\". TPS turun dari 4.01 ke 3.13 — perjalanan pulang-pergi dispatch ponsel (MI250 → terowongan → ponsel, ~100 ms/token) memakan 22%. Throughput kembali dengan pengelompokan dan replika (M4).",
      },
      { t: "h2", kick: "Alur partisipasi otonom", text: "Temukan → unduhan berbasis imbalan → ikut menghitung" },
      {
        t: "code",
        code: `1. Phone knows the hub (hub.kvasir-ai.net) — already holds its wallet node-token
2. GET /api/proxy/models/…/expert-shard?layers=0:1&experts=0:256
   # partially downloads its own expert slice (1.58 GB, WiFi)
3. linkcpp-expert-worker --serve
   # loads the slice (Adreno device, CPU backend) + waits for dispatch
4. MI250 backbone decodes the 122B → every token, layer-0 experts
   dispatch to the phone → experts return → combine → "Paris."
   (8/8 identical to local)`,
      },
      { t: "h2", kick: "Terverifikasi vs tersisa", text: "Mekanismenya lengkap; loop dalam-aplikasi adalah produktisasi" },
      {
        t: "ul",
        items: [
          "Unduhan parsial (endpoint expert-shard), penyajian worker, dispatch backbone, generasi 122B langsung, dan TPS — semua terverifikasi di perangkat nyata.",
          "Kebenaran: dengan ponsel berpartisipasi, 8/8 token sama dengan run lokal, dengan jawaban yang benar.",
          "Tersisa: loop otonom dalam-aplikasi (poll expert-demand → volunteer → unduh → serve → daftar) adalah pengkabelan Kotlin — demo ini menggerakkan mekanismenya langsung.",
          "Transport: demo ini memakai terowongan SSH; produksi memakai relay 443 (sudah terverifikasi di pekerjaan ring).",
        ],
      },
    ],
  },
  "kvasir-economy-virtuous-cycle": {
    title: "Ekonomi Kvasir: Siklus Bajik antara Biaya dan Imbalan",
    dek: "Jaringan inferensi terdesentralisasi hanya berhasil jika harga yang dibayar konsumen dan imbalan yang diperoleh node saling menguatkan. Inilah roda gila yang sedang kami bangun, spiral yang mematikannya, dan tiga invarian yang menjaganya terus berputar.",
    blocks: [
      {
        t: "callout",
        md: "**Tesis:** Kvasir adalah pasar dua sisi yang diselesaikan dalam satu token — konsumen membayar KVR untuk berinferensi, node memperoleh KVR untuk melayani. Seluruh desainnya berhasil atau gagal pada satu sifat: kedua sisi itu harus membentuk **siklus bajik**, di mana tiap putaran membuat putaran berikutnya lebih mudah. Salah menanganinya dan kebijakan harga apa pun akhirnya runtuh; benar menanganinya dan jaringan tumbuh *lebih murah* seiring ia tumbuh *lebih besar*.",
      },
      {
        t: "p",
        md: "Menggoda untuk memperlakukan biaya dan imbalan sebagai tarik tambang — tiap dolar yang dihemat konsumen adalah dolar yang tak diperoleh node. Kerangka itu adalah jebakan. Di jaringan yang sehat, keduanya adalah **roda gila yang sama** dilihat dari dua ujung: pembayaran menjadi imbalan, imbalan menjadi pasokan, pasokan menjadi kapasitas dan harga lebih rendah, harga lebih rendah menjadi lebih banyak pemakaian, dan lebih banyak pemakaian menjadi lebih banyak pembayaran. Pertanyaannya bukan bagaimana membagi kue yang tetap; melainkan bagaimana menjaga roda tetap berputar agar kuenya membesar.",
      },
      { t: "img", src: "/blog/kvasir-economy-virtuous-cycle.jpg", alt: "A flywheel where usage, token demand, rewards and supply each drive the next" },
      { t: "h2", kick: "Roda gila", text: "Mengapa pemakaian dan pasokan tumbuh bersama" },
      {
        t: "p",
        md: "Mesin siklusnya adalah satu aturan yang sudah berlaku di Kvasir: **inferensi harus dibayar dalam KVR**. Itu mengikat token pada pemakaian nyata — utilitas, bukan spekulasi. Pemakaian mendanai KVR yang diperoleh node; imbalan yang adil menarik masuk pasokan; pasokan memperluas kapasitas dan, lewat persaingan dan sharding pakar yang lebih halus, menurunkan biaya marjinal melayani; layanan yang lebih murah, lebih cepat, lebih cakap menarik lebih banyak pemakaian. Kvasir mengetatkan lingkar itu dengan sifat yang tak bisa ditiru API terpusat mana pun: seorang peserta bisa menjadi **konsumen dan pemasok sekaligus**. Sisi permintaan dan sisi pasokan kerap tumbuh di dalam *orang yang sama*, yang meredam ketimpangan yang merusak pasar satu sisi.",
      },
      { t: "h2", kick: "Mode kegagalan", text: "Empat spiral yang memutar roda mundur" },
      {
        t: "p",
        md: "Roda gila bisa melambat semudah ia berputar naik. Menamai spiral kematian adalah cara Anda merancang untuk menghadangnya:",
      },
      {
        t: "table",
        head: ["Spiral", "Bagaimana ia mulai", "Di mana ia berakhir"],
        rows: [
          ["Dilusi imbalan", "Lebih banyak node memperebutkan permintaan yang datar", "Imbalan per node turun, node pergi, kapasitas anjlok"],
          ["Harga terlalu rendah", "Harga murah, imbalan di bawah biaya node", "Melayani berhenti menguntungkan, pasokan dan kualitas runtuh"],
          ["Harga terlalu tinggi", "Imbalan bagus, tapi di atas pasar", "Pengguna memilih API lebih murah, pendapatan mengering"],
          ["Ketergantungan emisi", "Imbalan dibayar dengan pencetakan, bukan pendapatan", "Inflasi menggerus KVR hingga kedua sisi menyerah"],
        ],
      },
      { t: "h2", kick: "Invarian", text: "Tiga aturan yang menjaga siklus tetap bajik" },
      {
        t: "ul",
        items: [
          "**Imbalan didanai pendapatan nyata.** Pada kondisi mapan, yang diperoleh node berasal dari yang dibayar konsumen — bukan dari emisi token tanpa batas. Emisi adalah subsidi perintis yang harus *meruncing* seiring pendapatan biaya tumbuh. Kvasir sudah membantu di sini dengan mengganjar **kerja nyata** — KVR per token yang benar-benar dilayani × porsi lapisan, bukan sekadar kehadiran — sehingga subsidi tak bisa bocor ke node 'tentara bayaran' yang menganggur.",
          "**KVR adalah medium wajib.** Karena Anda tak bisa berinferensi tanpa membayar KVR, peran token terikat pada pemakaian nyata alih-alih spekulasi — ia adalah satuan hitung untuk inferensi, bukan investasi.",
          "**Harga mengambang di dalam pita.** Lantai yang dijaga di atas biaya marjinal node menjaga melayani tetap sepadan; langit-langit yang dijaga di bawah alternatif terpusat menjaga Kvasir tetap kompetitif. Di antara keduanya, harga bergerak — dan di situlah pertumbuhan jaringan akhirnya tampak sebagai biaya lebih rendah.",
        ],
      },
      { t: "h2", kick: "Termostat", text: "Membuat \"lebih banyak node → lebih murah\" benar dalam kode" },
      {
        t: "p",
        md: "Saat ini harga adalah konstanta yang diatur — masuk akal untuk devnet, tapi itu berarti menambah node menaikkan *kapasitas*, bukan keterjangkauan. Arah desainnya adalah **harga yang digerakkan utilisasi**: pasokan menganggur mendorong harga turun ke arah lantai, kemacetan mendorongnya naik ke arah langit-langit. Satu sinyal itu mengubah intuisi *\"makin banyak orang berbagi komputasi, makin murah jadinya\"* menjadi aturan yang ditegakkan protokol — sembari lantai menjaga operator tetap solven agar pasokan yang membuatnya murah tak menguap. Karena harga adalah parameter ekonomi yang sensitif, ia berubah hanya di bawah **otoritas dompet genesis dengan tanda tangan dompet + 2FA**, tak pernah oleh variabel lingkungan yang tersasar.",
      },
      {
        t: "callout",
        md: "**\"Gratis\" adalah nettonya, bukan harganya.** Anda membayar untuk apa yang Anda inferensikan dan memperoleh untuk apa yang Anda layani; berkontribusilah kira-kira sebanyak yang Anda konsumsi, maka tagihan Anda menjadi nol bersih. Tak ada API langganan — Claude Max, satu kursi Codex — yang bisa menawarkan itu, karena Anda tak pernah bisa menjadi sisi pasokan mereka. Dengan Kvasir Anda bisa menjalankan model yang tak muat di mesin Anda sendiri *dan* dibayar karena membantu orang lain menjalankan model mereka.",
      },
      {
        t: "p",
        md: "Tak satu pun dari ini menuntut desain mekanisme yang eksotis. Ia menuntut disiplin atas tiga hal: imbalan dari pendapatan, utilitas dari pemakaian, keseimbangan dari harga mengambang yang terbatas. Kvasir sudah mengirim bagian yang sulit dan jujur — imbalan yang dibayarkan ke dompet milik tiap node, imbalan sebanding kerja, token yang benar-benar harus Anda belanjakan untuk memakai jaringan. Sisanya adalah peta jalan ekonomi: peruncingan, pembagian biaya yang mendanai kumpulan asuransi untuk inferensi gagal, dan termostat. Dibangun dalam urutan itu, biaya dan imbalan berhenti bertikai dan mulai saling menggandakan.",
      },
    ],
  },
  "remote-gpu-joins-122b": {
    title: "Sebuah GPU lintas internet bergabung ke inferensi 122B",
    dek: "Sebuah workstation Blackwell di kota lain menelepon keluar satu koneksi 443 dan menghitung pakar untuk dekode 122B langsung — byte-identik dengan run lokal, dan dibayar dalam KVR atas kerja yang dilakukannya.",
    blocks: [
      {
        t: "callout",
        md: "**Yang terjadi:** sebuah dekode 122B yang berjalan pada backbone AMD di satu tempat mengirim kerja pakar per-token-nya ke mesin NVIDIA GB10 (Grace Blackwell) di kota lain — lewat satu WebSocket keluar di port 443 — dan menerima kembali keluaran pakar yang menghasilkan **token yang persis sama** dengan menghitung secara lokal. Tanpa terowongan, tanpa port-forwarding, tanpa lubang firewall masuk. Mesin jarak jauh itu memperoleh KVR atas byte yang dilayaninya.",
      },
      {
        t: "p",
        md: "Premis Kvasir adalah *perangkat keras apa pun yang datang* — termasuk perangkat keras di balik NAT operator, di internet publik, di kota berbeda. Qwen3.5-122B-A10B membawa **86% bobotnya dalam 12,544 pakar independen** (49 lapisan × 256, top-8), masing-masing fungsi murni 5.3 MB. Butiran itulah yang membuat mesin jauh yang tak berkaitan bisa memegang satu irisan dan berkontribusi. Pertanyaan terbuka tak pernah *bisakah kita membaginya* — melainkan *bisakah sebuah worker lintas internet terbuka benar-benar ikut dalam dekode langsung, dengan benar dan dapat dipertanggungjawabkan*. Kini sudah bisa.",
      },
      { t: "img", src: "/blog/remote-gpu-joins-122b.jpg", alt: "A GPU in one city dialing a single outbound line into a decode running elsewhere" },
      { t: "h2", kick: "Satu panggilan keluar", text: "Tanpa terowongan, tanpa port masuk" },
      {
        t: "p",
        md: "Worker jarak jauh membuka **satu** koneksi — `wss://` keluar ke gateway publik di 443, satu-satunya port yang diloloskan andal oleh NAT operator dan edge CDN. Gateway tak mengurai stream itu; ia **menyambung mentah (raw-splice)** WebSocket ke hub yang hanya-LAN, yang menjembataninya ke listener expert-dispatch backbone. Kedua ujung menelepon keluar dan bertemu di tengah. Worker mengekspos nol port masuk dan tak butuh alamat publik.",
      },
      {
        t: "code",
        caption: "Dua panggilan keluar, disambung menjadi satu stream dispatch biasa.",
        code: `remote worker ──outbound 443──▶ wss://gate.kvasir-ai.net  ◀──── backbone (LAN)
   (GB10, another city)          raw WS splice → hub → dispatch listener
per token:  backbone → (cur rows, expert ids) → worker → expert partials → backbone`,
      },
      { t: "h2", kick: "Byte-identik lintas internet", text: "Router memutuskan sekali; matematika mengelompok ulang persis" },
      {
        t: "p",
        md: "Backbone menjalankan router **sekali** dan dengan otoritas; worker adalah fungsi murni `(hidden, ids) → out`. Maka memindahkan fungsi itu melintasi benua mengubah *di mana* perkalian terjadi, bukan *apa* yang dihitungnya. Pada dekode 122B langsung dengan pakar layer-0 dilayani dari jarak jauh: stream token greedy **8/8 identik** (\" Paris.\"), **cosine logit 0.99773**, argmax cocok. Ini adalah sifat otoritas-router yang sama yang menjaga keputusan diskret CUDA↔ROCm↔CPU tetap invarian — backend heterogen tetap terbatasi galat kontinu, tak pernah cabang katastrofik.",
      },
      {
        t: "stats",
        items: [
          { n: "8/8", l: "token greedy identik" },
          { n: "0.99773", l: "cosine logit, jarak jauh vs lokal" },
          { n: "1.2%", l: "overhead TPS, langsung (13 ms RTT)" },
          { n: "0.00895", l: "KVR ke worker, sesi jarak jauh pertama" },
        ],
      },
      { t: "h2", kick: "Biaya jujurnya adalah RTT", text: "Mengapa swarm adalah jalinan throughput, bukan decoder latensi-rendah" },
      {
        t: "p",
        md: "Dispatch per-token serial membayar satu perjalanan pulang-pergi per langkah. Terukur: dengan tautan langsung (13 ms RTT) overhead throughput-nya **1.2%** (4.220 → 4.169 tok/s); dirutekan lewat edge CDN di 443 menjadi **~28%**. Kami memublikasikannya dengan jujur, karena ia menunjuk pada kebenaran desain — swarm WAN itu **terikat-RTT**, sehingga kekuatannya bukan latensi satu stream melainkan **kapasitas agregat**. Pengelompokan mengamortisasi perjalanan pulang-pergi: dispatch pakar berkelompok mencapai **77× throughput per-token** pada batch 512. Byte adalah ruang lega; perjalanan pulang-pergi adalah yang harus disembunyikan — yang menjadi topik tulisan peta jalan pendamping.",
      },
      { t: "h2", kick: "Dibayar tepat atas kerjanya", text: "Byte terukur menjadi KVR" },
      {
        t: "p",
        md: "Partisipasi tak berarti jika tak dapat dipertanggungjawabkan. Relay **mengukur byte yang dijembatani per sesi** ke dalam buku besar kontribusi hub; gateway mem-poll buku besar itu dan meng-kredit-delta KVR ke dompet **milik** worker sendiri. Sesi lintas-internet pertama benar-benar terakumulasi: **1.28 MB kerja → 1.277952 unit → 0.00895 KVR** dalam imbalan tertunda. Kecil, dan itulah maksudnya — ini penyelesaian nyata per-kerja, bukan piala partisipasi.",
      },
      {
        t: "p",
        md: "Jalur keluar-443 yang sama persis adalah cara sebuah **ponsel** bergabung: sebuah Galaxy S25 sudah menghitung pakar 122B melaluinya (8/8 token identik, cosine 0.99992). Sebuah model berskala frontier, dilayani oleh backbone di satu tempat, GPU datacenter di kota lain, dan ponsel di saku seseorang — semuanya menghasilkan token yang sama, masing-masing dibayar atas porsinya. Yang berikutnya adalah membuat perjalanan pulang-pergi WAN menjadi murah; peta jalan itu berpijak pada angka produksi pihak lain dan pengukuran kami sendiri.",
      },
    ],
  },
  "wan-dispatch-comm-roadmap": {
    title: "Membuat Dispatch WAN Murah: Peta Jalan yang Berpijak",
    dek: "Dispatch pakar jarak jauh berfungsi dan byte-identik — tapi dekode WAN terikat perjalanan pulang-pergi. Inilah rencana memangkas biayanya, berpijak pada angka produksi dari DeepSeek, Petals, dan lainnya (peta jalan, belum dikirim).",
    blocks: [
      {
        t: "callout",
        md: "**Bingkai:** angka yang *kami ukur* dinyatakan sebagai terukur; segala yang digambarkan sebagai rencana adalah **peta jalan**, bukan hasil yang telah dikirim. Tujuannya adalah mengambil dispatch pakar jarak jauh — yang sudah benar dan dibayar (lihat tulisan pendamping) — dan membuat perjalanan pulang-pergi WAN cukup murah sehingga GPU jauh atau ponsel menjadi anggota swarm kelas satu, bukan yang lambat.",
      },
      {
        t: "p",
        md: "Pengukuran kami sendiri, dipublikasikan terang-terangan: dispatch memakan sekitar **110 KB per token per lapisan** — 12.3 KB keluar (dispatch) plus 98.3 KB kembali (combine). Asimetri 8× itu karena tiap pakar terpilih mengembalikan keluaran penuhnya *sebelum* jumlah berbobot. Terhubung langsung, itu overhead throughput **1.2%**; lewat relay CDN, **~28%**. Itulah faktanya. Sisa tulisan ini adalah bagaimana kami hendak menutup celahnya — dan mengapa byte adalah bagian yang mudah.",
      },
      { t: "img", src: "/blog/wan-dispatch-comm-roadmap.jpg", alt: "A round trip being folded, batched and overlapped to hide latency" },
      { t: "h2", kick: "Hukum yang mendominasi", text: "Dekode WAN terikat perjalanan pulang-pergi" },
      {
        t: "p",
        md: "Hasil terpublikasi paling penting di sini bukan milik kami — melainkan Petals: saat RTT bergerak dari <5 ms ke 100 ms, dekode turun dari **1.24 ke 0.57 langkah/dtk**, sementara **pemangkasan bandwidth 10× mengubahnya ~0**. Latensi mendominasi; bandwidth adalah kelonggaran. Itu membingkai ulang seluruh masalah: memangkas byte adalah ruang lega, tapi **memangkas perjalanan pulang-pergi adalah substansinya**. Tiap butir di bawah diperingkat menurut seberapa banyak biaya perjalanan pulang-pergi yang dihapusnya.",
      },
      { t: "h2", kick: "Lebih murah di kabel", text: "Pengurangan byte yang mengutamakan akurasi" },
      {
        t: "ul",
        items: [
          "**Kembalikan jumlah parsial berbobot, bukan keluaran pakar mentah.** Berkat linearitas, combine backbone tetap eksak apa pun caranya, tapi worker mengembalikan satu vektor terjumlah alih-alih 8 — itulah pengurangan combine ~8×, dan persis itulah yang dilakukan DeepSeek-V3 / DeepEP di produksi.",
          "**Bintang paralel, bukan rantai serial** melintasi banyak worker: ΣRTT runtuh menjadi RTT maksimum.",
          "**F16 di kabel** — kami sudah menerima cosine lintas-backend ~0.998, jadi transport F16 berada di dalam toleransi yang ada; **INT8/FP8 blockwise menyusul**, setelah gerbang argmax/cosine kami sendiri meloloskannya terhadap bobot Q4_K_M (Petals menunjukkan INT8 melintasi internet nyata tanpa rugi kualitas).",
          "Bersama-sama ini menyasar **~110 KB → 9–12 KB per token (~12×)** — nyata, tapi ingat ini *ruang lega*, bukan leher botol.",
        ],
      },
      { t: "h2", kick: "Mengamortisasi perjalanan pulang-pergi", text: "Substansinya: lebih sedikit perjalanan, perjalanan tersembunyi" },
      {
        t: "ul",
        items: [
          "**Speculative decoding** mengubah banyak token menjadi satu perjalanan pulang-pergi. Pada WAN 80 ms terukur, titik impasnya hanya **~1.15–1.2 token diterima/langkah** — sehingga bahkan tebakan n-gram lemah pun menang (Jacobi polos bisa menjadi bumerang; pilihan teknik penting). Protokol dispatch kami sudah membawa `n_tokens > 1`, jadi tak perlu perubahan kabel.",
          "**Continuous batching di gateway** melipat permintaan bersamaan menjadi satu perjalanan; **prefix caching afinitas-slot** menjaga sesi tetap pada replika yang sama.",
          "**Penyembunyian latensi**: pakar bersama adalah suku aditif independen, sehingga backbone menghitungnya *secara lokal* selama perjalanan pulang-pergi jarak jauh (ScMoE melaporkan 1.82× di atas PCIe, tanpa pelatihan ulang). Jaga **hot expert tetap lokal**, kirim hanya yang dingin ke jarak jauh (EPLB mereplikasi ~32 terpanas demi percepatan dekode 2.54× di produksi).",
        ],
      },
      { t: "h2", kick: "Kebijakan & masa depan pipa gemuk", text: "Rutekan menurut peer, dan apa yang diubah 200 Gb/s" },
      {
        t: "p",
        md: "Kebijakan jalur: peer yang dapat dirutekan publik mengambil jalur **langsung** (rute 1.2% itu); relay hanya untuk perangkat terikat-NAT. Dan ketika tautan lebar 200 Gb/s tiba, 110 KB terserialisasi dalam **~4.4 µs** — suku bandwidth lenyap bahkan sebelum pengurangan di atas, dan irisan 794 MB terkirim dalam ~32 ms. Tapi **RTT adalah fisika; ia tak menyusut** — jadi speculative decoding dan overlap tetap menjadi tuas sesungguhnya bahkan di 200 G. Yang benar-benar penting bagi pipa gemuk adalah federasi multi-backbone (beberapa backbone berbagi satu kumpulan pakar) dan kerja terikat-bandwidth: prefill prompt-panjang dan throughput batch-besar.",
      },
      {
        t: "callout",
        md: "**Satu peringatan, dinyatakan dengan jujur:** lapisan transport itu sendiri (WebSocket vs QUIC, overhead masking, NAT hole-punching) **tak punya hasil eksternal yang bisa kami kutip** — itu rekayasa yang akan kami ukur sendiri sebelum mengklaim apa pun. Segala di atas berpijak pada angka produksi terpublikasi (DeepEP / DeepSeek-V3, Petals, DeepSpeed-MoE, ScMoE, SGLang/EPLB) plus pengukuran kami sendiri; ketika sebuah butir peta jalan dikirim, angka dan kala waktunya diperbarui di sini.",
      },
      { t: "h2", kick: "Apa selanjutnya", text: "Target onboarding" },
      {
        t: "p",
        md: "Hook dispatch kami bertumpu pada `build_moe_ffn` — **satu fungsi yang dibagi 43 arsitektur MoE** di mesin inferensi. Tiga invarian bersifat tak-bergantung-model: matematika MoE (routed = Σ wᵢ·Eᵢ(x), linear), jalur kode bersama, dan tensor pakar bertumpuk standar GGUF (`ne[2]` terluar → pengirisan selaras-blok). Maka onboarding model baru bukanlah desain ulang — melainkan satu lintasan melalui gerbang verifikasi argmax/cosine per-model.",
      },
      {
        t: "table",
        head: ["model", "pakar · perutean", "per-pakar (Q4≈)", "bersama", "status"],
        rows: [
          ["Step-3.7-Flash 428B (dilayani hari ini)", "288 · top-8", "terukur di armada", "ya", "dilayani — 16 stage, dua mesin"],
          ["Qwen3.5-122B", "256 · top-8", "5.3 MB (terukur)", "ya", "dilayani ujung-ke-ujung di 3 mesin"],
          ["GLM-5.2 744B", "—", "—", "ya", "terverifikasi di linkcpp — laporan belum dipublikasikan"],
          ["GLM-4.5 / 4.6 355B", "160 · top-8", "~13 MB", "ya", "direncanakan (hook terverifikasi)"],
          ["MiniMax-M2 230B", "256 · top-8", "~8 MB", "tidak", "direncanakan (hook terverifikasi)"],
          ["DeepSeek-V3 / R1 671B", "256 · top-8", "~25 MB", "ya", "direncanakan (graf deepseek2)"],
          ["Kimi K3 2.8T", "—", "—", "ya", "gerbang berikutnya — verifikasi di p4"],
          ["Qwen3-235B", "128 · top-8", "~11 MB", "tidak", "siap"],
          ["gpt-oss-120b", "128 · top-4", "~14 MB", "tidak", "siap"],
          ["Llama 4 Maverick 400B", "128 · top-1", "~70 MB", "ya", "direncanakan (MoE tiap lapisan berselang)"],
          ["MiniMax M3 428B", "128 · top-4", "TBD (GGUF)", "ya", "menunggu mesin upstream"],
          ["Mixtral 8×22B", "8 · top-2", "~170 MB", "tidak", "berfungsi — hanya worker GPU"],
        ],
      },
      {
        t: "p",
        md: "Industri sedang berkonvergensi ke MoE berbutir-halus — pakar lebih kecil, lebih banyak, sparsitas lebih tinggi (DeepSeek, Qwen, Kimi, GLM, gpt-oss semuanya bergerak ke arah ini). Tiap langkah ke arah itu membuat unit partisipasi swarm lebih kecil dan butiran pasar kelangkaan lebih halus. Model-model di atas bukan daftar keinginan; masing-masing sudah mengalir lewat hook dispatch yang sama yang kami jalankan di armada uji kami — onboarding adalah gerbang verifikasi, bukan proyek rekayasa.",
      },
    ],
  },
  "what-200g-buys-a-swarm": {
    title: "Pertanyaan 200G",
    dek: "Hub swarm kami sudah bisa bertaut pada 200 Gb/s dengan komponen yang tersedia di rak — satu tertanam di dalam GB10. Inilah yang dibeli sebuah pipa gemuk bagi MoE terdistribusi, dan satu hal yang tak bisa dibelinya.",
    blocks: [
      {
        t: "callout",
        md: "**Premisnya:** dekode WAN terikat-RTT, bukan terikat-bandwidth — peta jalan komunikasi kami menunjukkan byte adalah bagian yang mudah. Jadi apa yang sebenarnya berubah ketika hub memperoleh tautan 200 Gb/s? Hampir segalanya soal *kapasitas*, dan hampir tak ada soal *latensi*.",
      },
      { t: "img", src: "/blog/what-200g-buys-a-swarm.jpg", alt: "Two hubs joined by a fat 200G pipe beside a phone on a thin relay line" },
      { t: "h2", kick: "Sudah di dalam kotak · ConnectX-7", text: "Perangkat kerasnya bukan futuristik — satu terpasang di dalam worker GB10 kami" },
      {
        t: "p",
        md: "GB10 Grace Blackwell yang menghitung pakar 122B kami membawa **NVIDIA ConnectX-7 dengan dua port QSFP 200 GbE** di papannya. Dua mesin ini tersambung langsung dengan satu kabel QSFP56 DAC ~$100 — klaster dua-hub 200G tanpa switch. ARM adalah warga kelas satu di sini: tumpukan driver `mlx5` yang sama yang menjalankan NIC ini di datacenter x86 menjalankannya di aarch64, yang persis adalah GB10.",
      },
      {
        t: "callout",
        md: "**Cetakan halusnya:** GB10 menyuplai ConnectX-7-nya lewat dua tautan PCIe Gen5 x4 dalam mode multi-host. Kecepatan penuh terukur (~185–190 Gb/s) memerlukan **RoCE (RDMA) dan topologi yang dipetakan benar** — TCP naif di atas jalur yang salah-petakan mendarat di ~95 Gb/s atau lebih buruk. Pipa gemuk dibeli dengan konfigurasi, bukan sekadar kabel.",
      },
      { t: "h2", kick: "Tangga jarak", text: "200G adalah item katalog di tiap jangkauan" },
      {
        t: "table",
        head: ["jangkauan", "komponen", "faktor bentuk"],
        rows: [
          ["rak (0.5–3 m)", "QSFP56 DAC tembaga", "kabel, ~$100"],
          ["ruangan (~30 m)", "AOC optik aktif", "kabel"],
          ["kampus (2–10 km)", "optik 200G FR4 / LR4", "modul QSFP56"],
          ["metro (~40 km)", "optik 200G ER4", "modul QSFP56"],
          ["region (~120 km)", "400G ZR+ koheren, dijalankan pada laju saluran 200G", "modul QSFP-DD"],
          ["jarak-jauh (ratusan km)", "panjang gelombang 200G operator / sistem saluran DWDM", "layanan sewa"],
        ],
      },
      {
        t: "p",
        md: "Di WAN, *kabelnya* hanyalah serat single-mode standar — kaca netral-kecepatan yang sudah membentang di tiap kota. Kecepatannya ada di optik pluggable di tiap ujung, dan **OpenZR+ menjadikan 200G-lewat-120 km sebuah modul yang Anda colokkan ke switch**, bukan proyek telekomunikasi. Di luar itu, Anda menyewa sebuah panjang gelombang.",
      },
      { t: "h2", kick: "Yang dibelinya", text: "Setiap suku bandwidth dalam swarm lenyap" },
      {
        t: "ul",
        items: [
          "Sebuah payload dispatch (~110 KB/token/lapisan hari ini, ~10 KB setelah peta jalan kabel) terserialisasi dalam **mikrodetik** — ukuran payload berhenti menjadi kendala desain sama sekali.",
          "Sebuah **irisan pakar terkirim dalam ~32 ms** (794 MB, teoretis) dan seluruh model 122B tersinkron dalam **~3 dtk** — penyeimbangan-ulang pasar-cakupan dan onboarding hub-baru menjadi nyaris-seketika.",
          "Prefill konteks-panjang — satu-satunya fase yang benar-benar berat-bandwidth — bergerak pada kecepatan kabel, sehingga waktu token-pertama pada prompt 100K-token menjadi terikat-komputasi-backbone.",
          "**Dispatch berkelompok berskala tanpa langit-langit kabel**: lalu lintas kumpulan-pakar yang diagregasi lintas banyak stream pengguna persis merupakan beban berat-bandwidth, toleran-latensi yang diserap pipa gemuk. Inilah yang membuat federasi multi-backbone — beberapa hub, masing-masing memegang KV bagi penggunanya sendiri, berbagi satu kumpulan pakar — menjadi praktis.",
        ],
      },
      { t: "h2", kick: "Yang tak bisa dibelinya", text: "Cahaya tak terburu-buru" },
      {
        t: "p",
        md: "Serat membawa cahaya pada ~5 µs/km, dan berapa pun bandwidth tak mengubahnya. Perjalanan pulang-pergi 13 ms tetap 13 ms pada 200 Gb/s. Dekode autoregresif membayar perjalanan pulang-pergi itu per lapisan tersharding, per token — itulah sebabnya **speculative decoding (k token per perjalanan pulang-pergi) dan overlap pakar-bersama (menghitung selagi dispatch dalam perjalanan) tetap esensial** bahkan di antara hub yang tertaut pipa tergemuk di pasar. Bandwidth membeli throughput; hanya disiplin perjalanan-pulang-pergi yang membeli latensi.",
      },
      {
        t: "p",
        md: "Maka arsitekturnya mengendap menjadi dua tingkat. Sebuah **tingkat hub** — backbone dan hot expert yang tertaut oleh tautan kelas-200G, tempat kapasitas praktis tak terbatas — dan sebuah **tingkat edge** — ponsel dan perangkat kecil di relay 443, memegang ekor-panjang pakar yang ditetapkan pasar kelangkaan bagi mereka. Pipa gemuk membuat tingkat pertama terasa seperti satu mesin; relay menjaga tingkat kedua tetap terbuka bagi siapa pun. Tak satu pun menggantikan yang lain: pemisahan itulah desainnya.",
      },
    ],
  },
  "the-swarm-that-grows-under-load": {
    title: "Swarm yang tumbuh di bawah beban",
    dek: "Model raksasa yang meminjam bantuan hanya saat membutuhkannya — swarm MoE kini menskalakan dirinya sendiri terhadap lalu lintas: rapat dan cepat saat sepi, lebar dan paralel saat sibuk.",
    blocks: [
      {
        t: "img",
        src: "/blog/the-swarm-that-grows-under-load.jpg",
        alt: "A coordinator GPU breathing wider as idle phones and GPUs are drawn in under load",
      },
      {
        t: "p",
        md: "Kvasir melayani model jauh lebih besar daripada yang bisa dipegang mesin tunggal mana pun — sebuah model Mixture-of-Experts 122B-parameter berjalan melintasi sebuah koordinator ditambah sebuah swarm worker: GPU di LAN, GPU melintasi tautan 200 Gb/s, bahkan ponsel yang menyambung lewat internet. Karena model MoE merutekan setiap token hanya ke segelintir pakarnya, sebagian besar bobot diam pada saat mana pun, dan pakar-pakar yang diam itu bisa hidup **di luar** node utama — pada perangkat keras apa pun yang mengajukan diri untuk memegangnya.",
      },
      {
        t: "callout",
        md: "Bagian barunya adalah swarm kini **menskalakan dirinya sendiri terhadap beban**.",
      },
      { t: "h2", kick: "Bagaimana ia berperilaku", text: "Rapat saat sepi, lebar saat sibuk" },
      {
        t: "p",
        md: "Saat lalu lintas ringan, koordinator melayani semuanya pada GPU-nya sendiri — jalur tercepat per token, tanpa lompatan jaringan. Saat permintaan mulai menumpuk dan slot inferensinya jenuh, dua hal terjadi secara otomatis:",
      },
      {
        t: "ul",
        items: [
          "**Ia melibatkan kembali worker yang sudah dimilikinya.** Koordinator mengawasi antreannya sendiri. Di bawah kejenuhan, ia terus mengalirkan pekerjaan pakar-terutekan ke worker yang **terbukti** — yang benar-benar pernah melayani sebelumnya — menukar sedikit latensi per-token demi throughput total yang jauh lebih besar. Worker yang sekadar tersambung tetapi tak pernah menghitung tak pernah dipercaya memikul beban; worker yang baru sama sekali tetap mendapat percobaan pertama yang adil.",
          "**Hub merekrut yang baru.** Hub kontrol memperhatikan kejenuhan yang sama dan menaikkan \"permintaan\" akan pakar model itu. Node yang menganggur — ponsel di saku seseorang, GPU cadangan di seberang kota — sudah menjajaki pasar permintaan itu. Begitu permintaan naik, mereka ditawari sepotong pakar untuk dilayani, mengunduhnya, menyambung, dan bergabung. Saat lonjakan berlalu, permintaan surut kembali dan worker ekstra diam-diam melepaskan diri.",
        ],
      },
      {
        t: "p",
        md: "Tak seorang pun menjadwalkan ini. Tak ada node yang didorong. Swarm bernapas seirama beban: rapat dan cepat saat sepi, lebar dan paralel saat sibuk — dan ini bekerja bahkan untuk node di balik router rumahan, karena semuanya berbasis-tarik (pull-based).",
      },
      {
        t: "p",
        md: "Itulah bentuk sebuah jaringan yang dapat melayani model triliun-parameter pada perangkat keras yang tak dimiliki satu orang pun: kapasitas menganggur diundang masuk persis ketika layak diundang, dan hanya saat itu.",
      },
    ],
  },
  "moving-the-ring-onto-p4": {
    title: "Memindahkan Ring ke p4",
    dek: "Tujuh perubahan kontrak antara sebuah mesin dan pemanggilnya. Masing-masing gagal dengan cara berbeda, dan hanya satu yang tampak seperti error.",
    blocks: [
      {
        t: "p",
        md: "Kami menggabungkan rilis baru mesin p4 dan ring berhenti melayani. Bukan dengan crash — pemuat melaporkan sukses, para agen melaporkan ready, dan tak terjadi apa-apa. Menelusuri balik dari kesunyian itu memakan satu hari dan menemukan **tujuh** tempat di mana pemanggil kami dan mesinnya sudah saling menyimpang. Yang membuatnya layak dicatat bukan jumlahnya. Melainkan bahwa enam dari tujuh tak menghasilkan error apa pun.",
      },
      { t: "h2", kick: "Kegagalan satu", text: "Peristiwa untuk node yang tak ada diteruskan, bukan ditolak" },
      {
        t: "p",
        md: "Pemuat kami mengalamatkan perintah LOAD ke node yang ingin diciptakannya. Tetapi sebuah node belum ada sampai LOAD menciptakannya, dan aturan broker untuk peristiwa yang menyebut node tak dikenal adalah **meneruskannya keluar** alih-alih menolaknya. Perintah itu meninggalkan agen sambil mencari tempat lain untuk dituju, tak menemukan apa pun, lalu dibuang. Tak ada satu baris log pun, karena dari sudut pandang broker tak ada yang salah.",
      },
      {
        t: "p",
        md: "Perbaikannya adalah mengalamatkan LOAD ke *agen*, dibungkus dalam amplop siklus-hidup mesin yang netral-backend, dengan perintah adapter sendiri sebagai isi yang buram. Jelas setelah tahu; tak terlihat dari luar.",
      },
      { t: "h2", kick: "Kegagalan dua", text: "Sebuah angka yang harus sama dengan angka lain" },
      {
        t: "p",
        md: "Sebuah model dimuat di bawah **load generation**, dan tiap node didaftarkan dengan **node generation**. Kami memperlakukan keduanya sebagai independen — timestamp untuk yang satu, `1` untuk yang lain — dan semuanya berjalan. Ring termuat. Ia menjawab satu permintaan dengan benar. Lalu node kepala mati.",
      },
      {
        t: "code",
        caption: "Pemeriksaannya, di dalam akuntansi pelepasan milik adapter.",
        code: `let Endpoint::Node { generation, .. } = &event.envelope.source;
if *generation != receipt.load_generation {
    return Err("release owner census generation differs from source");
}`,
      },
      {
        t: "p",
        md: "Tanda terima yang menutup sebuah permintaan yang selesai membawa load generation, dan node yang mengirimnya membawa miliknya sendiri. Saat keduanya berbeda, node dihentikan. Jadi bentuk bug-nya adalah: **pemuatan sukses, permintaan pertama sukses, kepala mati, dan setiap sesi sesudah itu menggantung setengah-termuat.** Itu terbaca persis seperti crash di bawah beban dan sama sekali tak seperti ketidakcocokan. Pemuat kami kini menolak rencana yang kedua angkanya tak sepakat, sebelum apa pun dimuat.",
      },
      { t: "h2", kick: "Kegagalan tiga", text: "Sebuah batas yang tak mungkin terpenuhi" },
      {
        t: "p",
        md: "Adapter membandingkan hasil terbesar yang boleh dikembalikan sebuah stage terhadap angka yang dilaporkan stage server saat ia menyala, untuk kesetaraan persis. Kami tak bisa tahu angka itu tanpa memuat modelnya — jadi kami memuat dengan tebakan, dan kegagalannya memberi tahu kami angka nyata keempat stage sekaligus:",
      },
      {
        t: "code",
        caption: "Satu run, empat jawaban.",
        code: `step37-s0: profile=33554432, READY=34419218444
step37-s1: profile=33554432, READY=34419218444
step37-s2: profile=33554432, READY=34419218444
step37-s3: profile=33554432, READY=59136012`,
      },
      {
        t: "p",
        md: "34 GB. Retained store milik agen adalah 256 MiB, jadi ring itu tak akan pernah bisa diterima. Membaca penurunan rumusnya dari stage server menunjukkan sebabnya: batas itu menskala dengan `n_batch × n_ubatch`, dan kami mewarisi lebar batch 2048 dari sebuah konfigurasi yang mendahului pemeriksaan ini. Pada 128 baris — lebar yang dipakai tata letak produksi — batasnya 138 MB dan muat. Kini kami menurunkannya di dalam rencana dari rumus yang sama alih-alih membawa konstanta yang diingat, dan angka prediksi untuk stage ekor keluar persis sama dengan yang pernah dicatat deployment lain — jenis kesepakatan yang layak dimiliki sebelum menghabiskan dua puluh menit untuk satu pemuatan.",
      },
      { t: "h2", kick: "Empat yang lain", text: "Ringkas saja" },
      {
        t: "ul",
        items: [
          "**Alamat loopback di ring dua-host.** Sebuah stage menelepon agen stage berikutnya di alamat yang diiklankan agen itu. Iklankan `127.0.0.1` dan host A menelepon dirinya sendiri. Konfigurasi ring yang tercatat ternyata loopback sejak awal — ia tak pernah bisa bekerja antar-host.",
          "**Jurnalnya wajib.** Sebuah model tak akan dimuat tanpa jurnal operasional agen. Kami mematikannya saat mengejar error lain dan memperburuk gejalanya dengan cara yang tampak seperti kemajuan.",
          "**Nama perangkat bersifat khusus-backend.** Pembangun rencana acuan menargetkan CUDA dan memancarkan `--device CUDA0`. Build HIP menamai perangkatnya `ROCm0`. Rencana itu memuat seluruh model *lalu* gagal menemukan perangkatnya.",
          "**Server native adalah bagian dari rilis.** Agen yang dibangun dari pohon sumber lebih baru menginginkan kapabilitas yang tak dilaporkan stage server yang terpasang. Juga ditemukan setelah pemuatan model penuh.",
        ],
      },
      { t: "h2", kick: "Apa yang kami ambil darinya", text: "Kesunyian adalah mode kegagalan yang mahal" },
      {
        t: "p",
        md: "Setiap satu dari semua ini murah untuk diperbaiki dan mahal untuk ditemukan, dan polanya konsisten: kegagalan yang mahal adalah yang membuat sistem tampak benar tetapi tak melakukan apa-apa, atau melakukan sesuatu sekali saja. Penjaga yang kami tambahkan semuanya berbentuk sama — menolak lebih awal, di tempat kesalahannya masih terbaca. Pemuat menuliskan load generation ke disk *sebelum* perintah pertama berangkat, karena tanpa itu ia tak terpulihkan. Ia menolak ketidakcocokan generation alih-alih menemukannya setelah permintaan pertama. Ia menurunkan batas hasil alih-alih mengingatnya.",
      },
      {
        t: "callout",
        md: "**Ring sedang melayani.** Empat stage di dua mesin, 113 GiB bobot residen, token pertama dalam 1.4 dtk saat dingin dan ~0.3 dtk saat hangat, dan kontribusi per-node mengalir sampai ke buku besar penyelesaian untuk pertama kalinya.",
      },
    ],
  },
  "the-template-is-the-callers-job": {
    title: "Template Adalah Tugas Pemanggil",
    dek: "p4 meneruskan prompt buram dan tak menerapkan template chat apa pun. Lupakan itu dan model menjawab pertanyaan yang tak Anda ajukan — dengan fasih, dan sampai batas token.",
    blocks: [
      {
        t: "p",
        md: "Jawaban nyata pertama dari ring kami yang baru pulih adalah aritmetika yang benar, disusul percakapan yang tak pernah terjadi:",
      },
      {
        t: "code",
        caption: "17 × 23, ditanyakan kepada sebuah model yang dilayani.",
        code: `" 391\n\nWhat is 12 times 12? Reply with only the number. 144\n\nWhat is 14"`,
      },
      {
        t: "p",
        md: "Angkanya benar. Semua setelahnya adalah model yang melanjutkan sebuah dokumen, karena itulah yang kami serahkan kepadanya: pesan-pesan yang diratakan menjadi satu string. Model instruct membaca itu sebagai teks untuk diperpanjang, bukan giliran untuk dijawab. Ia tak pernah memancarkan token akhir-gilirannya, jadi generasi berjalan sampai batas setiap kali.",
      },
      { t: "h2", kick: "Tugas siapa", text: "Kelalaian yang disengaja, bukan lubang" },
      {
        t: "p",
        md: "p4 menyerahkan prompt buram ke stage server dan tak menerapkan format giliran apa pun dari dirinya sendiri — adapter berstage hanya membawa perkakas untuk *membaca* template dari sebuah GGUF, tak pernah untuk menerapkannya. Itu garis yang masuk akal untuk ditarik: mesinnya tetap sempit dan agnostik-model, dan pemanggil, yang sudah tahu model mana yang diajaknya bicara, merender formatnya. Tetapi garis yang ditarik dan tidak didokumentasikan adalah garis yang dilangkahi seseorang.",
      },
      {
        t: "p",
        md: "Membaca template dari berkas model menyelesaikannya: giliran ChatML, `<|im_end|>` sebagai token akhir-giliran, dan giliran asisten yang dibuka dengan blok berpikir. Setelah itu dirender oleh bridge, pertanyaan yang sama:",
      },
      {
        t: "code",
        caption: "Model yang sama, ring yang sama, format giliran diterapkan.",
        code: `finish_reason : "eos"          (was "length")
content       : "391"
reasoning     : "We need to compute 17*23. 17*20=340, plus 17*3=51, total 391."`,
      },
      { t: "h2", kick: "Bagian yang memakan biaya", text: "Sesi berpikir bisa melahap jawabannya" },
      {
        t: "p",
        md: "Model penalaran membelanjakan token sebelum ia mengatakan apa pun. Beri ia anggaran dan pertanyaan sulit, dan ia bisa menghabiskan seluruh anggaran untuk berpikir, meninggalkan jawabannya kosong — dan di jaringan tempat pemanggil **sudah membayar on-chain sebelum permintaannya dijalankan**, jawaban kosong bukan masalah kualitas. Itu tagihan untuk nol hasil.",
      },
      {
        t: "p",
        md: "Gateway penyelesaian sudah tahu ini dan meminta agar berpikir dinonaktifkan. Template model tak punya sakelar untuk itu, jadi bridge membuka *dan menutup* blok berpikirnya di dalam prompt, dan model menulis jawabannya setelah itu. Kami sempat salah sekali dengan cara yang paling jelas — menutup bloknya di dalam prompt berarti tag penutupnya tak lagi ada di keluaran, sehingga pemisahnya mengarsipkan seluruh jawaban sebagai penalaran dan mengembalikan content kosong. Yang persis merupakan kegagalan yang hendak dicegah pengaturan itu.",
      },
      {
        t: "callout",
        md: "**Di mana ini menempatkan kontraknya.** Mesin meneruskan byte. Bridge tahu modelnya: ia merender format giliran yang disebut di rencana penempatan, mengembalikan sesi berpikir sebagai `reasoning_content` terpisah dari `content`, dan menjepit permintaan yang meminta keluaran lebih banyak daripada yang dimuat ring — karena permintaan yang terlalu besar jika tidak akan ditolak mentah-mentah, dan jawaban lebih pendek lebih baik daripada error mesin.",
      },
    ],
  },
};
