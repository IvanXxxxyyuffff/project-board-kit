@echo off
set RUSTUP_HOME=<TOOLS>\rust\rustup
set CARGO_HOME=<TOOLS>\rust\cargo
set PATH=<TOOLS>\rust\cargo\bin;<TOOLS>\mingw64\bin;%PATH%
cd /d <ROOT>\rust-widget
cargo build --release > out\build.log 2>&1
echo EXIT=%ERRORLEVEL% >> out\build.log
