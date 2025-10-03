@echo off
REM Windows cross-platform build script (via cross)

setlocal enabledelayedexpansion

echo [INFO] Start cross-platform builds...

REM Check cargo
where cargo >nul 2>nul
if %errorlevel% neq 0 (
  echo [ERROR] cargo not found; install Rust toolchain
  exit /b 1
)

REM Check cross
where cross >nul 2>nul
if %errorlevel% neq 0 (
  echo [WARN] cross not found; installing (requires network)...
  cargo install cross --git https://github.com/cross-rs/cross
)

REM Output dir
if not exist "target\releases" mkdir "target\releases"

REM Supported targets
set targets=x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-gnu aarch64-pc-windows-msvc x86_64-pc-windows-msvc

set success_count=0
set total_count=0

REM Override by CLI args
if not "%~1"=="" (
  set targets=%*
)

REM Count
for %%t in (%targets%) do (
  set /a total_count+=1 >nul
)

REM Build each
for %%t in (%targets%) do (
  echo [INFO] Build target %%t
  cross build --target %%t --release -p filter-repo-rs
  if !errorlevel! equ 0 (
    echo [INFO] %%t build succeeded
    set /a success_count+=1 >nul

    REM Copy binary
    set "binary_name=filter-repo-rs"
    set "source_path=target\%%t\release\!binary_name!"

    REM Add .exe on Windows targets
    echo %%t | findstr /i "windows" >nul
    if !errorlevel! equ 0 (
      set "source_path=!source_path!.exe"
      set "binary_name=!binary_name!.exe"
    )

    if exist "!source_path!" (
      set "dest_name=!binary_name!-%%t"
      copy "!source_path!" "target\releases\!dest_name!" >nul
      echo [INFO] Copied to target\releases\!dest_name!
    ) else (
      echo [WARN] Artifact missing: !source_path!
    )
  ) else (
    echo [ERROR] %%t build failed
  )
  echo ----------------------------------------
)

echo [INFO] Done: %success_count%/%total_count% succeeded
echo [INFO] Artifacts:
dir /b target\releases\

endlocal
