# C++ バインディング（cpp/）

rustorb/rustorb#111 の D4（#43）の 2 番目、#45 の設計をまとめる。ISO/IEC の C++ PSM（DDS-PSM-Cxx 1.0、`<dds/dds.hpp>`）の部分集合を、[C ABI](c-abi.md)（`rustorb/dds.h`）の上にヘッダだけで実装する。型は rustorb-idl が IDL から書く。

| 場所 | 内容 |
| --- | --- |
| `cpp/include/dds/dds.hpp` | PSM の入口。`dds/core/ddscore.hpp` など部分ごとの見出しも同じものを読む |
| `cpp/include/rustorb/dds/core.hpp` | 例外、`Duration`、`Time`、`InstanceHandle` |
| `cpp/include/rustorb/dds/types.hpp` | 生成コードが使う部品：型の組み立て、値の書き方（shape）、`codec<T>`、topic の型の trait |
| `cpp/include/rustorb/dds/policy.hpp` | QoS の方針、entity ごとの QoS、状態 |
| `cpp/include/rustorb/dds/entities.hpp` | entity、listener、サンプル、条件、WaitSet |
| `cpp/CMakeLists.txt` | cargo で `librustorb_dds_ffi.a` を作り、`rustorb_dds` ターゲットにまとめる。試験と例、インストール（`find_package(rustorb_dds)`） |
| `cpp/tests` | 型の試験（`test_types.cc`）、entity の試験（`test_entities.cc`）、生成コード（`generated/`） |
| `cpp/examples` | `hello_world`、`keyed`。Rust と Python の例と同じコマンドラインで、`interop/run.sh` が Cyclone DDS・Fast DDS と双方向に試す |

## 使い方

```sh
rustorb-idl -bc++11 -Wbdds_only HelloWorld.idl   # HelloWorld.hh と HelloWorld_dds.hh
cmake -S cpp -B build/cpp && cmake --build build/cpp && ctest --test-dir build/cpp
```

```cpp
#include "HelloWorld_dds.hh"
#include <dds/dds.hpp>

dds::domain::DomainParticipant dp(0);
dds::topic::Topic<HelloWorld::Greeting> topic(dp, "hello_world");
dds::pub::Publisher pub(dp);
dds::pub::qos::DataWriterQos qos = pub.default_datawriter_qos();
qos << dds::core::policy::Reliability::Reliable();
dds::pub::DataWriter<HelloWorld::Greeting> writer(pub, topic, qos);
writer << HelloWorld::Greeting(1, "Hello, world!");
```

CMake では `find_package(rustorb_dds)` と `target_link_libraries(app rustorb_dds::rustorb_dds)`。C++17 が要る。

## 生成コード（rustorb-idl）

- `-bc++11 -Wbdds_only` は `Foo.hh`（型と定数だけ。標準ライブラリしか使わない）と `Foo_dds.hh` を書く。GIOP にない注釈と型（`@optional` は `std::optional`、`@external` はふつうのメンバー、`@value`、`int8`、`bitmask`）も使える。上限付きの文字列と列は `std::string` と `std::vector` で、上限は書くときに確かめる
- `-bc++11 -Wbdds` は CORBA のコード（`Foo.hh`、`Foo.cc`。rustorb の C++ 実行時を使う）に加えて `Foo_dds.hh` を書く。同じ型を CORBA と DDS の両方で使える（OpenRTM-aist 向け）
- `Foo_dds.hh` は、struct・union・enum・bitmask ごとに `rustorb::dds::codec<T>` を特殊化する。`type()` が DDS-XTypes の型を組み立て（C ABI の `rdds_*_builder`）、`write` と `read` が値を型の順にストリームへ書き、読む。topic の型（`@nested` でない struct）には `dds::topic::is_topic_type<T>` と `dds::topic::topic_type_name<T>` もある
- 型は Rust と Python の生成コードと同じに記述するので、TypeObject は他の実装と一致する。`cpp/tests/test_types.cc` が Cyclone DDS の idlc の TypeInformation とハッシュを比べる

**shape**：C++ の型だけでは IDL の形がわからないことがある。たとえば `long m[2][3]`（2 次元の配列一つ）と、`typedef long Row[3]; Row m[2];`（`Row` の配列）は、どちらも `std::array<std::array<int32_t, 3>, 2>` だが、C ABI のストリームでは前者は 6 要素をまとめて、後者は 3 要素ずつ 2 回書く。そこで生成コードは各メンバーの書き方を `rustorb::dds::shape` の型（`prim<int32_t>`、`sequence<E>`、`array<E, 次元数>`、`optional<E>`、`type<T>`...）で明示する。

`cpp/tests/generated` は、Cyclone DDS の fixture（`crates/rustorb-xtypes/tests/fixtures/cyclonedds`）の `Generated.idl`、`HelloWorld.idl`、`Keyed.idl` と、`cpp/tests/idl/CppTypes.idl` に対する rustorb-idl の出力：

```sh
for f in crates/rustorb-xtypes/tests/fixtures/cyclonedds/{Generated,HelloWorld,Keyed}.idl \
         cpp/tests/idl/CppTypes.idl; do
  rustorb-idl -bc++11 -Wbdds_only -Wbextensibility=final -C cpp/tests/generated $f
done
```

## PSM のうち実装したもの

- `dds::domain::DomainParticipant`（`domain_id`、`assert_liveliness`、`current_time`、既定の QoS）
- `dds::topic::Topic<T>`（`name`、`type_name`、`inconsistent_topic_status`）
- `dds::pub::Publisher`、`dds::pub::DataWriter<T>`（`write`、`<<`、`register_instance`、`unregister_instance`、`dispose_instance`、`key_value`、`lookup_instance`、`wait_for_acknowledgments`、状態の取得、`assert_liveliness`、`listener`）
- `dds::sub::Subscriber`、`dds::sub::DataReader<T>`（`read`、`take`、`select()` の `instance`・`next_instance`・`state`・`max_samples`、`key_value`、`lookup_instance`、状態の取得、`listener`）、`Sample<T>`、`SampleInfo`、`LoanedSamples<T>`、`DataState`
- QoS：Reliability、History、Durability、Deadline、Liveliness、ResourceLimits、Partition、UserData、WriterDataLifecycle、DataRepresentation、TypeConsistencyEnforcement。`qos << policy`、`qos.policy<P>()`、`DataWriterQos(topic_qos)`
- 状態：`StatusMask` と各状態のクラス（PSM のつづり `offered_deadlined_missed_status`、`offered_incompatible_qos_staus` も）
- `DataWriterListener<T>`、`DataReaderListener<T>` と `NoOp` 版
- `GuardCondition`、`StatusCondition`、`ReadCondition`、`WaitSet`（`wait`、`dispatch` と条件の `handler`）
- `dds::topic::ContentFilteredTopic<T>`（`topic`、`filter_expression`、`filter_parameters` の取得と変更）と `dds::topic::Filter`。`DataReader` は `ContentFilteredTopic` からも作れる
- DDS-RPC の `dds::rpc::Requester<TReq, TRep>` と `dds::rpc::Replier<TReq, TRep>`（`rustorb/dds/rpc.hpp`、`RequesterParams`/`ReplierParams` でサービス名かトピック名と QoS を与える）と `dds::rpc::SampleIdentity`。[rpc.md](rpc.md)
- 例外：`ReturnCode_t` ごとの PSM の例外（`dds::core::TimeoutError` など）

entity は PSM どおり参照で、写しは同じ entity を指す。最後の参照がなくなるか `close()` で削除する。子は親を生かしておく。`dds::core::null` から作ったものは何も指さず、`== dds::core::null` が真になる。

## PSM と違うところ、rustorb の拡張

- `DataWriter::unregister_instance(handle)`、`dispose_instance(handle)`、`key_value(sample, handle)` は、その writer の `register_instance` か `lookup_instance` が返したハンドルだけを受ける（C ABI はキーの値で instance を指すため）。キーの値を渡す `unregister_instance(const T&)`、`dispose_instance(const T&)` もある
- `take` と `max_samples` は一緒に使えない（`UnsupportedError`）。`read` では使える
- `Selector::condition(read_condition)` で `ReadCondition` のサンプルを読む
- `rustorb::dds::localhost_participant(domain)` はループバックだけを使う参加者（試験用）。`wait_for_subscriptions`、`wait_for_publications`、`wait_for_data`、`discovered_participants` も足した
- listener は `DataWriter` と `DataReader` だけ（参加者・publisher・subscriber・topic の listener は渡すと `UnsupportedError`）。listener はライブラリのスレッドで呼ばれるか、同じ参加者の writer が書いたときはその `write` の中で呼ばれる。entity を作る間に起きた呼び出しは、作り終えてから行う
- `LoanedSamples` は写しで、貸し出しを返す操作は何もしない
- entity の QoS は作った後に変えられない（`UnsupportedError`）

## まだないもの

- `DATA_ON_READERS` と参加者・publisher・subscriber の listener（C ABI にまだない）
- builtin topic、`find`、`ignore`、content-filtered topic、query、coherent access
- IDL の struct の継承、`map`、再帰する型（rustorb-idl がまだ対応していない）
