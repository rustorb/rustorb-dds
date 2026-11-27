# 共有メモリ転送

rustorb/rustorb#111 の D5（#55）の 3 番目、#58 をまとめる。同じホストにある rustorb-dds の参加者どうしは、RTPS メッセージを UDP ではなく共有メモリで渡す。ほかの実装とは今までどおり UDP で通じる。

## 使い方

既定で有効（Linux、macOS、Windows。`rustorb-dds` の feature `shm`、既定）。参加者ごとに切るには `ParticipantConfig::shared_memory = false`。feature を外す（`default-features = false`）と依存の memmap2 も入らない。

## しくみ

- 各参加者は受信用の segment を持つ。共有メモリのディレクトリ（Linux では `/dev/shm`、なければ一時ディレクトリ）のファイル `rustorb-dds-<id>` を mmap した ring buffer で、受け手は通知用に 127.0.0.1 の UDP socket を開き、その port を segment の header に書く。大きさは既定 4 MiB。
- 送り手は相手の segment を開いて mmap し、segment の中の lock を取って ring に書き、その port に 1 octet 送って起こす。lock の持ち主のプロセスがなくなっていれば（Linux では `/proc/<pid>` で確かめる）奪う。`/proc` のない OS では、copy にかかるよりずっと長く持たれたままの lock を奪う。
- 受け手のスレッドは通知か 100 ms ごとに ring を読み、UDP で受けたものと同じ経路で処理する。
- ring に空きがなければ、送り手は受け手を起こして空くのを最大 5 ms 待つ（受け手は読んだ record から順に空ける）。それでも入らないメッセージは捨てる。UDP で失われたのと同じで、reliable なら再送で直る（#112）。
- 参加者は SPDP で、UDP の locator に加えて共有メモリの locator を告知する。種別は vendor の値 `0x52440001`（`Locator::KIND_SHM`）、address はホストの識別子（machine-id のハッシュ）8 octet と segment の id 8 octet、port はメタトラフィックが 1、ユーザのトラフィックが 2。
- 送るときは、相手の locator にホストが同じで開ける共有メモリの locator があればそれだけを使い、なければ UDP の locator を使う。コンテナどうしのように machine-id が同じでも `/dev/shm` が別なら開けないので UDP になる。
- 参加者が終われば segment のファイルを消す。異常終了したプロセスのファイルは残る（次に同じ id で開くときに消す）。Windows では、ほかのプロセスが map している間は消せずに残ることがある。

## 確かめたこと

- Windows では CI の Windows job（`cargo test --workspace`）で同じ試験を通す。
- 同じホストの 2 プロセスで、100 kB の標本 20 個を reliable で送ったとき、UDP に流れたのは SPDP のマルチキャストだけだった（tcpdump）。
- Cyclone DDS 0.10.4、Fast DDS 2.11.2 とは、告知が増えても今までどおり通る（`interop/run.sh` の基本、reliable、large の各ケース）。
- 試験 `tests/shared_memory.rs`（大きな標本、共有メモリを使わない参加者との UDP）、`tests/loan.rs`（貸し出し: 同じ参加者と別の参加者の reader、slot を持ち続けたときのヒープへの切り替えと返したあとの再利用、共有メモリのない参加者への copy、keyed、並びが直列化した形でない型、共有メモリなし）と `rustorb-rtps` の `shm` の単体試験。ほかの試験も、同じプロセスの参加者どうしは共有メモリを通る。

## 貸し出し（zero-copy、#74）

大きさの決まった型の標本を、書き手が共有メモリの上で直接作り、受け手はそのまま読む。シリアライズも copy もしない。Cyclone DDS の loan API にならった rustorb 独自の API。

```rust
// SAFETY: repr(C) の平たいデータで、メモリ上の並びが直列化した形と同じ。
#[repr(C)]
#[derive(Clone, Copy)]
struct Frame { id: u32, len: u32, stamp: u64, data: [u8; 1 << 20] }
unsafe impl rustorb_dds::PlainType for Frame {}

let mut f = writer.loan_sample()?;   // WriteLoan<Frame>: 共有メモリの slot
f.id = 1;
f.data[..3].copy_from_slice(b"abc");
writer.write_loaned(f)?;

for s in reader.take_loaned() {      // Sample<ReadLoan<Frame>>
    if let Some(f) = s.data {
        println!("{} {}", f.id, f.is_shared());   // slot の上をそのまま読む
    }                                // drop で slot を返す
}
```

- 型は unsafe trait `PlainType` を実装する。`#[repr(C)]`（または primitive、その配列）で、ポインタを持たず、全 0 でも、書き手が入れたどの値でも有効で、メモリ上の並びが書き手のデータ表現（XCDR1 は各 member をその大きさに、XCDR2 は最大 4 に整列）と host の byte order での直列化した形（encapsulation の 4 octet の後）と同じであること。整数、浮動小数点、その配列には実装済み。
- 書き手は最初の貸し出し標本で、メモリ上の並びが直列化した形と同じかを確かめる（直列化して比べる）。違えば直列化して送る（遅いが正しい）。全 0 の標本の直列化した長さが型の大きさ + 4 でないとき、整列が 8 を超えるときも slot を使わない。
- `loan_sample` の中身は、slot を初めて使うときは 0、使い回すときは前の標本。全 member を書くこと。
- 書き手ごとに pool を作る。共有メモリのディレクトリのファイル `rustorb-dds-pool-<id>` で、大きさの決まった slot を並べる。slot の数は KEEP_LAST(n) で 2n + 16、KEEP_ALL で `max_samples`（なければ 32）+ 16、最大 256。各 slot には世代と参照数（AtomicU64）があり、書き手は参照数 0 の slot を世代を上げて取る。
- 書き手は、送る相手が全部「同じ参加者の reader」か「共有メモリで届く reader」（共有メモリの locator は rustorb-dds だけが告知する）で、書き手が VOLATILE、DDS Security が off のときに限り、標本の代わりに参照（encapsulation `0x5244`、pool の id、slot、世代で 20 octet）を送る。それ以外の相手がいれば slot の octet をそのまま送る（直列化はしないが copy はする）。同じ参加者の reader には今までどおり直接渡すので、そこでも copy しない。
- 受け手は参照を受けたとき pool を開いて（開いた pool は覚えておく）slot の参照数を上げる。世代が違えば（slot が使い回された後なら）その標本は捨てる。
- slot を返すのは、書き手の history からその変更がなくなり（最新の 8 個は history になくても持っておく。best-effort の受け手が取り込む前に使い回さないため）、受け手が全員 `ReadLoan` を drop したとき。slot が尽きると、reliable の書き手の `loan_sample` は受け手の ACKNACK を求め、`max_blocking_time` まで slot が空くのを待つ（#112）。best-effort の書き手と、待っても空かなかったときは、ヒープの標本を返し、書き込みは直列化して送る。
- keyed な型では、受け手は `get_key_value` などのため各 instance の最後の値を copy して持つ（keyless なら copy しない）。
- 共有メモリを使わないとき（feature `shm` なし、`ParticipantConfig::shared_memory = false`）や DDS Security を使うときは、`loan_sample` はヒープの標本を返し、`write_loaned` は `write` と同じ。`take_loaned` は直列化された標本を復元して返す（`is_shared()` が false）。

### 性能

同じプロセスの 2 参加者（共有メモリ）で、1 標本を書いて相手が take するまでの時間（`cargo test --release -p rustorb-dds --test loan -- --ignored --nocapture`、4 vCPU の Xeon 2.1 GHz、reliable、KEEP_LAST 8）:

| 標本 | write + take | 貸し出し | 比 |
|---|---|---|---|
| 4 KiB | 118 µs | 69 µs | 1.7 倍 |
| 64 KiB | 944 µs | 76 µs | 12 倍 |
| 1 MiB | 12.4 ms | 72 µs | 172 倍 |

貸し出しでは大きさによらずほぼ一定（参照の 1 メッセージと通知の時間）。

## 未対応

- C、C++、Java、Python からの貸し出し（Rust の API だけ）。rustorb-idl が `PlainType` を生成すること。
- 可変長の型（string、sequence）の貸し出し。
- Windows では、受け手が map している pool のファイルは書き手が終わっても消えずに残ることがある。
