$env:RUSTUP_HOME = '<TOOLS>\rust\rustup'
$env:CARGO_HOME = '<TOOLS>\rust\cargo'
$env:PATH = '<TOOLS>\rust\cargo\bin;<TOOLS>\mingw64\bin;' + $env:PATH
Set-Location '<ROOT>\rust-widget'
& cargo build --release *> '<ROOT>\rust-widget\out\build.log'
"EXIT=$LASTEXITCODE" | Out-File -Append -Encoding utf8 '<ROOT>\rust-widget\out\build.log'
