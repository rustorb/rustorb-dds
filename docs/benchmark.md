# 性能測定

rustorb-dds の往復レイテンシとスループットを、同じ機械の上で Cyclone DDS と Fast DDS と比べた（rustorb-dds#100 の段 1）。discovery の速さ（段 2、#106）と DDS Security の上乗せ（#107）も測った。測り方の道具は [bench/](../bench/README.md) にある。

**ここの数値はコンテナで測った。** CPU 4 個を他と分け合い、周波数も割り込みも固定できないので、回ごとに揺れる。同じ実行の中での相対比較として読み、別の機械や別の実行の数値と並べないこと。公開用の数値を専用の機械で測るかどうかは、あとで決める（#100）。

## 測り方

- 3 つの実装で同じ手順のプログラムを使う。rustorb-dds は `rustorb-dds-perf`（[crates/rustorb-dds-perf](../crates/rustorb-dds-perf)）、Cyclone DDS 0.10.4 は [bench/cyclonedds/perf.c](../bench/cyclonedds/perf.c)、Fast DDS 2.11.2 は [bench/fastdds/perf.cpp](../bench/fastdds/perf.cpp)。どれも同じ型（[bench/idl/Perf.idl](../bench/idl/Perf.idl): 通し番号、時刻、`sequence<octet>`）、同じ QoS、同じ待ち方（reader に標本が来るまで待って take）で、互いにつなげられる。参考に Cyclone DDS の `ddsperf`（型 `KS`、listener で受ける）も測った。
- レイテンシ: ping が書き、pong が受けて書き返すまでの往復時間。32 B〜1 MiB（2 倍刻み）。各大きさで 1 秒の warm-up のあと 5 秒。reliable は KEEP_LAST 1。
- スループット: pub が全力で書き、sub が 5 秒で受けた payload。reliable は writer が KEEP_ALL で未確認応答 100 標本まで（溢れると write が待つ）、best-effort は待たない。sub は 256 標本ずつ take し（Fast DDS 版は 1 標本ずつ）、best-effort の sub は take していない標本を 1000 まで保つ（入らない分は失われる）。
- 転送: `udp`（rustorb-dds と Fast DDS は共有メモリを切る。Cyclone DDS の既定）、`shm`（rustorb-dds の共有メモリ、Fast DDS の既定の組み込み転送、Cyclone DDS の iceoryx）、`loan`（rustorb-dds の貸し出し: payload に触れない zero-copy）。実装をまたぐ組は `udp` だけ。
- 2 つのプロセスを CPU の半分ずつに固定した。各組を 3 回測り、中央値を載せる。CPU 時間と最大 RSS は両方のプロセスの `wait4` から取った。**この段の最大 RSS は正しくない。** `wait4` の ru_maxrss は Python から fork した時点の Python の RSS（約 13 MiB）を引き継ぐので、それより小さいプロセスは約 13 MiB と出ている。いまの `bench/run.py` は動いている間に /proc の VmHWM を読む（#106 で直した）。大きな値（best-effort の受け手の GB など）はそのまま正しい。

再現:

```sh
bench/run.py --interop --out bench/results/<名前>
bench/report.py bench/results/<名前> --svg docs/benchmark
```

この文書の数値は [bench/results/2026-11-26-container-2](../bench/results/2026-11-26-container-2)（Intel Xeon 2.10GHz 4 CPU のコンテナ、Linux 6.18）。レイテンシは rustorb-dds 74362f5、スループットは 3134856 で測った（PR #115 の途中と最後。違いは断片で送った標本のあとの HEARTBEAT だけ）。PR #115 の前の数値は [bench/results/2026-11-26-container](../bench/results/2026-11-26-container)（771410f）にある。

## わかったこと

レイテンシ（reliable の往復、中央値）:

![往復レイテンシ](benchmark/latency.svg)

- 小さな標本で、rustorb-dds は 79 µs、Cyclone DDS は 66 µs、Fast DDS は 89 µs（32 B、UDP）。PR #115 の前は rustorb 123 µs だった。受け取ったメッセージを受信スレッドでそのまま処理し、待っているアプリを ACKNACK より先に起こし、ACKNACK を標本ごとには求めないようにした。rustorb と Cyclone の組は 72 µs。→ #111
- rustorb-dds の共有メモリは UDP とほぼ同じ（32 B で 79 µs）。
- 貸し出し（zero-copy）は大きさによらず 93〜110 µs で、1 MiB でも UDP（3.2 ms）の 1/30。
- 16 KiB を超えると rustorb-dds の往復が Cyclone より大きく伸びる（16 KiB: rustorb 143 µs、Cyclone 85 µs。256 KiB: 0.80 ms と 0.46 ms）。→ #112
- Cyclone の ddsperf は 18 µs。listener の中で返事を書くので、待っているスレッドを起こす手順がない。上の 3 つのプログラムとは手順が違う。
- `rustorb-dds-perf --listener`（bench の `rustorb-listener`）は ddsperf と同じく listener の中で返事を書く。別の測定（`bench/run.py --peers rustorb,rustorb-listener,cyclonedds,cyclonedds-ddsperf`、32 B、reliable）で、WaitSet の 85 µs が listener では 51 µs になった（SHM も 50 µs）。同じ測定で Cyclone DDS は 66 µs、ddsperf は 17 µs。→ #111
- Fast DDS は best-effort で 256 KiB 以上（rustorb との組でも）の往復が全部失われ、reliable の 512 KiB 以上で中央値が 10 ms を超えた。Fast DDS 2.11 の UDP での大きな標本の扱いと思われる（相手は black box なので確かめていない）。

スループット（reliable）:

![スループット](benchmark/throughput.svg)

- 小さな標本で rustorb-dds は Cyclone DDS と同じくらいになった（256 B: 256 Mbit/s と 256 Mbit/s、1 KiB: 918 と 870 Mbit/s）。PR #115 の前は約 6 割（1 KiB で 495 Mbit/s）。ACKNACK を 16 標本ごとと書き止んだ 1 ms 後に求めるようにしたのが主な理由。Fast DDS（149 Mbit/s）より速い。ddsperf は 3.5 Gbit/s。→ #113
- 16 KiB〜1 MiB で rustorb 8.7〜10 Gbit/s、Cyclone 5.3〜8.7 Gbit/s。
- 貸し出しの 1 MiB が 2.3 Gbit/s に落ち、回ごとの揺れも大きい。→ #112

best-effort:

- 受け手の最大 RSS は、rustorb → rustorb で 37 MiB 以下（中央値）になった。PR #115 の前は 1.9 GB、Fast DDS → rustorb で 2.7 GB まで増えていた。増えていたのは KEEP_ALL で上限のない reader の history（QoS どおり）で、rustorb-dds の sub が送り手より遅かったため。測定ツールの sub はそれを一度に全部 take していた。→ #110（閉じた）
- 小さな標本で rustorb-dds の受け手は送り手に追いつかず、取りこぼしが多い。取りこぼし数は送り手の速さで決まるので、実装どうしの比較には向かない（表は下に載せる）。
- 貸し出しの best-effort の数値（1 MiB で 1.2 Tbit/s など）は、payload に触れずに数えているので転送の速さではない。

測っている途中で、速い writer が受け手の共有メモリの ring を溢れさせると write が「the shared memory buffer is full」で失敗する不具合が見つかり、直した（ring に入らないメッセージは UDP で失われたのと同じ扱い。docs/shm.md のとおり）。

## Discovery（#106）

N 個のプロセスを同時に起動し、それぞれが参加者 1 つ、同じトピックの writer と reader を 1 つずつ作る。各プロセスが、共通の開始時刻から数えて自分の writer と reader が N 者すべて（自分も含む）とマッチするまでの時間を出す。最後にマッチしたプロセスの時間がその回の値。N は 1, 2, 5, 10, 20, 50、各 3 回、回ごとに別のドメイン、UDP だけ（共有メモリなし）、CPU は固定しない。「rustorb+cyclonedds」は rustorb-dds と Cyclone DDS のプロセスを交互に起動した組。tcpdump でその回の UDP パケットを数えた（標本は書かないので discovery の通信: SPDP、SEDP とその HEARTBEAT・ACKNACK、liveliness。プロセスが残る 5〜20 秒全体）。メモリはプロセスごとの VmHWM の最大。

```sh
bench/discovery.py --out bench/results/<名前>
bench/discovery.py --report bench/results/<名前>
```

数値は [bench/results/2026-11-26-container-discovery](../bench/results/2026-11-26-container-discovery)（上と同じコンテナ）。

わかったこと:

- 測り始めたとき rustorb-dds は遅かった（試しの 1 回で、2 者 55 ms、10 者 1,059 ms。Cyclone DDS は 8 ms と 128 ms）。原因は 3 つで、この PR で直した。
  - 最初の SPDP 告知（と新しい参加者への返事）が、参加者のスレッドが次に起きるまで（最大 50 ms）出なかった。
  - 相手がまだこちらを知らないうちに送った SEDP の DATA は捨てられ、組み込み writer の HEARTBEAT 周期（1 s）まで直らなかった。周期を 100 ms にした（すべて確認応答されたら何も送らない）。
  - reliable reader がマッチした writer に最初に送る ACKNACK で HEARTBEAT を求める（final flag を立てない。8.4.2.3.4 の preemptive ACKNACK）。マッチ前に送られて捨てられた DATA がすぐ再送される。
- 直したあと、rustorb-dds は 50 者まで 3 つの中でいちばん早くすべてマッチした（10 者 23 ms、Cyclone DDS 42 ms、Fast DDS 203 ms。50 者 0.50 s、0.75 s、3.8 s）。
- 通信量は rustorb-dds と Cyclone DDS が同じくらい（50 者でパケット約 44,000、octet は rustorb が 3/4）、Fast DDS は約 2 倍のパケットと 4〜5 倍の octet。
- CPU 時間は 20 者以上で rustorb-dds が Cyclone DDS の約 1.6〜1.7 倍（50 者で 6.3 s と 3.8 s）。Fast DDS と同じくらい。
- 1 プロセスの最大 RSS は rustorb-dds 5〜6 MiB、Cyclone DDS 7〜9 MiB、Fast DDS 15〜17 MiB。
- rustorb-dds と Cyclone DDS を混ぜても、Cyclone DDS だけのときと同じくらいで、すべての回ですべてマッチした。

#### Until all matched, the last participant (ms, median of runs)

| participants | rustorb | cyclonedds | fastdds | rustorb+cyclonedds |
|---:|---:|---:|---:|---:|
| 1 | 3 | 7 | 9 |  |
| 2 | 6 | 9 | 81 | 8 |
| 5 | 11 | 24 | 184 | 22 |
| 10 | 23 | 42 | 203 | 43 |
| 20 | 69 | 186 | 283 | 180 |
| 50 | 501 | 747 | 3,792 | 687 |

#### Until matched, the median participant (ms)

| participants | rustorb | cyclonedds | fastdds | rustorb+cyclonedds |
|---:|---:|---:|---:|---:|
| 1 | 3 | 7 | 9 |  |
| 2 | 6 | 9 | 46 | 8 |
| 5 | 10 | 20 | 23 | 13 |
| 10 | 22 | 35 | 119 | 31 |
| 20 | 62 | 90 | 157 | 82 |
| 50 | 453 | 616 | 1,477 | 561 |

#### CPU time of all the processes over the run (s)

| participants | rustorb | cyclonedds | fastdds | rustorb+cyclonedds |
|---:|---:|---:|---:|---:|
| 1 | 0.02 | 0.01 | 0.01 |  |
| 2 | 0.03 | 0.02 | 0.03 | 0.03 |
| 5 | 0.08 | 0.05 | 0.10 | 0.08 |
| 10 | 0.18 | 0.14 | 0.26 | 0.19 |
| 20 | 0.78 | 0.49 | 0.85 | 0.74 |
| 50 | 6.32 | 3.78 | 6.09 | 5.81 |

#### Largest resident set of one process (MiB)

| participants | rustorb | cyclonedds | fastdds | rustorb+cyclonedds |
|---:|---:|---:|---:|---:|
| 1 | 5 | 7 | 15 |  |
| 2 | 5 | 7 | 15 | 7 |
| 5 | 5 | 7 | 16 | 7 |
| 10 | 5 | 7 | 16 | 7 |
| 20 | 5 | 8 | 16 | 8 |
| 50 | 6 | 9 | 17 | 9 |

#### UDP packets of the run (all of its seconds)

| participants | rustorb | cyclonedds | fastdds | rustorb+cyclonedds |
|---:|---:|---:|---:|---:|
| 1 | 2 | 2 | 14 |  |
| 2 | 35 | 17 | 83 | 35 |
| 5 | 322 | 224 | 605 | 321 |
| 10 | 1,460 | 1,091 | 2,883 | 1,480 |
| 20 | 6,395 | 5,394 | 15,248 | 6,431 |
| 50 | 44,133 | 43,751 | 89,864 | 50,096 |

#### UDP octets of the run (KiB)

| participants | rustorb | cyclonedds | fastdds | rustorb+cyclonedds |
|---:|---:|---:|---:|---:|
| 1 | 0 | 1 | 6 |  |
| 2 | 4 | 6 | 26 | 6 |
| 5 | 37 | 47 | 178 | 46 |
| 10 | 161 | 209 | 838 | 210 |
| 20 | 724 | 918 | 4,720 | 896 |
| 50 | 4,855 | 6,396 | 25,967 | 6,245 |

## DDS Security（#107）

[docs/security.md](security.md) の組み込みプラグインで、Security なし（none）、認証とアクセス制御だけ（auth: governance の保護をすべて NONE）、署名（sign: discovery、liveliness、metadata、data を SIGN、つまり AES-GMAC）、暗号化（encrypt: 同じく ENCRYPT、AES-GCM）を比べた。RTPS メッセージ全体の保護（rtps_protection_kind）はどれも NONE。証明書と governance・permissions は `interop/security/gen.sh` で作り、どの参加者も同じ identity（EC P-256）にした。Cyclone DDS と Fast DDS もそれぞれ自分の Security を有効にして測った（実装をまたぐ組は測っていない）。UDP、reliable、各 3 回の中央値。

```sh
bench/run.py --peers rustorb,cyclonedds,fastdds --security none,auth,sign,encrypt \
  --reliability reliable --sizes 32,4096,65536 --out bench/results/<名前>
bench/discovery.py --peers rustorb,cyclonedds,fastdds --mixed '' \
  --security none,auth,sign,encrypt --counts 2,5,10,20 --out bench/results/<名前>
```

数値は [bench/results/2026-11-26-container-security](../bench/results/2026-11-26-container-security)（上と同じコンテナ。rustorb-dds は PR #118 の途中、handshake の直しの後）。この回の throughput の「other」側の RSS は取れていない（`run.py` の不具合。直した）。

わかったこと:

- 小さな標本（32 B）の往復は、署名・暗号化で rustorb-dds が 77 → 95〜101 µs、Cyclone DDS が 71 → 108〜111 µs、Fast DDS が 95 → 336〜347 µs。
- 64 KiB では rustorb-dds の署名・暗号化が 645〜763 µs で、Cyclone DDS（417〜441 µs）の約 1.6 倍。スループットも 64 KiB で rustorb 2.4〜2.7 Gbit/s、Cyclone 3.7〜4.0 Gbit/s。→ #119
- 認証だけ（保護なし）でも、rustorb-dds は 4 KiB 以上で少し遅くなる（64 KiB のスループット 9.2 → 8.2 Gbit/s）。Cyclone DDS と Fast DDS は Security なしと変わらない。→ #119
- #119 で直した（[bench/results/2026-11-27-container-security](../bench/results/2026-11-27-container-security)、同じコンテナ）。遅さの大半は GHASH（GCM と GMAC の認証計算）で、RustCrypto の polyval 0.6 は 1 ブロックずつ掛けて約 1.5 GB/s。data と metadata を両方保護すると、64 KiB の 1 往復で GHASH が 8 回走る。security crate に AES-GCM/GMAC を持ち、GHASH は PCLMULQDQ で 8 ブロックの積をまとめて 1 回だけ簡約するようにした（PCLMULQDQ がなければ RustCrypto の ghash。AES と CTR は RustCrypto）。64 KiB の GMAC が 43 → 7 µs、GCM が 53 → 25 µs。あわせて、保護しないサブメッセージしかないメッセージは写さないようにした。直したあと（往復の中央値、µs）:

| size | rustorb sign | rustorb encrypt | Cyclone sign | Cyclone encrypt | Fast DDS sign | Fast DDS encrypt |
|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 90 | 92 | 100 | 92 | 210 | 201 |
| 4 KiB | 100 | 112 | 106 | 103 | 207 | 208 |
| 64 KiB | 360（前 645〜698） | 551（前 763〜840） | 452 | 450 | 490 | 542 |

  64 KiB のスループットは署名 6.5 Gbit/s、暗号化 4.5 Gbit/s（Cyclone 4.7 と 4.5）。暗号化の往復がまだ Cyclone より長いのは、AES の CTR（RustCrypto の aes と ctr、64 KiB に約 18 µs）が残るため。認証だけのときの往復は Security なしと同じになった（64 KiB 192 µs）が、64 KiB のスループットはまだ 10.4 → 9.1 Gbit/s。
- discovery（handshake を含めて、すべてマッチするまで）: 測り始めたとき、rustorb-dds は 10 者で約 0.5 s かかっていた。相手の SPDP より先に届いた handshake の要求を捨てていて、送り手が 500 ms 後に送り直すまで待っていたため。いまは、その要求を相手を見つけるまで取っておいて、見つけたらすぐ答える（参加者 256 まで、相手ごとに最後の 1 つ）。直したあとは 10 者で 84〜95 ms、20 者で 283〜384 ms。Cyclone DDS は 2 者でも約 330 ms、10 者 530〜710 ms、20 者 1.5 s。Fast DDS は 10 者で約 1.1 s、20 者の署名・暗号化では 3 回とも 20 s のうちにすべてはマッチしなかった。
- handshake の通信量は rustorb-dds と Cyclone DDS が同じくらい（20 者の暗号化で 5.6 MiB と 6.4 MiB）、Fast DDS は約 110 MiB（すべてはマッチしないまま送り直し続けた）。
- 1 プロセスの最大 RSS は、Security で rustorb-dds が 6 → 8 MiB、Cyclone DDS が 8 → 13 MiB、Fast DDS が 16 → 22〜29 MiB。

### 往復とスループット

#### Latency, reliable: round trip, median (µs)

| size | rustorb udp+none | rustorb udp+auth | rustorb udp+sign | rustorb udp+encrypt | cyclonedds udp+none | cyclonedds udp+auth | cyclonedds udp+sign | cyclonedds udp+encrypt | fastdds udp+none | fastdds udp+auth | fastdds udp+sign | fastdds udp+encrypt |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 76.5 | 82.0 | 95.1 | 101.1 | 70.7 | 73.7 | 107.8 | 110.9 | 94.8 | 97.2 | 347.3 | 336.1 |
| 4 KiB | 78.1 | 91.1 | 119.0 | 138.8 | 75.9 | 74.6 | 112.9 | 118.7 | 96.3 | 106.1 | 352.3 | 334.2 |
| 64 KiB | 184.6 | 203.6 | 645.0 | 762.9 | 151.4 | 149.8 | 416.5 | 441.4 | 130.6 | 140.9 | 656.8 | 702.8 |

In this table the runs of a cell differed by 8 % (median over the cells), 28 % at most.

#### Latency, reliable: round trip, 99th percentile (µs)

| size | rustorb udp+none | rustorb udp+auth | rustorb udp+sign | rustorb udp+encrypt | cyclonedds udp+none | cyclonedds udp+auth | cyclonedds udp+sign | cyclonedds udp+encrypt | fastdds udp+none | fastdds udp+auth | fastdds udp+sign | fastdds udp+encrypt |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 191.0 | 179.8 | 215.9 | 222.3 | 156.8 | 157.5 | 223.6 | 253.8 | 204.3 | 202.5 | 966.0 | 1,208 |
| 4 KiB | 180.8 | 222.3 | 250.4 | 303.4 | 175.5 | 173.2 | 226.8 | 235.6 | 200.7 | 198.1 | 1,127 | 1,370 |
| 64 KiB | 417.0 | 493.8 | 1,654 | 2,179 | 357.0 | 296.6 | 1,086 | 1,225 | 274.1 | 291.1 | 2,493 | 2,653 |

In this table the runs of a cell differed by 18 % (median over the cells), 68 % at most.

#### Throughput, reliable (Mbit/s of payload received)

| size | rustorb udp+none | rustorb udp+auth | rustorb udp+sign | rustorb udp+encrypt | cyclonedds udp+none | cyclonedds udp+auth | cyclonedds udp+sign | cyclonedds udp+encrypt | fastdds udp+none | fastdds udp+auth | fastdds udp+sign | fastdds udp+encrypt |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 21.4 | 20.9 | 16.2 | 16.1 | 34.0 | 35.2 | 11.8 | 11.6 | 4.6 | 4.7 | 3.8 | 3.8 |
| 4 KiB | 2,643 | 2,375 | 1,329 | 1,254 | 2,754 | 2,796 | 1,341 | 1,382 | 353.6 | 376.5 | 252.0 | 256.6 |
| 64 KiB | 9,153 | 8,161 | 2,723 | 2,378 | 11,879 | 11,685 | 3,973 | 3,710 | 3,332 | 3,198 | 1,689 | 1,867 |

In this table the runs of a cell differed by 8 % (median over the cells), 42 % at most.

### Discovery

#### Until all matched, the last participant (ms, median of runs)

| participants | rustorb | rustorb auth | rustorb sign | rustorb encrypt | cyclonedds | cyclonedds auth | cyclonedds sign | cyclonedds encrypt | fastdds | fastdds auth | fastdds sign | fastdds encrypt |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 2 | 5 | 13 | 12 | 14 | 9 | 332 | 332 | 335 | 14 | 29 | 31 | 28 |
| 5 | 11 | 29 | 33 | 27 | 23 | 448 | 601 | 646 | 187 | 290 | 291 | 295 |
| 10 | 23 | 84 | 95 | 93 | 40 | 532 | 691 | 709 | 125 | 1,053 | 1,116 | 1,088 |
| 20 | 90 | 283 | 367 | 384 | 197 | 1,478 | 1,558 | 1,472 | 241 | 1,299 | 3,989 (3 of 3 not all matched) | 2,365 (3 of 3 not all matched) |

#### CPU time of all the processes over the run (s)

| participants | rustorb | rustorb auth | rustorb sign | rustorb encrypt | cyclonedds | cyclonedds auth | cyclonedds sign | cyclonedds encrypt | fastdds | fastdds auth | fastdds sign | fastdds encrypt |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 2 | 0.03 | 0.04 | 0.04 | 0.05 | 0.01 | 0.05 | 0.05 | 0.05 | 0.03 | 0.06 | 0.06 | 0.06 |
| 5 | 0.08 | 0.13 | 0.17 | 0.15 | 0.05 | 0.20 | 0.23 | 0.23 | 0.09 | 0.24 | 0.29 | 0.29 |
| 10 | 0.17 | 0.41 | 0.50 | 0.49 | 0.13 | 0.64 | 0.77 | 0.75 | 0.26 | 0.80 | 1.40 | 1.38 |
| 20 | 0.81 | 1.76 | 2.32 | 2.38 | 0.53 | 2.15 | 2.92 | 2.92 | 0.89 | 3.67 | 9.92 (3 of 3 not all matched) | 9.25 (3 of 3 not all matched) |

#### UDP octets of the run (KiB)

| participants | rustorb | rustorb auth | rustorb sign | rustorb encrypt | cyclonedds | cyclonedds auth | cyclonedds sign | cyclonedds encrypt | fastdds | fastdds auth | fastdds sign | fastdds encrypt |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 2 | 4 | 18 | 31 | 31 | 6 | 20 | 30 | 31 | 26 | 58 | 69 | 69 |
| 5 | 37 | 160 | 298 | 299 | 50 | 201 | 325 | 328 | 180 | 762 | 1,013 | 1,019 |
| 10 | 165 | 707 | 1,321 | 1,342 | 201 | 912 | 1,464 | 1,479 | 824 | 6,126 | 8,717 | 9,192 |
| 20 | 733 | 3,012 | 5,622 | 5,745 | 915 | 3,955 | 6,406 | 6,507 | 4,717 | 59,474 | 116,087 (3 of 3 not all matched) | 111,849 (3 of 3 not all matched) |

## 言語バインディング（#107）

C++、Java、.NET、Python、Ruby の各バインディングで書いた ping（[bench/bindings](../bench/bindings)）から、Rust の `rustorb-dds-perf pong` に往復させ、Rust の ping との差をバインディングの上乗せとみなした。型は `bench/idl/Perf.idl` を rustorb-idl で生成したもの（`bench/bindings/generated`）。バインディングには共有メモリを切る設定がないので、どれも共有メモリ（Rust 側は `--transport shm`）。reliable、KEEP_LAST 1、各 5 秒を 3 回、その中央値。

```sh
bench/bindings.py                                   # 見つかったバインディングすべて
bench/bindings.py --bindings rust,cpp,python --sizes 32,4096
```

数値は [bench/results/2026-11-26-container-bindings](../bench/results/2026-11-26-container-bindings)（直す前は `before/`）。

わかったこと:

- 測り始めたとき、どのバインディングも 4 KiB 以上で桁違いに遅かった（64 KiB の往復が Rust 200 µs に対して 5.7〜8.5 ms）。バインディングは標本を `DynamicData` で受け渡していて、`DynamicData` が octet の列を 1 要素ずつ `DynamicData` として持っていたため（1 octet に数十バイトと割り当て 1 回）。octet（byte、uint8）の列と配列はバイト列のまま持つようにし、CDR の読み書きと `get/set_byte_values` などをまとめて行うようにした。Python はさらに `bytes` との変換を 1 回で行う。→ #120
- 直したあと（往復の中央値、µs、括弧内は Rust との差）:

| size | rust | cpp | java | dotnet | python | ruby |
|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 85 | 96 (+10) | 100 (+15) | 92 (+7) | 120 (+35) | 103 (+18) |
| 4 KiB | 94 | 98 (+4) | 103 (+9) | 92 (-1) | 118 (+24) | 193 (+99) |
| 64 KiB | 195 | 215 (+20) | 251 (+56) | 248 (+53) | 270 (+75) | 1,245 (+1,050) |

- 直す前:

| size | rust | cpp | java | dotnet | python | ruby |
|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 87 | 102 (+15) | 102 (+15) | 106 (+18) | 134 (+47) | 125 (+38) |
| 4 KiB | 91 | 562 (+471) | 580 (+489) | 604 (+514) | 743 (+652) | 695 (+604) |
| 64 KiB | 200 | 5,750 (+5,549) | 6,770 (+6,570) | 7,468 (+7,268) | 8,466 (+8,266) | 7,130 (+6,929) |

- 小さな標本では、上乗せは C++・Java・.NET で 10〜15 µs、Python と Ruby で 20〜35 µs。
- Ruby は 64 KiB でまだ約 1 ms 上乗せがある。Ruby の型対応では octet の列が Integer の Array なので、受け取るたびに 65,536 要素の Array を作るため（送る側は String を渡せる）。

## OpenDDS と RustDDS（#108）

同じ手順のプログラムを OpenDDS 3.31 と RustDDS 0.15.1 にも作った。

- OpenDDS: [bench/opendds/perf.cpp](../bench/opendds/perf.cpp)（DCPS の公開 API、RTPS discovery と RTPS_UDP、[rtps.ini](../bench/opendds/rtps.ini)）。OpenDDS の Docker イメージ（ghcr.io/opendds/opendds:DDS-3.31）の中でビルドし、リンクするライブラリをイメージから写して、イメージの外で動かす（CPU の固定、CPU 時間、RSS がほかの実装と同じように測れるように）。コンテナからはイメージを取れないので、CI のジョブ `bench-opendds` で、同じ runner の rustorb-dds と並べて測る。
- RustDDS: [bench/rustdds](../bench/rustdds)（ワークスペースの外の別 crate、publish しない。RustDDS に依存するのはこのプログラムだけ）。payload は `serde_bytes::ByteBuf`（RustDDS の CDR がまとめて読み書きする）、reader は mio で待つ。RustDDS 0.15 では keyless な writer と reader の matched status が取れないので、status イベントから数える。

```sh
bench/run.py --peers rustorb,rustdds,cyclonedds --transports udp --interop --out bench/results/<名前>
docker pull ghcr.io/opendds/opendds:DDS-3.31   # OpenDDS（CI の bench-opendds と同じ）
bench/run.py --peers rustorb,opendds --transports udp --interop --out bench/results/<名前>
```

測っている途中で見つかったこと:

- rustorb-dds は、相手の reader（と writer）が広告する unicast locator のすべてに同じメッセージを送っていた。RustDDS は自分のアドレスに加えて 127.0.0.1 も広告するので、どのメッセージも 2 回届いていた。そのうえ RustDDS は、届け終わった標本の DATA_FRAG がもう一度来ると組み立て用のバッファを新しく作り、128 個たまると待っている標本のほうを捨てるので、64 KiB の標本が 128 個目で止まった。いまの rustorb-dds は、相手が同じホストにいれば（そのアドレスの 1 つが自分のもの）1 つだけ（loopback があればそれ）に送り、ほかのホストの loopback には送らない（`rustorb_rtps::udp::select_unicast`）。
- RustDDS 0.15 の reader は、RESOURCE_LIMITS をつけると標本を 1 つも渡さない。ほかの実装の best-effort の sub は KEEP_ALL で 1000 標本までだが、RustDDS だけ KEEP_LAST(1000) にした（溢れたときに捨てるのが新しい標本ではなく古い標本になる）。
- RustDDS の best-effort の sub は、速い pub に追いつかないあいだ、受け取ったものをため続ける（rustorb-dds の pub から 64 KiB で 5 秒に約 3 GiB）。CI の runner がこれで落ちたので、`bench/run.py` は RSS が `--max-rss-mib`（既定 3 GiB）を超えた側を止め、その回を「Killed for memory」として数値なしで記録する。
- OpenDDS の reliable の writer は、未確認応答 100 標本の窓が埋まると、既定の heartbeat 周期（1 秒）ごとにしか進まない。reliable のスループットが大きさによらず約 100 標本/秒（1 MiB で 814 Mbit/s、32 B で 0.03 Mbit/s）なのはこのためで、OpenDDS の設定を変えれば変わる（ここでは既定のまま測った）。

### OpenDDS（CI）

数値は [bench/results/2026-11-27-ci-opendds](../bench/results/2026-11-27-ci-opendds/report.md)（GitHub の ubuntu-24.04 runner、4 CPU、rustorb-dds f01bc33。0.5 秒の warm-up のあと 3 秒、各 1 回）。コンテナの数値とは並べないこと。

| size | rustorb | OpenDDS | rustorb ping ↔ OpenDDS pong | OpenDDS ping ↔ rustorb pong |
|---:|---:|---:|---:|---:|
| 32 B | 67 | 297 | 728 | 695 |
| 16 KiB | 85 | 337 | 2,431 | 2,520 |
| 64 KiB | 155 | 598 | 1,743 | 1,776 |
| 1 MiB | 1,371 | 3,172 | 5,029 | 5,124 |

（reliable の往復の中央値、µs）

- OpenDDS どうしの往復は rustorb-dds どうしの約 4 倍（32 B で 297 µs と 67 µs）。best-effort でも 267 µs と 47 µs。
- rustorb-dds と OpenDDS の組の reliable の往復は、どちら向きでも 0.7〜2.5 ms で、OpenDDS どうしより長い。best-effort の組は 140 µs（OpenDDS どうしより短い）なので、reliable の HEARTBEAT・ACKNACK のやりとりの待ちと思われる。→ 調べる
- OpenDDS の 1 MiB は、best-effort で往復がすべて失われ、reliable の 99 パーセンタイルが 824 ms。
- スループット（best-effort）は 64 KiB で rustorb 14.3 Gbit/s、OpenDDS 5.0 Gbit/s、OpenDDS → rustorb 5.4 Gbit/s。rustorb → OpenDDS は 4.2 Gbit/s で取りこぼしが多い（OpenDDS の受け手が追いつかない）。
- 1 プロセスの最大 RSS は、OpenDDS が 37〜43 MiB（reliable の往復では 218 MiB）、rustorb-dds が 5〜12 MiB（best-effort の受け手は 34 MiB）。

### RustDDS（コンテナ）

（測定中）

## 結果の表

`bench/report.py` の出力。列は実装と転送。2 つの実装の組は、レイテンシでは「ping↔pong」、スループットでは「pub→sub」。「lost」はすべての往復が失われたもの、「–」は測っていないもの。

### Latency, reliable: round trip, median (µs)

| size | rustorb udp | rustorb shm | rustorb loan | cyclonedds udp | cyclonedds shm | cyclonedds-ddsperf udp | cyclonedds-ddsperf shm | fastdds udp | fastdds shm | rustorb↔cyclonedds udp | rustorb↔fastdds udp | cyclonedds↔rustorb udp | fastdds↔rustorb udp |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 78.6 | 79.1 | 92.7 | 66.2 | 63.5 | 17.9 | 17.7 | 89.1 | 86.5 | 71.9 | 78.4 | 72.3 | 78.0 |
| 64 B | 80.1 | 80.7 | 92.5 | 66.8 | 64.2 | – | – | 88.3 | 88.4 | 72.8 | 75.0 | 74.3 | 78.3 |
| 128 B | 80.7 | 77.9 | 94.7 | 67.7 | 65.8 | – | – | 91.3 | 88.6 | 72.9 | 77.2 | 72.0 | 75.9 |
| 256 B | 80.9 | 78.9 | 94.7 | 65.4 | 66.8 | – | – | 89.6 | 88.6 | 74.3 | 76.9 | 76.7 | 77.3 |
| 512 B | 78.5 | 78.7 | 95.3 | 67.5 | 66.0 | – | – | 89.0 | 88.9 | 73.3 | 75.9 | 72.9 | 78.4 |
| 1 KiB | 80.6 | 78.8 | 95.5 | 67.9 | 67.5 | 18.7 | 18.5 | 91.6 | 84.7 | 75.0 | 78.1 | 75.2 | 79.5 |
| 2 KiB | 83.6 | 83.4 | 94.0 | 69.6 | 67.9 | – | – | 92.1 | 89.1 | 76.2 | 80.0 | 75.7 | 82.9 |
| 4 KiB | 83.9 | 85.6 | 94.4 | 71.3 | 67.8 | – | – | 94.2 | 90.7 | 79.2 | 81.0 | 79.9 | 84.6 |
| 8 KiB | 86.1 | 86.8 | 95.3 | 75.6 | 68.9 | – | – | 98.6 | 91.4 | 84.0 | 90.0 | 84.5 | 91.5 |
| 16 KiB | 143.1 | 139.2 | 97.0 | 84.5 | 71.4 | 23.4 | 24.2 | 107.4 | 99.3 | 120.5 | 99.1 | 120.7 | 100.8 |
| 32 KiB | 165.8 | 166.0 | 95.7 | 94.8 | 77.8 | – | – | 119.1 | 106.8 | 128.9 | 117.9 | 136.4 | 120.8 |
| 64 KiB | 200.8 | 210.9 | 96.7 | 142.4 | 95.7 | 50.7 | 50.3 | 130.4 | 125.7 | 167.7 | 176.8 | 182.8 | 176.2 |
| 128 KiB | 409.2 | 376.4 | 95.5 | 235.8 | 153.2 | – | – | 178.2 | 156.0 | 267.6 | 264.2 | 317.8 | 261.4 |
| 256 KiB | 801.3 | 645.2 | 98.1 | 456.7 | 368.8 | – | – | 305.5 | 254.6 | 536.1 | 554.4 | 676.3 | 631.1 |
| 512 KiB | 2,291 | 2,114 | 100.0 | 832.1 | 684.1 | – | – | 10,120 | 662.9 | 1,006 | 4,020 | 1,353 | 1,842 |
| 1 MiB | 3,215 | 2,123 | 110.0 | 1,780 | 1,296 | 857.4 | 856.9 | 12,339 | 1,136 | 2,790 | 6,576 | 2,675 | 6,543 |

In this table the runs of a cell differed by 4 % (median over the cells), 2970 % at most.

### Latency, reliable: round trip, 99th percentile (µs)

| size | rustorb udp | rustorb shm | rustorb loan | cyclonedds udp | cyclonedds shm | cyclonedds-ddsperf udp | cyclonedds-ddsperf shm | fastdds udp | fastdds shm | rustorb↔cyclonedds udp | rustorb↔fastdds udp | cyclonedds↔rustorb udp | fastdds↔rustorb udp |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 138.7 | 142.7 | 190.2 | 124.7 | 115.1 | 35.4 | 34.2 | 170.2 | 166.7 | 136.7 | 175.1 | 161.9 | 167.0 |
| 64 B | 137.1 | 141.4 | 162.7 | 125.5 | 115.0 | – | – | 155.4 | 159.6 | 135.5 | 150.0 | 150.1 | 164.2 |
| 128 B | 142.8 | 140.9 | 179.8 | 122.5 | 126.4 | – | – | 190.9 | 152.2 | 130.6 | 152.8 | 144.9 | 171.3 |
| 256 B | 141.1 | 147.5 | 166.7 | 119.1 | 142.5 | – | – | 181.8 | 151.7 | 130.7 | 133.2 | 149.9 | 160.3 |
| 512 B | 128.9 | 145.4 | 174.0 | 114.9 | 127.7 | – | – | 181.3 | 158.8 | 137.2 | 150.9 | 141.7 | 168.4 |
| 1 KiB | 129.4 | 143.6 | 164.2 | 117.4 | 146.9 | 38.5 | 36.9 | 168.9 | 150.5 | 132.0 | 170.2 | 138.3 | 163.4 |
| 2 KiB | 136.4 | 144.2 | 155.0 | 129.2 | 127.6 | – | – | 180.1 | 154.6 | 140.8 | 157.5 | 149.3 | 176.5 |
| 4 KiB | 141.4 | 148.1 | 168.1 | 123.1 | 131.9 | – | – | 181.9 | 156.8 | 143.1 | 164.3 | 147.1 | 175.7 |
| 8 KiB | 137.3 | 145.7 | 162.4 | 140.5 | 123.8 | – | – | 171.4 | 161.1 | 149.3 | 165.4 | 163.4 | 183.4 |
| 16 KiB | 219.3 | 249.0 | 167.9 | 148.1 | 150.0 | 53.1 | 56.6 | 216.1 | 179.7 | 192.8 | 193.3 | 214.8 | 191.0 |
| 32 KiB | 281.7 | 289.5 | 175.0 | 185.2 | 140.8 | – | – | 231.7 | 182.6 | 205.9 | 222.8 | 289.5 | 245.2 |
| 64 KiB | 364.4 | 423.3 | 155.3 | 311.6 | 181.6 | 98.8 | 105.2 | 285.7 | 246.0 | 309.6 | 441.9 | 464.8 | 441.1 |
| 128 KiB | 708.1 | 747.1 | 161.4 | 397.5 | 340.3 | – | – | 395.2 | 323.9 | 558.9 | 661.2 | 660.9 | 607.5 |
| 256 KiB | 1,209 | 1,112 | 164.7 | 764.7 | 718.4 | – | – | 11,081 | 535.6 | 924.2 | 6,329 | 1,212 | 6,385 |
| 512 KiB | 3,174 | 2,877 | 186.3 | 1,307 | 1,106 | – | – | 43,391 | 11,319 | 2,119 | 250,622 | 2,497 | 237,989 |
| 1 MiB | 4,651 | 3,435 | 182.0 | 3,050 | 1,868 | 1,330 | 1,479 | 54,894 | 22,222 | 4,322 | 400,217 | 4,401 | 350,854 |

In this table the runs of a cell differed by 14 % (median over the cells), 95 % at most.

### Throughput, reliable (Mbit/s of payload received)

| size | rustorb udp | rustorb shm | rustorb loan | cyclonedds udp | cyclonedds shm | cyclonedds-ddsperf udp | cyclonedds-ddsperf shm | fastdds udp | fastdds shm | rustorb→cyclonedds udp | rustorb→fastdds udp | cyclonedds→rustorb udp | fastdds→rustorb udp |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 33.2 | 31.2 | 28.2 | 29.2 | 29.9 | 291.5 | 258.5 | 4.7 | 4.7 | 31.6 | 4.4 | 92.8 | 11.6 |
| 256 B | 256.0 | 236.2 | 227.2 | 256.1 | 254.8 | 1,277 | 1,219 | 37.2 | 37.4 | 253.0 | 35.7 | 528.2 | 88.6 |
| 1 KiB | 917.8 | 873.3 | 903.0 | 870.1 | 819.8 | 3,498 | 3,447 | 148.8 | 149.4 | 938.0 | 141.3 | 1,789 | 364.3 |
| 4 KiB | 3,512 | 3,477 | 3,676 | 2,584 | 2,626 | 6,373 | 5,999 | 388.8 | 599.7 | 3,186 | 296.1 | 4,935 | 1,366 |
| 16 KiB | 8,666 | 9,003 | 5,929 | 5,266 | 5,068 | 7,151 | 6,988 | 1,066 | 1,937 | 7,954 | 595.4 | 7,355 | 4,994 |
| 64 KiB | 9,327 | 11,936 | 14,772 | 7,373 | 11,035 | 10,277 | 9,876 | 3,203 | 3,734 | 4,222 | 396.8 | 6,715 | 9,790 |
| 256 KiB | 10,105 | 9,997 | 113,476 | 8,670 | 10,201 | 9,248 | 9,852 | 8,254 | 10,583 | 6,977 | 1,152 | 10,440 | 8,800 |
| 1 MiB | 10,311 | 12,117 | 2,329 | 8,518 | 9,363 | 7,371 | 7,259 | 10,199 | 12,289 | 3,357 | 4,540 | 11,110 | 7,065 |

In this table the runs of a cell differed by 7 % (median over the cells), 16292 % at most.

### Latency, best-effort: round trip, median (µs)

| size | rustorb udp | rustorb shm | rustorb loan | cyclonedds udp | cyclonedds shm | cyclonedds-ddsperf udp | cyclonedds-ddsperf shm | fastdds udp | fastdds shm | rustorb↔cyclonedds udp | rustorb↔fastdds udp | cyclonedds↔rustorb udp | fastdds↔rustorb udp |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 75.2 | 74.6 | 85.2 | 66.1 | 65.6 | 17.2 | 17.8 | 73.1 | 71.1 | 69.5 | 70.2 | 72.3 | 71.6 |
| 64 B | 76.2 | 74.6 | 87.0 | 66.5 | 66.2 | – | – | 72.1 | 69.4 | 69.5 | 70.5 | 69.3 | 72.5 |
| 128 B | 77.3 | 76.9 | 85.5 | 65.8 | 65.3 | – | – | 71.0 | 70.8 | 68.5 | 71.2 | 73.3 | 72.3 |
| 256 B | 74.6 | 77.7 | 88.0 | 65.4 | 67.7 | – | – | 71.7 | 69.4 | 70.9 | 72.1 | 70.2 | 69.6 |
| 512 B | 76.7 | 74.7 | 88.4 | 66.3 | 67.6 | – | – | 71.9 | 72.4 | 68.9 | 71.4 | 71.5 | 71.8 |
| 1 KiB | 76.5 | 74.6 | 87.5 | 64.5 | 67.0 | 18.2 | 18.1 | 72.7 | 72.3 | 70.7 | 72.2 | 71.6 | 73.0 |
| 2 KiB | 79.4 | 76.5 | 88.1 | 66.9 | 68.5 | – | – | 74.8 | 70.5 | 72.3 | 72.7 | 72.8 | 74.1 |
| 4 KiB | 79.1 | 78.6 | 87.7 | 69.4 | 69.0 | – | – | 73.9 | 73.4 | 73.3 | 76.1 | 75.9 | 75.2 |
| 8 KiB | 84.7 | 83.3 | 88.2 | 72.2 | 70.9 | – | – | 80.2 | 77.6 | 78.2 | 80.0 | 79.4 | 81.2 |
| 16 KiB | 93.6 | 88.4 | 89.0 | 80.2 | 72.5 | 21.9 | 22.6 | 85.2 | 81.1 | 103.7 | 87.6 | 106.3 | 86.9 |
| 32 KiB | 112.8 | 105.0 | 90.3 | 93.5 | 78.2 | – | – | 99.9 | 92.1 | 113.7 | 103.0 | 118.5 | 103.9 |
| 64 KiB | 190.9 | 187.4 | 90.7 | 141.0 | 98.5 | 47.0 | 49.4 | 134.9 | 118.1 | 158.8 | 164.6 | 161.0 | 165.3 |
| 128 KiB | 315.9 | 348.9 | 90.3 | 231.0 | 153.5 | – | – | 166.9 | 152.2 | 241.1 | 239.7 | 391.7 | 304.3 |
| 256 KiB | 832.9 | 818.6 | 91.6 | 453.2 | 369.8 | – | – | lost | 278.1 | 468.4 | lost | 675.5 | lost |
| 512 KiB | 1,823 | 1,545 | 92.5 | 818.6 | 667.7 | – | – | lost | 736.9 | 905.4 | lost | 1,282 | lost |
| 1 MiB | 2,152 | 2,097 | 101.6 | 2,508 | 1,301 | 743.4 | 930.8 | lost | 1,364 | 2,727 | lost | 2,041 | lost |

In this table the runs of a cell differed by 4 % (median over the cells), 126 % at most.

### Latency, best-effort: round trip, 99th percentile (µs)

| size | rustorb udp | rustorb shm | rustorb loan | cyclonedds udp | cyclonedds shm | cyclonedds-ddsperf udp | cyclonedds-ddsperf shm | fastdds udp | fastdds shm | rustorb↔cyclonedds udp | rustorb↔fastdds udp | cyclonedds↔rustorb udp | fastdds↔rustorb udp |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 124.4 | 131.5 | 148.4 | 130.1 | 121.0 | 33.3 | 34.5 | 135.8 | 134.8 | 123.7 | 131.3 | 137.6 | 147.2 |
| 64 B | 132.7 | 131.8 | 150.9 | 129.4 | 124.6 | – | – | 137.8 | 122.2 | 116.0 | 140.6 | 119.1 | 153.1 |
| 128 B | 123.4 | 142.7 | 148.4 | 123.2 | 126.5 | – | – | 127.8 | 141.7 | 128.5 | 146.1 | 135.3 | 156.2 |
| 256 B | 127.7 | 139.0 | 146.8 | 129.4 | 135.3 | – | – | 135.8 | 128.8 | 141.4 | 134.8 | 131.9 | 140.2 |
| 512 B | 133.7 | 128.0 | 155.1 | 121.5 | 125.2 | – | – | 134.2 | 134.6 | 126.8 | 133.4 | 127.5 | 144.0 |
| 1 KiB | 132.7 | 122.4 | 156.1 | 115.0 | 117.1 | 33.2 | 37.0 | 130.4 | 137.5 | 136.0 | 127.4 | 131.7 | 146.8 |
| 2 KiB | 134.5 | 127.6 | 169.1 | 120.1 | 117.0 | – | – | 153.6 | 131.7 | 126.8 | 147.3 | 139.0 | 145.3 |
| 4 KiB | 130.2 | 163.7 | 158.3 | 121.6 | 135.2 | – | – | 134.8 | 130.9 | 133.1 | 145.9 | 133.8 | 137.2 |
| 8 KiB | 148.6 | 146.8 | 149.9 | 125.6 | 130.4 | – | – | 149.4 | 147.6 | 138.0 | 155.8 | 141.3 | 148.2 |
| 16 KiB | 155.2 | 161.1 | 157.2 | 141.1 | 142.6 | 49.8 | 51.2 | 165.5 | 149.9 | 188.2 | 165.0 | 205.4 | 172.2 |
| 32 KiB | 192.5 | 182.6 | 169.2 | 185.9 | 144.1 | – | – | 174.5 | 165.3 | 223.6 | 232.2 | 209.9 | 191.7 |
| 64 KiB | 362.5 | 378.6 | 164.4 | 264.4 | 168.9 | 89.7 | 95.3 | 308.8 | 207.6 | 346.6 | 435.1 | 357.3 | 385.4 |
| 128 KiB | 544.7 | 750.4 | 163.8 | 514.5 | 334.3 | – | – | 454.2 | 321.4 | 567.9 | 632.9 | 698.4 | 639.4 |
| 256 KiB | 1,245 | 1,250 | 155.7 | 875.1 | 733.6 | – | – | lost | 633.2 | 829.2 | lost | 1,156 | lost |
| 512 KiB | 2,323 | 2,195 | 177.5 | 1,591 | 1,037 | – | – | lost | 1,831 | 1,456 | lost | 1,997 | lost |
| 1 MiB | 3,381 | 3,008 | 164.9 | 4,556 | 1,941 | 1,121 | 1,445 | lost | 2,782 | 4,448 | lost | 3,489 | lost |

In this table the runs of a cell differed by 13 % (median over the cells), 138 % at most.

### Throughput, best-effort (Mbit/s of payload received)

| size | rustorb udp | rustorb shm | rustorb loan | cyclonedds udp | cyclonedds shm | cyclonedds-ddsperf udp | cyclonedds-ddsperf shm | fastdds udp | fastdds shm | rustorb→cyclonedds udp | rustorb→fastdds udp | cyclonedds→rustorb udp | fastdds→rustorb udp |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 37.9 | 76.1 | 38.6 | 23.6 | 18.3 | 388.7 | 397.4 | 23.0 | 15.3 | 29.2 | 20.9 | 24.2 | 37.7 |
| 256 B | 556.5 | 582.4 | 308.2 | 223.1 | 165.1 | 3,075 | 3,058 | 184.9 | 128.9 | 236.7 | 174.8 | 263.8 | 360.9 |
| 1 KiB | 1,305 | 1,914 | 1,213 | 762.2 | 692.9 | 7,629 | 8,191 | 706.4 | 503.4 | 795.9 | 657.7 | 1,158 | 896.9 |
| 4 KiB | 4,865 | 3,486 | 4,909 | 2,654 | 2,177 | 10,987 | 14,255 | 2,406 | 1,734 | 2,564 | 1,944 | 4,077 | 4,338 |
| 16 KiB | 10,090 | 7,293 | 19,745 | 6,457 | 5,710 | 16,255 | 14,093 | 6,925 | 7,061 | 7,380 | 6,541 | 7,339 | 8,002 |
| 64 KiB | 11,396 | 10,674 | 78,031 | 13,602 | 12,235 | 15,997 | 14,156 | 11,962 | 14,720 | 16,411 | 7,693 | 10,753 | 8,778 |
| 256 KiB | 11,777 | 7,937 | 305,924 | 16,593 | 17,700 | 24,833 | 24,062 | 17,825 | 18,155 | 17,807 | 12,536 | 9,639 | 9,664 |
| 1 MiB | 12,140 | 13,071 | 1,244,514 | 16,764 | 11,585 | 7,161 | 6,960 | 15,022 | 19,673 | 11,241 | 803.6 | 8,652 | 10,737 |

In this table the runs of a cell differed by 13 % (median over the cells), 107 % at most.

Samples lost, best-effort:

| size | rustorb udp | rustorb shm | rustorb loan | cyclonedds udp | cyclonedds shm | cyclonedds-ddsperf udp | cyclonedds-ddsperf shm | fastdds udp | fastdds shm | rustorb→cyclonedds udp | rustorb→fastdds udp | cyclonedds→rustorb udp | fastdds→rustorb udp |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 928147 | 8763 | 207877 | 129701 | 1548 | 2311527 | 2607183 | 2171241 | 701271 | 1314506 | 1569797 | 0 | 1676057 |
| 256 B | 377410 | 15497 | 191750 | 317510 | 21184 | 758128 | 904834 | 2206182 | 717202 | 1245207 | 1465203 | 1053 | 1151382 |
| 1 KiB | 693588 | 72124 | 193398 | 356236 | 65887 | 239785 | 540852 | 2118361 | 1537369 | 1210294 | 1393047 | 1900 | 1486908 |
| 4 KiB | 534374 | 1040179 | 292624 | 345337 | 58386 | 58371 | 97569 | 1797318 | 2065332 | 1154891 | 946596 | 3872 | 1163037 |
| 16 KiB | 381710 | 879828 | 259021 | 106703 | 21875 | 18109 | 17851 | 870240 | 1137777 | 554568 | 710153 | 16272 | 429193 |
| 64 KiB | 105859 | 157318 | 254512 | 6893 | 2781 | 3017 | 4232 | 235887 | 234223 | 66825 | 120894 | 76748 | 174105 |
| 256 KiB | 26730 | 28 | 211649 | 2050 | 1033 | 14638 | 1977 | 23920 | 33887 | 1772 | 4981 | 45942 | 42034 |
| 1 MiB | 1467 | 2685 | 352504 | 267 | 156 | 12727 | 11687 | 3098 | 2129 | 68 | 8482 | 9873 | 6406 |

### CPU time and memory, latency runs

| case | CPU s (measuring side) | CPU s (other side) | max RSS MiB (measuring) | max RSS MiB (other) |
|---|---:|---:|---:|---:|
| cyclonedds shm best-effort | 40.0 | 50.3 | 15.5 | 15.6 |
| cyclonedds shm reliable | 39.7 | 49.5 | 15.6 | 15.4 |
| cyclonedds udp best-effort | 46.9 | 48.4 | 13.7 | 13.1 |
| cyclonedds udp reliable | 48.5 | 48.6 | 14.7 | 14.1 |
| cyclonedds-ddsperf shm best-effort | 3.0 | 3.2 | 13.1 | 13.1 |
| cyclonedds-ddsperf shm reliable | 3.3 | 3.2 | 13.1 | 13.1 |
| cyclonedds-ddsperf udp best-effort | 3.3 | 3.2 | 13.1 | 13.1 |
| cyclonedds-ddsperf udp reliable | 3.3 | 3.3 | 13.1 | 13.1 |
| cyclonedds↔rustorb udp best-effort | 40.1 | 57.6 | 13.8 | 15.9 |
| cyclonedds↔rustorb udp reliable | 41.6 | 61.8 | 13.9 | 16.0 |
| fastdds shm best-effort | 40.9 | 40.7 | 23.3 | 21.2 |
| fastdds shm reliable | 59.9 | 59.5 | 23.9 | 21.4 |
| fastdds udp best-effort | 37.7 | 37.2 | 19.9 | 16.8 |
| fastdds udp reliable | 55.2 | 55.0 | 22.1 | 19.6 |
| fastdds↔rustorb udp best-effort | 34.5 | 39.9 | 21.4 | 13.4 |
| fastdds↔rustorb udp reliable | 58.3 | 47.9 | 22.2 | 16.0 |
| rustorb loan best-effort | 41.3 | 54.7 | 20.0 | 20.3 |
| rustorb loan reliable | 42.6 | 57.1 | 20.1 | 20.1 |
| rustorb shm best-effort | 45.6 | 49.7 | 22.5 | 22.7 |
| rustorb shm reliable | 49.2 | 49.8 | 24.0 | 23.7 |
| rustorb udp best-effort | 46.6 | 50.0 | 14.0 | 14.0 |
| rustorb udp reliable | 48.5 | 50.8 | 15.4 | 15.5 |
| rustorb↔cyclonedds udp best-effort | 55.1 | 43.5 | 14.5 | 13.2 |
| rustorb↔cyclonedds udp reliable | 60.2 | 45.0 | 15.9 | 13.2 |
| rustorb↔fastdds udp best-effort | 38.8 | 35.1 | 13.2 | 17.2 |
| rustorb↔fastdds udp reliable | 47.5 | 59.3 | 15.9 | 19.8 |

### CPU time and memory, throughput runs

| case | CPU s (measuring side) | CPU s (other side) | max RSS MiB (measuring) | max RSS MiB (other) |
|---|---:|---:|---:|---:|
| cyclonedds shm best-effort | 6.0 | 7.1 | 14.6 | 13.8 |
| cyclonedds shm reliable | 5.8 | 2.6 | 13.8 | 13.8 |
| cyclonedds udp best-effort | 6.7 | 7.1 | 13.8 | 13.8 |
| cyclonedds udp reliable | 5.8 | 2.4 | 13.8 | 13.8 |
| cyclonedds-ddsperf shm best-effort | 6.1 | 8.8 | 13.8 | 13.8 |
| cyclonedds-ddsperf shm reliable | 3.6 | 5.7 | 13.8 | 13.8 |
| cyclonedds-ddsperf udp best-effort | 6.1 | 8.9 | 13.8 | 13.8 |
| cyclonedds-ddsperf udp reliable | 3.5 | 7.5 | 13.8 | 13.8 |
| cyclonedds→rustorb udp best-effort | 8.0 | 7.0 | 26.3 | 13.8 |
| cyclonedds→rustorb udp reliable | 8.2 | 4.4 | 13.8 | 13.8 |
| fastdds shm best-effort | 6.6 | 6.0 | 21.9 | 17.3 |
| fastdds shm reliable | 2.1 | 1.8 | 18.2 | 17.3 |
| fastdds udp best-effort | 7.0 | 6.0 | 19.4 | 16.1 |
| fastdds udp reliable | 2.2 | 1.9 | 17.1 | 16.2 |
| fastdds→rustorb udp best-effort | 9.5 | 6.0 | 34.2 | 16.4 |
| fastdds→rustorb udp reliable | 5.8 | 6.8 | 13.8 | 16.4 |
| rustorb loan best-effort | 7.3 | 6.0 | 13.8 | 13.8 |
| rustorb loan reliable | 6.9 | 6.6 | 14.0 | 17.9 |
| rustorb shm best-effort | 8.6 | 6.0 | 30.9 | 13.8 |
| rustorb shm reliable | 8.1 | 6.5 | 17.5 | 18.1 |
| rustorb udp best-effort | 9.4 | 5.9 | 36.5 | 13.2 |
| rustorb udp reliable | 8.0 | 6.5 | 13.6 | 13.2 |
| rustorb→cyclonedds udp best-effort | 6.9 | 5.9 | 14.1 | 13.8 |
| rustorb→cyclonedds udp reliable | 8.4 | 5.9 | 13.8 | 13.8 |
| rustorb→fastdds udp best-effort | 7.0 | 6.0 | 21.6 | 13.8 |
| rustorb→fastdds udp reliable | 1.6 | 0.8 | 20.3 | 13.9 |
