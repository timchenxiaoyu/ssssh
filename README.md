

老版本 opsd中的 ssh 是通过 go 写的，为了节省内存开销

新版本通过rust 实现ssh 协议


代码编译
```
 cargo build  --target=x86_64-unknown-linux-musl  --release
```