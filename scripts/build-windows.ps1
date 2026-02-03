# Build script for HumanSSH on Windows
# Usage: .\scripts\build-windows.ps1

$ErrorActionPreference = "Stop"

$APP_NAME = "HumanSSH"
$BINARY_NAME = "humanssh"

# Get version from Cargo.toml
$VERSION = (Get-Content Cargo.toml | Select-String '^version\s*=' | Select-Object -First 1) -replace '.*"(.*)".*', '$1'

Write-Host "Building HumanSSH v$VERSION..." -ForegroundColor Cyan

# Build release binary
Write-Host "==> Building release binary..." -ForegroundColor Yellow
cargo build --release
if ($LASTEXITCODE -ne 0) {
    Write-Host "Build failed!" -ForegroundColor Red
    exit 1
}

# Verify binary exists
$BINARY_PATH = "target\release\$BINARY_NAME.exe"
if (-not (Test-Path $BINARY_PATH)) {
    Write-Host "Binary not found at $BINARY_PATH" -ForegroundColor Red
    exit 1
}

Write-Host "==> Binary built successfully: $BINARY_PATH" -ForegroundColor Green

# Create distribution directory
$DIST_DIR = "target\release\$APP_NAME-$VERSION-windows"
Write-Host "==> Creating distribution package..." -ForegroundColor Yellow

if (Test-Path $DIST_DIR) {
    Remove-Item -Recurse -Force $DIST_DIR
}
New-Item -ItemType Directory -Path $DIST_DIR | Out-Null

# Copy binary
Copy-Item $BINARY_PATH "$DIST_DIR\"

# Copy themes if they exist
if (Test-Path "themes") {
    Copy-Item -Recurse "themes" "$DIST_DIR\"
    Write-Host "    Copied themes/" -ForegroundColor Gray
}

# Copy README
if (Test-Path "README.md") {
    Copy-Item "README.md" "$DIST_DIR\"
    Write-Host "    Copied README.md" -ForegroundColor Gray
}

# Create ZIP archive
$ZIP_PATH = "target\release\$APP_NAME-$VERSION-windows.zip"
Write-Host "==> Creating ZIP archive..." -ForegroundColor Yellow

if (Test-Path $ZIP_PATH) {
    Remove-Item $ZIP_PATH
}

Compress-Archive -Path "$DIST_DIR\*" -DestinationPath $ZIP_PATH

# Cleanup distribution directory
Remove-Item -Recurse -Force $DIST_DIR

Write-Host ""
Write-Host "==> Done!" -ForegroundColor Green
Write-Host "    Binary: $BINARY_PATH" -ForegroundColor White
Write-Host "    ZIP: $ZIP_PATH" -ForegroundColor White
Write-Host ""
Write-Host "To run: Execute $BINARY_NAME.exe from the extracted ZIP" -ForegroundColor Cyan
