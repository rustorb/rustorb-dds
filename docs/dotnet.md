# .NET バインディング（dotnet/）

D7（#90）の #93 の設計をまとめる。OMG に .NET の DDS PSM はないので、Java 5 PSM（`org.omg.dds`、[java.md](java.md)）の構造を C# の書き方に移し、[C ABI](c-abi.md)（`rustorb/dds.h`）の上に P/Invoke で実装する。Rust 側に .NET 用のコードはない。形は rustorb 0.7.0 の .NET バインディング（NuGet の `Rustorb`、アセンブリ `Rustorb.Corba`）にそろえた。

| 場所 | 内容 |
| --- | --- |
| `dotnet/src/Rustorb.Dds` | NuGet の `Rustorb.Dds`（.NET 8）。`Omg.Dds.*`（API）、`Rustorb.Dds`（生成コードが使う `ICodec<T>`・`Out`・`In`・`Types`・`TypeSupport`、拡張）、`Rustorb.Dds.Native`（P/Invoke の層） |
| `dotnet/tools/gen-pinvoke.py` | `rustorb/dds.h` から `Native/Ffi.g.cs`（`LibraryImport` の宣言）を書く。`--check` で最新か確かめる（CI） |
| `dotnet/tests/Rustorb.Dds.Tests` | xUnit。型の試験（`TypesTests`）、entity の試験（`EntitiesTests`）、生成コード（`generated/`）。ビルドが cargo で `librustorb_dds_ffi` を作る（`-p:CargoSkip=true` で省く） |
| `dotnet/examples` | `HelloWorld`、`Keyed`。Rust・Python・C++・Java の例と同じコマンドラインで、`interop/run.sh` が Cyclone DDS・Fast DDS・OpenDDS と双方向に試す |

## 使い方

```sh
rustorb-idl -bcsharp -Wbdds_only HelloWorld.idl   # HelloWorld.cs と HelloWorld_dds.cs
dotnet test dotnet/tests/Rustorb.Dds.Tests
```

```csharp
using DomainParticipant dp = new DomainParticipant(0);
Topic<HelloWorld.Greeting> topic = dp.CreateTopic<HelloWorld.Greeting>("hello_world");
DataWriter<HelloWorld.Greeting> writer = dp.CreatePublisher().CreateDataWriter(topic,
    DataWriterQos.Default with { Reliability = Reliability.Reliable() });
writer.Write(new HelloWorld.Greeting(1, "Hello, world!"));

DataReader<HelloWorld.Greeting> reader = dp.CreateSubscriber().CreateDataReader(topic);
foreach (Sample<HelloWorld.Greeting> s in reader.Take())
{
    if (s.Info.ValidData) Console.WriteLine(s.Data!.message);
}
```

`librustorb_dds_ffi` は、環境変数 `RUSTORB_DDS_FFI_LIBRARY`、NuGet の `runtimes/<rid>/native`（#91）、アプリの隣、OS の動的リンカの順に探す（`NativeLibrary.SetDllImportResolver`）。読み込んだライブラリの `rdds_abi_version()` が合わなければ `DdsException`。

## 生成コード（rustorb-idl）

- `-bcsharp -Wbdds_only` は、型のクラス（`Foo.cs`）と、型ごとの `<Name>Codec`（`Foo_dds.cs`）を書く。CORBA は使わない（Helper もない）。GIOP にない注釈と型も使える。型のクラスは rustorb の `-bcsharp`（IDL4 to C#）と同じ形：
  - struct はプロパティを持つクラスで、全メンバーのコンストラクタ、コピーコンストラクタ、`Equals`・`GetHashCode` がある。新しい値のメンバーは null でない
  - 列は `Omg.Types.ISequence<T>`（取得だけのプロパティ）、配列は C# の配列（多次元はジャグ配列）
  - union は `Discriminator`、メンバーのプロパティ（設定すると最初のラベルを選ぶ）、`Set<member>(value, discriminator)`、`_default()`。選ばれていないメンバーを読むと `InvalidOperationException`
  - `enum` は C# の enum（`@value` の値）。`bitmask` は `[Flags]` の enum
  - `@optional` は null 許容（`int?`、クラス）。`@external` はふつうのメンバー
  - `int8` は `sbyte`、`long double` は `decimal`（binary128 との変換は `Float128`）
- `-bcsharp -Wbdds` は、CORBA のコード（rustorb の `Rustorb` パッケージを使う）に加えて `Foo_dds.cs` を書く。同じ型を CORBA と DDS の両方で使える
- `<Name>Codec.DdsType()` が DDS-XTypes の型を組み立て（`Rustorb.Dds.Types`、C ABI の `rdds_*_builder`）、`<Name>Codec.Instance`（`ICodec<T>`）が値を型の順にストリーム（`Out`、`In`）へ書き、読む。ファイルごとの `ModuleInitializer` が struct と union の Codec を `TypeSupport` に登録するので、`CreateTopic<T>` は型だけで済む
- 型は Rust・Python・C++・Java の生成コードと同じに記述するので、TypeObject は他の実装と一致する。`TypesTests` が Cyclone DDS の idlc の TypeInformation とハッシュを比べる
- 列を使う型の生成コードは `Omg.Types.ISequence<T>` と `Sequence<T>` を使う。いまはこれを rustorb の `Rustorb` パッケージ（0.7.0）から取る（試験のプロジェクトもそうしている）。CORBA の要らない DDS の利用者のために、rustorb 側で `Omg.Types` だけの小さなパッケージに分ける予定で（2026-11-23 に決定）、出たらそちらに替える

`dotnet/tests/Rustorb.Dds.Tests/generated` は、Cyclone DDS の fixture（`crates/rustorb-xtypes/tests/fixtures/cyclonedds`）の `Generated.idl`、`HelloWorld.idl`、`Keyed.idl` と、`cpp/tests/idl/CppTypes.idl` に対する rustorb-idl の出力：

```sh
for f in crates/rustorb-xtypes/tests/fixtures/cyclonedds/{Generated,HelloWorld,Keyed}.idl \
         cpp/tests/idl/CppTypes.idl; do
  rustorb-idl -bcsharp -Wbdds_only -Wbextensibility=final -C dotnet/tests/Rustorb.Dds.Tests/generated $f
done
```

## P/Invoke の層

- `Ffi.g.cs` は `gen-pinvoke.py` が `dds.h` から書く。`LibraryImport`（ソース生成）で、ハンドルとポインタは `nint`。ストリームの関数（`rdds_out_*`、`rdds_in_*`）はコールバックもブロックもしないので `SuppressGCTransition` を付ける
- ハンドルは種類ごとの `SafeHandle` で、`rdds_*_release` を呼ぶ。entity は `IDisposable`。閉じたものを使うと `AlreadyClosedException`
- listener の表は `UnmanagedCallersOnly` の関数ポインタで、`self` は listener の状態の `GCHandle`。`release` で解く。entity を作り終える前に届いた呼び出しは、作り終えてから渡す（publisher・subscriber・参加者の listener に届く writer・reader の状態も、GUID が分かるまで待たせる）。listener の例外はライブラリへ伝えず、`Trace` に書く
- エラーは `rdds_status` から例外：`TIMEOUT` は `TimeoutException`、`BAD_PARAMETER` は `ArgumentException`、`UNSUPPORTED` は `NotSupportedException`、`ALREADY_DELETED` は `AlreadyClosedException`、ほかは `DdsException` の派生（`PreconditionNotMetException` など）。理由は `rdds_last_error_message()`

## 実装したもの

- `DomainParticipant`（`DomainId`、`AssertLiveliness`、`CurrentTime`、`LookupTopicDescription`、`DeleteContainedEntities`、listener）
- `Topic<T>`（`Name`、`TypeName`、`GetInconsistentTopicStatus`、listener）、`ContentFilteredTopic<T>`（`FilterExpression`、`ExpressionParameters` は設定もできる、`RelatedTopic`）
- `Publisher`、`DataWriter<T>`（`Write`、`RegisterInstance`、`UnregisterInstance`、`DisposeInstance`、`LookupInstance`、`WaitForAcknowledgments`、状態の取得、`GetMatchedSubscriptions`、`AssertLiveliness`、`StatusChanges`、`StatusCondition`、listener）
- `Subscriber`（`NotifyDataReaders`、listener の `OnDataOnReaders`）、`DataReader<T>`（`Read`・`Take` と、その `Instance`・`NextInstance`・`ReadCondition` 版、`maxSamples` と状態の絞り込み、`GetKeyValue`、`LookupInstance`、状態の取得、`GetMatchedPublications`、`CreateReadCondition`、listener）、`Sample<T>`（`Data`、`Info`）、`SampleInfo`
- QoS は不変の record で、既定値（`DataWriterQos.Default` など、C ABI の既定）から `with` で変える：Reliability、History、ResourceLimits、Durability、Deadline、Liveliness、UserData、DataRepresentation、WriterDataLifecycle、DurabilityService（[durability.md](durability.md)）、TypeConsistencyEnforcement、Partition（publisher・subscriber）、Property（参加者。`dds.sec.*` で DDS Security、[security.md](security.md)）
- listener はインターフェース（`IDataWriterListener<T>`、`IDataReaderListener<T>`、`IPublisherListener`、`ISubscriberListener`、`ITopicListener`、`IDomainParticipantListener`）で、メソッドは空の既定実装を持つ（要るものだけ書く）。entity の listener が受けない状態は publisher・subscriber、次に参加者の listener へ（DDS 1.4 2.2.4.3）
- `GuardCondition`、`StatusCondition`、`ReadCondition`、`WaitSet`（`Wait` は条件の一覧を返し、時間切れは `TimeoutException`）
- `Duration` と `Time`。`Duration` は `TimeSpan` からも作れる（暗黙の変換）

## Java PSM と違うところ、rustorb の拡張

- 名前は C# の慣習：メソッドは PascalCase、get/set はプロパティ、状態の集合は `[Flags]` の enum（`StatusKind`、`SampleState`...）
- QoS の変更は `withPolicy` ではなく record の `with`。PolicyFactory はない
- listener は Adapter ではなく既定実装のあるインターフェース
- 拡張：`Participants.Localhost`（ループバックだけを使う参加者、試験用）、`WaitForSubscriptions`、`WaitForPublications`、`WaitForData`、`DiscoveredParticipants`、`TypeSupport.Serialize`・`Deserialize`
- 型は IDL の名前で登録する（C ABI は型の名前で登録するため）
- listener はライブラリのスレッドで呼ばれるか、同じ参加者の writer が書いたときはその `Write` の中で呼ばれる
- entity の QoS は作った後に変えられない
- 無効なサンプル（dispose などの知らせ）は `Info.ValidData` が false で、`Data` は使わない。キーは `GetKeyValue(sample.Info.InstanceHandle)`（instance が残っていれば）

## まだないもの

- DDS-RPC（Requester / Replier）、loan、DynamicData の公開 API
- builtin topic、`find`、`ignore`、query、coherent access
- IDL の struct の継承、`map`、再帰する型、bitset（rustorb-idl の C# 出力がまだ対応していない）
- NuGet への公開（#91、#94）。いまは `librustorb_dds_ffi` を自分で作る
