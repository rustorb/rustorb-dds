# 共有メモリ転送

rustorb/rustorb#111 の D5（#55）の 3 番目、#58 をまとめる。同じホストにある rustorb-dds の参加者どうしは、RTPS メッセージを UDP ではなく共有メモリで渡す。ほかの実装とは今までどおり UDP で通じる。

## 使い方

Unix では既定で有効（`rustorb-dds` の feature `shm`、既定）。参加者ごとに切るには `ParticipantConfig::shared_memory = false`。feature を外す（`default-features = false`）と依存の memmap2 も入らない。

## しくみ

- 各参加者は受信用の segment を持つ。共有メモリのディレクトリ（Linux では `/dev/shm`、なければ一時ディレクトリ）のファイル `rustorb-dds-<id>` を mmap した ring buffer で、隣に通知用の Unix datagram socket `rustorb-dds-<id>.sock` を置く。大きさは既定 4 MiB。
- 送り手は相手の segment を開いて mmap し、segment の中の lock を取って ring に書き、socket に 1 octet 送って起こす。lock の持ち主のプロセスがなくなっていれば（Linux では `/proc/<pid>` で確かめる）奪う。
- 受け手のスレッドは通知か 100 ms ごとに ring を読み、UDP で受けたものと同じ経路で処理する。
- ring に入らないメッセージは捨てる。UDP で失われたのと同じで、reliable なら再送で直る。
- 参加者は SPDP で、UDP の locator に加えて共有メモリの locator を告知する。種別は vendor の値 `0x52440001`（`Locator::KIND_SHM`）、address はホストの識別子（machine-id のハッシュ）8 octet と segment の id 8 octet、port はメタトラフィックが 1、ユーザのトラフィックが 2。
- 送るときは、相手の locator にホストが同じで開ける共有メモリの locator があればそれだけを使い、なければ UDP の locator を使う。コンテナどうしのように machine-id が同じでも `/dev/shm` が別なら開けないので UDP になる。
- 参加者が終われば segment と socket のファイルを消す。異常終了したプロセスのファイルは残る（次に同じ id で開くときに消す）。

## 確かめたこと

- 同じホストの 2 プロセスで、100 kB の標本 20 個を reliable で送ったとき、UDP に流れたのは SPDP のマルチキャストだけだった（tcpdump）。
- Cyclone DDS 0.10.4、Fast DDS 2.11.2 とは、告知が増えても今までどおり通る（`interop/run.sh` の基本、reliable、large の各ケース）。
- 試験 `tests/shared_memory.rs`（大きな標本、共有メモリを使わない参加者との UDP）と `rustorb-rtps` の `shm` の単体試験。ほかの試験も、同じプロセスの参加者どうしは共有メモリを通る。

## 未対応

- Windows。
- 送り手から受け手への zero-copy（今は ring へのコピーと、受け手での取り出しのコピーがある）。
