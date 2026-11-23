# Ruby バインディング（ruby/）

D7（#90）の #92 の設計をまとめる。OMG に Ruby の DDS PSM はないので、C++ PSM（ISO/IEC C++ 2003 Language DDS PSM、[cpp.md](cpp.md)）の構造を Ruby の書き方（snake_case）に移し、[C ABI](c-abi.md)（`rustorb/dds.h`）の上の C 拡張として実装する。形は rustorb 0.7.0 の Ruby バインディング（gem の `rustorb`）にそろえた：`librustorb_dds_ffi.a` を静的にリンクする C 拡張、`extconf.rb` は環境変数で場所を変えられる、試験は minitest。

| 場所 | 内容 |
| --- | --- |
| `ruby/lib/rustorb/dds.rb` ほか | gem の `rustorb-dds`（`require "rustorb/dds"`）。モジュール `DDS`：`DDS::Core`（エラー、`Duration`、`Time`、`InstanceHandle`、状態、`Entity`、条件と WaitSet）、`DDS::Domain`、`DDS::Topic`、`DDS::Pub`、`DDS::Sub`、`DDS::Types`（生成コードが使う型の記述と値の読み書き） |
| `ruby/ext/rustorb_dds_ext` | C 拡張 `rustorb_dds_ext`（`DDS::Native`）。`extconf.rb` が `librustorb_dds_ffi.a` を探す |
| `ruby/test` | minitest。型の試験（`test_types.rb`）、entity の試験（`test_entities.rb`）、生成コード（`generated/`） |
| `ruby/examples` | `hello_world.rb`、`keyed.rb`。Rust・Python・C++・Java・.NET の例と同じコマンドラインで、`interop/run.sh` が Cyclone DDS・Fast DDS・OpenDDS と双方向に試す |

## 使い方

```sh
rustorb-idl -bruby -Wbdds_only HelloWorld.idl   # HelloWorld_dds.rb
cd ruby && rake                                 # librustorb_dds_ffi と拡張をビルドして試験
```

```ruby
require "rustorb/dds"
require_relative "HelloWorld_dds"

dp = DDS::Domain::DomainParticipant.new(0)
topic = DDS::Topic::Topic.new(dp, "hello_world", HelloWorld::Greeting)
qos = DDS::Pub::DataWriterQos.new << DDS::Core::Policy::Reliability.reliable
writer = DDS::Pub::DataWriter.new(DDS::Pub::Publisher.new(dp), topic, qos)
writer.write(HelloWorld::Greeting.new(1, "Hello, world!"))

reader = DDS::Sub::DataReader.new(DDS::Sub::Subscriber.new(dp), topic)
reader.take.each { |s| puts s.data.message if s.valid? }
dp.close
```

`rake compile` は `cargo build --release -p rustorb-dds-ffi` をしてから拡張を `ruby/lib/rustorb/dds/` にビルドする（`CARGO_SKIP=1` で cargo を省く）。`extconf.rb` は、環境変数 `RUSTORB_DDS_FFI_LIBRARY`（`.a`）と `RUSTORB_DDS_FFI_INCLUDE`、gem に入れた `vendor/<os>-<cpu>/librustorb_dds_ffi.a`（#94）、このチェックアウトの `target/release` の順に探す。読み込んだときに `rdds_abi_version()` が合わなければエラー。

## 生成コード（rustorb-idl）

- `-bruby -Wbdds_only` は `Foo_dds.rb` に、型と定数と、型ごとの `self._dds_type` を書く。CORBA は使わない。GIOP にない注釈と型も使える：
  - struct は `DDS::Struct` の子クラス。`MEMBERS`（IDL の順）、メンバーの `attr_accessor`、`DEFAULTS`（新しい値のメンバー）。`new` は位置引数（省いたものは既定値）、`==` と `hash` は全メンバー
  - union は `DDS::Union` の子クラス。`_disc`、`_member`（選ばれているメンバーの名前）、メンバーごとのアクセサ（設定すると最初のラベルを選ぶ）、`_put(name, value, disc)`、`_default`。選ばれていないメンバーを読むと `DDS::Core::PreconditionNotMetError`
  - 列と配列は Array（多次元は入れ子）。`octet` の列は String（ASCII-8BIT）でも書ける
  - `enum` は `@value` の定数のモジュール（`ENUMERATORS`）。`bitmask` はビット値の定数のモジュール（`BIT_BOUND`）
  - `@optional` は nil で無い。`@external` はふつうのメンバー
  - `char` は 1 文字の String（ISO-8859-1）、`wchar` は Integer（コードポイント）、`wstring` は UTF-16 の符号単位の Array（String で書くこともできる）、`long double` は `DDS::LongDouble`（binary128）
- `-bruby -Wbdds` は、rustorb の `-bruby` のクラスを開き直して `_dds_type` だけを足す。同じ型を CORBA と DDS の両方で使える
- `_dds_type` は型の記述（`DDS::Types.struct_type` など）を返し、最初に使うときに C ABI の `rdds_*_builder` で型を組み立てる。値は記述に沿ってストリーム（`DDS::Native::Out`、`In`）へ書き、読む。プリミティブの列と配列は 1 回の呼び出しでまとめて書く
- 型は Rust・Python・C++・Java・.NET の生成コードと同じに記述するので、TypeObject は他の実装と一致する。`test_types.rb` が Cyclone DDS の idlc の TypeInformation とハッシュを比べる

`ruby/test/generated` は、Cyclone DDS の fixture（`crates/rustorb-xtypes/tests/fixtures/cyclonedds`）の `Generated.idl`、`HelloWorld.idl`、`Keyed.idl` と、`cpp/tests/idl/CppTypes.idl` に対する rustorb-idl の出力で、`rake gen` が書く（`RUSTORB_IDL` で rustorb-idl のコマンドを変えられる）。

## C 拡張

- ハンドルは 1 つの TypedData クラスで、種類（型、参加者、topic、publisher、writer...）を持つ。GC が解放したハンドルは、拡張のスレッド（reaper）が GVL の外で `rdds_*_release` する。entity は `close` で閉じ、閉じたものを使うと `DDS::Core::AlreadyClosedError`
- 待つ呼び出し（`wait_for_*`、`WaitSet#wait`、書き込み、作成と削除）は GVL を外す（`rb_thread_call_without_gvl`）
- listener の表の `self` は Ruby 側の listener の番号。ライブラリのスレッドからの呼び出しは、そのスレッドが Ruby のスレッドならそのまま、そうでなければ拡張の dispatcher スレッド（Ruby のスレッド）に渡して終わるまで待つ。listener の例外はライブラリへ伝えず、`warn` に書く。entity を作り終える前に届いた呼び出しは、作り終えてから渡す（publisher・subscriber・参加者の listener に届く writer・reader の状態も、GUID が分かるまで待たせる）
- エラーは `rdds_status` から例外：`DDS::Core::TimeoutError`、`InvalidArgumentError`（値が型に合わないときも）、`UnsupportedError`、`AlreadyClosedError`、`PreconditionNotMetError` など（どれも `DDS::Core::Error`）。理由は `rdds_last_error_message()`

## 実装したもの

- `DomainParticipant`（`domain_id`、`assert_liveliness`、`current_time`、`find_topic`、`lookup_topicdescription`、`delete_contained_entities`、`default_*_qos`、listener）
- `Topic`（`name`、`type_name`、`inconsistent_topic_status`、listener）、`ContentFilteredTopic`（`filter_expression`、`expression_parameters` は設定もできる、`related_topic`）
- `Publisher`、`DataWriter`（`write`・`<<`、`register_instance`、`unregister_instance`、`dispose_instance`、`lookup_instance`、`wait_for_acknowledgments`、状態の取得、`matched_subscriptions`、`assert_liveliness`、`status_changes`、`status_condition`、listener）
- `Subscriber`（`notify_datareaders`、listener の `on_data_on_readers`）、`DataReader`（`read`・`take` と `read_data`・`take_data`。キーワード引数 `max_samples`、`sample_state`、`view_state`、`instance_state`、`instance`、`next_instance`、`condition`。`key_value`、`lookup_instance`、状態の取得、`matched_publications`、listener）、`Sample`（`data`、`info`、`valid?`）、`SampleInfo`
- QoS は policy を `<<` で足していく（`DDS::Core::Policy::Reliability.reliable` など。policy は凍結した値）：Reliability、History、ResourceLimits、Durability、Deadline、Liveliness、UserData、DataRepresentation、WriterDataLifecycle、DurabilityService（[durability.md](durability.md)）、TypeConsistencyEnforcement、Partition（publisher・subscriber）、Property（参加者。`dds.sec.*` で DDS Security、[security.md](security.md)）
- listener はモジュール（`DDS::Pub::DataWriterListener`、`DDS::Sub::DataReaderListener`、`PublisherListener`、`SubscriberListener`、`DDS::Topic::TopicListener`、`DDS::Domain::DomainParticipantListener`）で、メソッドは空の既定実装を持つ（`include` して要るものだけ書く）。entity の listener が受けない状態は publisher・subscriber、次に参加者の listener へ（DDS 1.4 2.2.4.3）
- `GuardCondition`、`StatusCondition`、`DDS::Sub::Cond::ReadCondition.new(reader, sample_state: ...)`、`WaitSet`（`wait` は条件の一覧を返し、時間切れは `TimeoutError`。`dispatch` は条件の handler を呼ぶ）
- `Duration` と `Time`。時間を取る引数は秒の数値も受ける

## C++ PSM と違うところ、rustorb の拡張

- 名前は Ruby の慣習（snake_case、述語は `?`）。entity は `new` で作る（`DataWriter.new(publisher, topic, qos)`）
- 拡張：`localhost: true`（ループバックだけを使う参加者、試験用）、`wait_for_subscriptions`、`wait_for_publications`、`wait_for_data`、`discovered_participants`、`DDS::Types.serialize`・`deserialize`
- 型は IDL の名前で登録する（C ABI は型の名前で登録するため）
- entity の QoS は作った後に変えられない
- 無効なサンプル（dispose などの知らせ）は `valid?` が false で、`data` は使わない。キーは `key_value(sample.info.instance_handle)`（instance が残っていれば）

## まだないもの

- DDS-RPC（Requester / Replier）、loan、DynamicData の公開 API
- builtin topic、`ignore`、query、coherent access
- IDL の struct の継承、mutable な union、`map`、再帰する型、bitset（rustorb-idl の Ruby 出力がまだ対応していない）
- Windows（rustorb の gem にあわせて Linux と macOS）
- RubyGems への公開（#94）。いまは `librustorb_dds_ffi` を自分で作る
