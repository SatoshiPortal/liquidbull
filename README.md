# liquidbull

Core library for liquid wallet used in BullBitcoin mobile app.


## test

Currently requires electrs and elements binaries and runs in regtest. 

Future updates will use testnet.

```bash
PROJECT_DIR=$PWD
mkdir -p server
cd ..
git clone https://github.com/Blockstream/electrs
cd electrs
git checkout new-index

cargo clean
cargo update
cargo install --debug --root $PROJECT_DIR/server/electrs_liquid --locked --path . --features liquid

cd $PROJECT_DIR/server
curl -L https://github.com/ElementsProject/elements/releases/download/elements-0.18.1.8/elements-0.18.1.8-x86_64-linux-gnu.tar.gz | tar -xvz elements-0.18.1.8/bin/elementsd
cd $PROJECT_DIR

export ELECTRS_LIQUID_EXEC=$PWD/server/electrs_liquid/bin/electrs
export ELEMENTSD_EXEC=$PWD/server/elements-0.18.1.8/bin/elementsd
```

Now you can run the tests: (may take a while since we are running electrs and elementsd binaries)

```bash
cargo test
```

If you get errors compiling librocksdb-sys its related to https://github.com/rust-rocksdb/rust-rocksdb/issues/713

To fix on Arch Linux, you need to downgrade the following pacakges to the following versions:

- gcc 13 to 12
- clang 16 to 15
- llvm 16 to 15

```bash

sudo pacman -U https://archive.archlinux.org/packages/l/llvm/llvm-15.0.7-3-x86_64.pkg.tar.zst
sudo pacman -U https://archive.archlinux.org/packages/l/llvm-libs/llvm-libs-15.0.7-3-x86_64.pkg.tar.zst 
sudo pacman -U https://archive.archlinux.org/packages/c/clang/clang-15.0.7-3-x86_64.pkg.tar.zst
sudo pacman -U https://archive.archlinux.org/packages/c/clang/clang-15.0.7-3-x86_64.pkg.tar.zst 
```

Note: Since Arch is a rolling release, everytime you update your system, these packages will get updated. Either keep running these commands when working on this repo, or add these packages to `IgnorePkg` line in `/etc/pacman.conf`.
