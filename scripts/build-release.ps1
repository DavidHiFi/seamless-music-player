# Builds the release exe through the load gate, log beside this script.
$ErrorActionPreference = 'Continue'
Set-Location (Split-Path $PSScriptRoot -Parent)
python "H:\System Configuration\agent-tools\LoadGate\heavy.py" run --wait 6600 --reason "smp release build" -- cargo build --release *> "$PSScriptRoot\build-release.log"
