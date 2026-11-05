# rustorb-dds

Rust 製の DDS（Data Distribution Service）実装です。[rustorb](https://github.com/rustorb/rustorb) プロジェクトの一部です（[English](README.md)）。

**現状: 初期段階です。** 参加者どうしが SPDP と SEDP で互いを見つけ、reliable または best-effort の writer と reader で UDPv4 の上でサンプルをやり取りできます。キー付きのインスタンス、データグラムに収まらない大きなサンプル（DATA_FRAG）、TRANSIENT_LOCAL の durability、パーティション、LIVELINESS と DEADLINE、通信状態の WaitSet・Condition・listener にも対応しています。Cyclone DDS、Fast DDS と hello world が両方向で通ります。reliable、transient-local、liveliness でも通ります。ROS 2 のデモの talker・listener とも `std_msgs/msg/String` を両方向にやり取りできます（[interop](interop/)）。計画は [rustorb/rustorb#111](https://github.com/rustorb/rustorb/issues/111) にあります。

hello world は、2 つの端末で次を実行すると試せます。

```sh
cargo run --example hello_world -- sub
cargo run --example hello_world -- pub
```

Python からは（`pip install ./python`、[python/](python/README.md) を参照）:

```sh
python python/examples/hello_world.py sub
python python/examples/hello_world.py pub
```

## クレート構成

| クレート | 役割 | ライセンス |
|---|---|---|
| `rustorb-dds` | DCPS API（DDS 1.4） | MIT OR Apache-2.0 |
| `rustorb-rtps` | 通信路（DDSI-RTPS 2.5）: メッセージ、ディスカバリ、writer と reader | MIT OR Apache-2.0 |
| `rustorb-xtypes` | 型系（DDS-XTypes 1.3）: TypeObject、型の互換判定、DynamicData | MIT OR Apache-2.0 |
| `rustorb-dds-python` | Python パッケージ `rustorb_dds`（[python/](python/)）のネイティブモジュール。PyO3 を使う。crates.io には公開しない | MIT OR Apache-2.0 |
| `rustorb-dds-ffi` | C++ と Java のバインディングの土台になる C ABI（[`include/rustorb/dds.h`](crates/rustorb-dds-ffi/include/rustorb/dds.h)、[docs/c-abi.md](docs/c-abi.md)）。crates.io には公開しない | MIT OR Apache-2.0 |

XCDR1/XCDR2 の符号化は `rustorb-cdr` に、DDS の型サポートの trait は `rustorb-typesupport` にあります。どちらも rustorb リポジトリにあり、ORB と共有しています。IDL の生成器は [rustorb-idl](https://github.com/rustorb/rustorb-idl) です。DDS のクレートが依存するのは `rustorb-cdr` と `rustorb-typesupport` だけです。生成器は GPL-3.0-or-later ですが、生成されたコードはこのライセンスの対象外です。

## 段階

1. **D0**（rustorb 側）: `rustorb-cdr` の XCDR1/XCDR2、`rustorb-idl` による DDS の型サポートの出力
2. **D1**: RTPS メッセージ、SPDP/SEDP、best-effort の writer/reader。Cyclone DDS・Fast DDS と hello world を双方向で通す
3. **D2**: reliable、DATA_FRAG、キー付きインスタンス、History/Durability/Liveliness/Deadline、WaitSet。ROS 2 の talker/listener と接続
4. **D3**: TypeObject、型の互換判定、TypeLookup、DynamicData、Python バインディング
5. **D4**: C++/Java、OpenRTM-aist 2.0、TypeCode ⇄ TypeObject の橋渡し
6. **後回し**: DDS Security、共有メモリ、Content Filtered Topic、DDS-RPC

## clean-room

OMG の仕様（DDS 1.4、DDSI-RTPS 2.5、DDS-XTypes 1.3、DDS Security、IDL4）と、他実装との黒箱の相互接続試験だけを根拠に実装します。他の DDS 実装のソースコードは読まず、写しません。詳しくは [CONTRIBUTING.md](CONTRIBUTING.md) を見てください。

## ライセンス

MIT OR Apache-2.0 のどちらかを選べます（[LICENSE-MIT](LICENSE-MIT)、[LICENSE-APACHE](LICENSE-APACHE)）。
