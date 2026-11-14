# Java バインディング（java/）

rustorb/rustorb#111 の D4（#43）の 3 番目、#46 の設計をまとめる。OMG の Java 5 Language PSM for DDS（formal/2013-11-02、`org.omg.dds`）の部分集合を、[C ABI](c-abi.md)（`rustorb/dds.h`）の上に FFM API（`java.lang.foreign`、Java 22 以降）で実装する。JNI もネイティブのグルーコードも使わない。型は rustorb-idl が IDL から書く。

| 場所 | 内容 |
| --- | --- |
| `java/src/main/java/org/omg/dds` | PSM の API（インターフェースと抽象クラス）。仕様の規範ソース（omgdds_src.zip）の署名にそろえ、rustorb-dds が実装するものだけを置いた |
| `java/src/main/java/org/rustorb/dds` | 実装。`RustorbServiceEnvironment`（PSM の入口）、`Rustorb`（拡張）、生成コードが使う `Codec`・`Out`・`In`・`Types`・`TypeRef`、FFM の層（`Library`、`Ffi`） |
| `java/tools/gen-ffi.py` | `rustorb/dds.h` から `Ffi.java`（関数の MethodHandle）を書く。`--check` で最新か確かめる（CI） |
| `java/pom.xml` | Maven。cargo で `librustorb_dds_ffi`（共有ライブラリ）を作り、JUnit 5 で試験する |
| `java/src/test` | 型の試験（`TypesTest`）、entity の試験（`EntitiesTest`）、生成コード（`generated/`） |
| `java/examples` | `HelloWorldExample`、`KeyedExample`。Rust・Python・C++ の例と同じコマンドラインで、`interop/run.sh` が Cyclone DDS・Fast DDS と双方向に試す |

## 使い方

```sh
rustorb-idl -bjava -Wbdds_only HelloWorld.idl   # HelloWorld/Greeting.java と HelloWorld/GreetingCodec.java
mvn -f java/pom.xml verify
```

```java
ServiceEnvironment env = ServiceEnvironment.createInstance(getClass().getClassLoader());
PolicyFactory policy = PolicyFactory.getPolicyFactory(env);
DomainParticipant dp = env.getSPI().getParticipantFactory().createParticipant(0);
Topic<HelloWorld.Greeting> topic = dp.createTopic("hello_world", HelloWorld.Greeting.class);
Publisher pub = dp.createPublisher();
DataWriter<HelloWorld.Greeting> writer = pub.createDataWriter(topic,
        pub.getDefaultDataWriterQos().withPolicy(policy.Reliability().withReliable()));
writer.write(new HelloWorld.Greeting(1, "Hello, world!"));
```

実行には `--enable-native-access=ALL-UNNAMED` と、`librustorb_dds_ffi` の場所が要る。場所は、システムプロパティ `rustorb.dds.ffi.library`、環境変数 `RUSTORB_DDS_FFI_LIBRARY`、`java.library.path`、OS の動的リンカの順に探す。`org.omg.dds.serviceClassName` を設定しなければ `ServiceEnvironment.createInstance` は `org.rustorb.dds.RustorbServiceEnvironment` を使う。

## 生成コード（rustorb-idl）

- `-bjava -Wbdds_only` は、型のクラスと、型ごとの `<Name>Codec` を書く。CORBA は使わない（Helper と Holder もない）。GIOP にない注釈と型も使える。IDL4 to Java（formal/2023-03-01）にならって：
  - `enum` は Java の enum で、`getValue()` と `valueOf(int)` がある（`@value` も）
  - `@optional` は箱の型（`Integer` など）かクラスで、ないときは `null`。`@external` はふつうのメンバー
  - `bitmask` は `java.util.BitSet` と、ビットの位置（`getPosition()`）を持つ enum `<Name>Flags`
  - `int8` は `byte`
  - 新しい struct のメンバーは null でない：文字列は空、列は要素なし、配列はその長さ、union は最初のメンバー

  struct・union・配列はほかは IDL to Java の形（public のフィールド、列は Java の配列、`discriminator()`...）。上限付きの文字列と列は `String` と配列で、上限は書くときに確かめる
- `-bjava -Wbdds` は、CORBA のコード（IDL to Java の形、rustorb の Java 実行時を使う）に加えて `<Name>Codec` を書く。同じ型を CORBA と DDS の両方で使える（OpenRTM-aist 向け）
- `<Name>Codec.type()` が DDS-XTypes の型を組み立て（`org.rustorb.dds.Types`、C ABI の `rdds_*_builder`）、struct・union・enum の `<Name>Codec.INSTANCE` は `org.rustorb.dds.Codec` で、値を型の順にストリーム（`Out`、`In`）へ書き、読む。`TypeSupport.newTypeSupport(Name.class, env)` はクラスの隣の `<Name>Codec` を探す
- 型は Rust・Python・C++ の生成コードと同じに記述するので、TypeObject は他の実装と一致する。`TypesTest` が Cyclone DDS の idlc の TypeInformation とハッシュを比べる
- 多次元の基本型の配列は `Out.flatten` で一つの配列にまとめて書き、`In.unflatten` で戻す。`typedef long Row[3]; Row m[2];` のような typedef の配列は、C++ と同じく要素ごとに書く

`java/src/test/generated` は、Cyclone DDS の fixture（`crates/rustorb-xtypes/tests/fixtures/cyclonedds`）の `Generated.idl`、`HelloWorld.idl`、`Keyed.idl` と、`cpp/tests/idl/CppTypes.idl` に対する rustorb-idl の出力：

```sh
for f in crates/rustorb-xtypes/tests/fixtures/cyclonedds/{Generated,HelloWorld,Keyed}.idl \
         cpp/tests/idl/CppTypes.idl; do
  rustorb-idl -bjava -Wbdds_only -Wbextensibility=final -C java/src/test/generated $f
done
```

## FFM の層

- `Ffi.java` は `gen-ffi.py` が `dds.h` から書く。Java のソースが使う `rdds_` の関数だけを束ねる。ポインタは `long` で渡す
- ストリームの関数（`rdds_out_*`、`rdds_in_*`）はコールバックもブロックもしないので critical で呼ぶ。配列と文字列は Java の配列をそのまま（ヒープのセグメントで）渡す
- listener は upcall stub。`self` は表の番号で、entity を作り終える前に届いた呼び出しは作り終えてから渡す。`release` で表から外す
- ネイティブのハンドルは `Cleaner` でも手放すが、entity は `close()` で閉じる
- binary128（`long double`）は `double` に丸める（`Float128`）

## PSM のうち実装したもの

- `ServiceEnvironment`、`DomainParticipantFactory`、`DomainParticipant`（`getDomainId`、`assertLiveliness`、`getCurrentTime`、既定の QoS、`lookupTopicDescription`、`closeContainedEntities`）
- `Topic`（`getName`、`getTypeName`、`getInconsistentTopicStatus`）、`TypeSupport`
- `Publisher`、`DataWriter`（`write`、`registerInstance`、`unregisterInstance`、`dispose`、`getKeyValue`、`lookupInstance`、`waitForAcknowledgments`、状態の取得、`getMatchedSubscriptions`、`assertLiveliness`、listener）
- `Subscriber`、`DataReader`（`read`、`take`、`select()` の `instance`・`nextInstance`・`dataState`・`maxSamples`、`getKeyValue`、`lookupInstance`、状態の取得、`getMatchedPublications`、listener）、`Sample`、`Sample.Iterator`、`SampleState`・`ViewState`・`InstanceState`、`Subscriber.DataState`
- QoS：Reliability、History、Durability、Deadline、Liveliness、ResourceLimits、Partition、UserData、Property（参加者。`dds.sec.*` で DDS Security、`docs/security.md`）、WriterDataLifecycle、DataRepresentation。`withPolicy`、`withPolicies`、`get(Class)`、`PolicyFactory`
- 状態と event（`core.status`、`core.event`）、`DataWriterListener`・`DataReaderListener`・`PublisherListener`・`SubscriberListener`（`onDataOnReaders`）・`TopicListener`・`DomainParticipantListener` と Adapter。entity の listener が受けない状態は publisher・subscriber、次に参加者の listener へ（DDS 1.4 2.2.4.3）。`Subscriber.notifyDataReaders`
- `GuardCondition`、`StatusCondition`、`ReadCondition`、`WaitSet`
- `ContentFilteredTopic`（`DomainParticipant.createContentFilteredTopic`、`getFilterExpression`、`getExpressionParameters`、`setExpressionParameters`、`getRelatedTopic`）。`createDataReader` に渡せる
- DDS-RPC の `org.rustorb.dds.Requester`、`Replier`、`SampleIdentity`（rustorb の API。要求と応答のトピックから作る）。[rpc.md](rpc.md)
- 例外：`ReturnCode_t` ごとに PSM の表 7.1 の例外（`TimeoutException`、`PreconditionNotMetException`、`AlreadyClosedException` など）

## PSM と違うところ、rustorb の拡張

- 拡張は `org.rustorb.dds.Rustorb` に置く（`org.omg.dds` には足さない）：`localhostParticipant`（ループバックだけを使う参加者、試験用）、`waitForSubscriptions`、`waitForPublications`、`waitForData`、`discoveredParticipants`、`select(ReadCondition)`（条件のサンプルを読む Selector）、`XCDR2_DATA_REPRESENTATION`
- 型は IDL の名前で登録する。`TypeSupport.newTypeSupport` に別の名前を渡すと `UnsupportedOperationException`（C ABI は型の名前で登録するため）
- listener はライブラリのスレッドで呼ばれるか、同じ参加者の writer が書いたときはその `write` の中で呼ばれる
- 状態の条件は writer と reader のもの
- `DataWriter` の instance をハンドルで指す操作は、その writer の `registerInstance` か `lookupInstance` が返したハンドルだけを受ける（C++ と同じ）
- entity の QoS は作った後に変えられない（`setQos` は `UnsupportedOperationException`）
- `waitForHistoricalData` はすぐ返す
- 無効なサンプル（dispose などの知らせ）の `getData()` は PSM どおり `null`。キーは `getKeyValue(null, sample.getInstanceHandle())` で得る

## まだないもの

- builtin topic、`find`、`ignore`、content-filtered topic、query、coherent access、DynamicData
- IDL の struct の継承、`map`、再帰する型（rustorb-idl がまだ対応していない）
- Maven Central への公開。いまは `librustorb_dds_ffi` を自分で作る
