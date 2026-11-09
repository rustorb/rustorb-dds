# OpenRTM-aist 2.0 との接続

rustorb/rustorb#111 の D4（#43）の 5 番目、#48 をまとめる。OpenRTM-aist 2.0（C++）のデータポートは、Fast DDS の転送（FastRTPSTransport）でつなぐと DDS のトピックで値をやりとりする。rustorb-dds の writer と reader は、そのトピックに同じ型と QoS で入れば、OpenRTM のポートと両方向に通じる。OpenRTM-aist は黒箱として扱い、配布されたプログラムを rtc.conf で設定して動かし、discovery の内容を外から観察した（`interop/openrtm.sh`、CI の job `openrtm`）。

## OpenRTM-aist 側の設定

FastRTPSTransport（`-DFASTRTPS_ENABLE=ON` で build する）を読み込み、コネクタの `interface_type` を `fast-rtps` にする。`fast-rtps.topic` が DDS のトピック名になる（ないときは `chatter`）。

```
manager.modules.load_path: /usr/local/lib/openrtm-2.0/transport
manager.preload.modules: FastRTPSTransport.so
manager.components.preconnect: ConsoleIn0.out?interface_type=fast-rtps&fast-rtps.topic=chatter
```

DDS の domain は 0 になる（rtc.conf の `fast-rtps.domain.id` は効かなかった）。ネームサーバーも RT System Editor も要らない。`preconnect` はポートひとつのコネクタを作り、DDS のトピックに入る。

## DDS に出るもの（観察）

OpenRTM-aist 2.0.2、Fast DDS 2.6（ROS 2 Humble のもの）、Ubuntu 22.04 の ConsoleIn（OutPort `out`）と ConsoleOut（InPort `in`）の SEDP：

| | OutPort（writer） | InPort（reader） |
| --- | --- | --- |
| トピック | `fast-rtps.topic`（既定 `chatter`） | 同じ |
| 型の名前 | IDL の名前：`RTC::TimedLong` | 同じ |
| key | なし（entity kind が no key） | なし |
| RELIABILITY | BEST_EFFORT | BEST_EFFORT |
| DURABILITY | TRANSIENT_LOCAL（DURABILITY_SERVICE は KEEP_LAST 1） | VOLATILE |
| そのほか | 既定値。PARTITION なし、USER_DATA なし | 既定値。TYPE_CONSISTENCY を送る |
| TypeInformation | 送らない | 送らない |

- 値は型の CDR（XCDR1、`@final` と同じ）で、型の名前で一致を判定する。rustorb-idl で `-Wbextensibility=final` を付けて生成した型を同じ名前で登録すれば通じる。XCDR2 では書かない
- QoS は `fast-rtps.publisher.qos.*`、`fast-rtps.subscriber.qos.*`（reliability、durability、history など）で変えられる。既定は上の表
- OpenRTM のプロセスどうしは同じホストでは Fast DDS の共有メモリでも送るが、rustorb-dds とは UDP で通じる

## rustorb-dds の例

`openrtm` という例を Rust（`crates/rustorb-dds/examples/openrtm`）、C++（`cpp/examples/openrtm.cc`）、Python（`python/examples/openrtm.py`）で書いた。どれも同じコマンドラインで、`RTC::TimedLong` の OutPort か InPort のようにふるまう。QoS は OpenRTM のポートの既定（BEST_EFFORT、KEEP_LAST 1）。

```sh
cargo run --example openrtm -- in 0 10          # ConsoleIn の値を読む
cargo run --example openrtm -- out 0 10 200     # ConsoleOut に書く
```

型は rustorb-idl が OpenRTM-aist の BasicDataType.idl から生成する：

```sh
rustorb-idl -brust   -Wbdds_only -Wbextensibility=final BasicDataType.idl
rustorb-idl -bc++11  -Wbdds_only -Wbextensibility=final BasicDataType.idl
rustorb-idl -bpython -Wbdds_only -Wbextensibility=final BasicDataType.idl
```

例が使うのは、その中の `RTC::Time`、`RTC::TimedLong`、`RTC::TimedString` を同じに宣言した `interop/idl/OpenRTM.idl` の生成コードで（BasicDataType.idl から生成したものと同じ）、リポジトリに置いてある。Python の例は、BasicDataType.idl から生成した `BasicDataType_dds.py` を import できればそれを使い、CI はそれで試す。

## 試験

`interop/openrtm.sh` は、Rust・C++・Python の例それぞれについて、OpenRTM の ConsoleIn から例の `in` へ、例の `out` から OpenRTM の ConsoleOut へ送り、読む側が `COUNT` 個（既定 5）受け取ることを確かめる。BEST_EFFORT なので、書く側は 4 倍送る。CI の job `openrtm` は、OpenRTM の開発用イメージ `openrtm/devel-rtm:ubuntu22.04` で OpenRTM-aist 2.0.2 をタグから build して、この試験を回す。2.0.2 は Fast CDR 2（Ubuntu 24.04 の ROS 2 Jazzy の Fast DDS 2.14）では build できないので、Ubuntu 22.04 を使う。

## まだないもの

- CORBA の `corba_cdr` 転送（ネームサーバーと CORBA のポート）とつなぐこと。rustorb（ORB）の側の仕事
- OpenRTM-aist 2.0 の Python 版（OpenRTM-aist-Python）、OpenSplice 転送、ROS 2 転送（`ros2:` の marshaling_type）での試験
