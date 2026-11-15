# TRANSIENT と PERSISTENT

D6（rustorb-dds#63）の #71 をまとめる。DURABILITY の TRANSIENT と PERSISTENT（DDS 1.4、2.2.3.4）と、DURABILITY_SERVICE（2.2.3.5）を実装した。

## 使い方

- writer の QoS の `durability` を `DurabilityKind::Transient` か `Persistent` にする。reader は TRANSIENT_LOCAL 以上なら、writer が消えた後に作られても、残されたサンプルを受け取る（offered ≥ requested の順序は VOLATILE < TRANSIENT_LOCAL < TRANSIENT < PERSISTENT）。
- `DataWriterQos::durability_service`（`DurabilityService`）で、writer が消えた後に残す分を決める。`history`（既定 KEEP_LAST 1）、`resource_limits`（既定は無制限）、`service_cleanup_delay`（既定 0）。
- PERSISTENT のファイルの置き場所は、`ParticipantConfig::persistence_directory`、参加者の PROPERTY `rustorb.dds.persistence.directory`、環境変数 `RUSTORB_DDS_PERSISTENCE_DIR`、カレントディレクトリの `rustorb-dds-persistent` の順に決まる。
- Python は `Qos(durability="transient" | "persistent", service_history=..., service_max_samples=..., service_cleanup_delay=...)`。C ABI は `rdds_writer_qos` の `service_*`、C++ は `dds::core::policy::DurabilityService`、Java は `DurabilityService`（`PolicyFactory.DurabilityService()`）。置き場所はどれも参加者の PROPERTY で渡せる。

## しくみ

仕様は、TRANSIENT と PERSISTENT のトピックごとに「組み込みの durability 用の reader と writer」があるかのように振る舞う、と書く（2.2.3.4）。ここでは別の reader と writer は作らず、writer そのものを参加者に残す。

- TRANSIENT・PERSISTENT の writer は、生きている間は TRANSIENT_LOCAL と同じで、HISTORY と RESOURCE_LIMITS の分を ack の後も持ち、後から来た reader に送る。
- writer を削除しても、インスタンスの unregister・dispose はせず、RTPS の endpoint も消さない（SEDP の告知も残る）。参加者が writer を「みなしご」として持ち続け、HISTORY と RESOURCE_LIMITS を DURABILITY_SERVICE のものに切り替えて、超える分を古い順に捨てる（`max_instances` を超えたら、最後の変更がいちばん古いインスタンスから）。後から match した reader には、ふつうの writer と同じく HEARTBEAT と再送で届ける。
- 別の writer を作らないので、reader が同じサンプルを二度受け取ることはない。reader から見ると writer は生きたまま。AUTOMATIC の liveliness は参加者が主張し続ける。MANUAL の writer は誰も主張しないので、lease が切れれば reader からは NOT_ALIVE_NO_WRITERS に見える（それでもサンプルは届く）。
- みなしごの writer は、dispose されたインスタンスの変更を `service_cleanup_delay` の後に捨てる。条件の「生きた writer がいない」は削除で満たされるので、削除の時から数える。
- 参加者を削除すれば、みなしごの writer も消える。参加者より長く残すのは PERSISTENT だけ。
- DURABILITY_SERVICE は TRANSIENT・PERSISTENT の writer の SEDP に `PID_DURABILITY_SERVICE`（0x001e）として載せる。受け取った相手の値は読んで持つが、match には使わない（RxO ではない）。

## PERSISTENT のファイル

- PERSISTENT の writer は、作られるときにファイルを一つ取る。名前は `<トピック名>.<n>.rdds`（トピック名の英数字と `-`、`_` 以外は `_`）。ほかの writer が持っていない（ロックされていない）もののうち、見出しのトピック名と型名が同じ最初のものを取り、なければ新しく作る。ロックは `File::try_lock`（OS のファイルロック）なので、別のプログラムどうしでも同じファイルは取り合わない。このため MSRV は 1.89。
- 取ったファイルの変更（値の書き込み、dispose、unregister）を、元の source timestamp ごと writer の履歴に入れなおしてから（HISTORY の分だけ残る）、reader と match する。同じ参加者にすでにある TRANSIENT_LOCAL 以上の reader にもその場で渡す。
- 書くたびに変更を一つ追記する。履歴の何倍も（2 倍 + 64 を超えて）たまったら、履歴の分だけで書きなおす。取ったときも書きなおす。書き込みに失敗すると、その write は `Error::Io` で失敗する。
- 参加者を削除するとファイルを放す（ロックを外す）。次に同じ置き場所で作られた PERSISTENT の writer が取る。
- 形式（little endian）: 見出し `RDDSPST1`、u32 の長さとトピック名、u32 の長さと型名。続いて変更ごとに、u32 の残りの長さ、u8 の種類（0 値、1 dispose か unregister）、u8 のフラグ（1 dispose、2 unregister）、16 octet のインスタンスハンドル、u32 の秒、u32 の秒の端数、値の payload（種類 0 のとき）。途中で切れた変更（追記中の異常終了）はそこでファイルの終わりとみなす。
- payload は暗号化する前のもの（DDS Security の payload 保護があっても平文で書く）。`fsync` はしない。

## 制限

- 残すのはその参加者の writer のものだけ。ほかの参加者の writer のサンプルを受けて残す、独立した durability service はない。参加者ごと消えれば TRANSIENT のサンプルも消える。
- PERSISTENT のサンプルを再び送り出すのは、同じトピックと型の PERSISTENT writer が作られたとき。reader だけのプログラムがファイルから送ることはない。
- 再起動の後の writer は GUID が新しいので、前の writer から受け取っていた reader には同じサンプルがもう一度届く。
- 再起動の後の writer は、取りなおした値のインスタンスを登録しなおす（unregister されていたインスタンスの変更は残るが登録はされない）。

## 確かめたこと

- `crates/rustorb-dds/tests/durability.rs`: 削除の後に別の参加者と同じ参加者の遅れた reader が DURABILITY_SERVICE の分を受け取る、VOLATILE の reader は受け取らない、インスタンスが ALIVE のまま、`max_instances` と `service_cleanup_delay`、参加者を作りなおした後の PERSISTENT、二つの writer が別のファイルを取る。
- `crates/rustorb-dds/src/persistent.rs` の単体テスト: 追記、書きなおし、ロック、切れた変更、別トピックの同名ファイル。
- Python（`test_transient_and_persistent`）、C ABI（`persistent_writers_keep_files_where_the_property_says`）、C++（`transient_samples_outlive_the_writer`）、Java（`transientSamplesOutliveTheWriter`）。
- 相互接続（`interop/run.sh` の `rustorb-to-<peer>-transient`、`-persistent`）: rustorb-dds の writer が書いて削除され（PERSISTENT は参加者ごと削除して二つ目の参加者の writer がファイルから取りなおし）、2 秒遅れて作られた相手の TRANSIENT の reader（Fast DDS は TRANSIENT_LOCAL）が 50 個を順に受け取る。Cyclone DDS 0.10.4 と Fast DDS 2.11.2 はこのコンテナで通った。OpenDDS 3.31 は CI で確かめる。
- Fast DDS 2.11.2 の reader は、persistence plugin の設定なしには TRANSIENT で作れない（"Persistence GUID not specified"）。
